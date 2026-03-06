use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::mpsc;

use crate::server::{notify_update, AppState, BroadcastMessage};

/// ファイル監視からtokioタスクへのメッセージ型
enum WatcherMessage {
    /// ファイル変更検知
    ///
    /// 削除イベントではcanonicalizeに失敗しうるため、生のパスを保持する。
    FileChanged(PathBuf),
    /// 監視ランタイムエラー（notify debouncerコールバック由来）
    WatchError(String),
}

fn send_watcher_message(tx: &mpsc::Sender<WatcherMessage>, msg: WatcherMessage, label: &str) {
    match tx.try_send(msg) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!(
                "[markdown-view] 監視メッセージ送信キューが満杯のため破棄: {}",
                label
            );
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::warn!("[markdown-view] 通知チャネルが閉じています: {}", label);
        }
    }
}

/// デバウンス間隔（ミリ秒）
const DEBOUNCE_MS: u64 = 300;
/// 監視スレッドのpark待機間隔（ミリ秒）
const WATCHER_THREAD_PARK_MS: u64 = 250;
/// shutdown() のグレースフル停止待機秒数
const SHUTDOWN_TIMEOUT_SECS: u64 = 2;

/// 監視実行中ハンドル
///
/// `shutdown()` は通知タスクの終了を2秒（`SHUTDOWN_TIMEOUT_SECS`）待つ
/// グレースフル停止を行う。
/// `Drop` は待機せず即時abortするフォールバック停止を行う。
pub struct WatchHandle {
    runtime: Option<WatchRuntime>,
}

struct WatchRuntime {
    shutdown_flag: Arc<AtomicBool>,
    watcher_thread: std::thread::JoinHandle<()>,
    notify_task: tokio::task::JoinHandle<()>,
}

impl WatchHandle {
    fn new(
        shutdown_flag: Arc<AtomicBool>,
        watcher_thread: std::thread::JoinHandle<()>,
        notify_task: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self {
            runtime: Some(WatchRuntime {
                shutdown_flag,
                watcher_thread,
                notify_task,
            }),
        }
    }

    /// 監視スレッドと通知タスクを停止する
    pub async fn shutdown(mut self) {
        if let Some(mut runtime) = self.runtime.take() {
            runtime.shutdown_flag.store(true, Ordering::Release);
            runtime.watcher_thread.thread().unpark();

            if let Err(e) = runtime.watcher_thread.join() {
                tracing::warn!(
                    "[markdown-view] 監視スレッドの停止中にパニックを検出: {:?}",
                    e
                );
            }

            if tokio::time::timeout(
                Duration::from_secs(SHUTDOWN_TIMEOUT_SECS),
                &mut runtime.notify_task,
            )
            .await
            .is_err()
            {
                tracing::warn!("[markdown-view] 通知タスク停止がタイムアウトしたためabortします");
                runtime.notify_task.abort();
                let _ = runtime.notify_task.await;
            }
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_flag.store(true, Ordering::Release);
            runtime.watcher_thread.thread().unpark();
            if let Err(e) = runtime.watcher_thread.join() {
                tracing::warn!(
                    "[markdown-view] 監視スレッドのDrop停止中にパニックを検出: {:?}",
                    e
                );
            }
            runtime.notify_task.abort();
        }
    }
}

/// ファイルまたはディレクトリの監視を開始する
///
/// notify + debouncer でファイル変更を検知し、
/// tokioランタイムにブリッジしてbroadcastで通知する
pub async fn watch_path(state: Arc<AppState>) -> Result<WatchHandle> {
    if let Some(file_path) = state.mode().single_file().map(Path::to_path_buf) {
        watch_single_file(state, file_path).await
    } else if let Some(dir_path) = state.mode().directory().map(Path::to_path_buf) {
        watch_directory(state, dir_path).await
    } else {
        anyhow::bail!("未知のAppModeです")
    }
}

/// 単一ファイルの監視
async fn watch_single_file(state: Arc<AppState>, file_path: PathBuf) -> Result<WatchHandle> {
    // 監視対象ディレクトリ（ファイルの親ディレクトリ）
    let watch_dir = file_path
        .parent()
        .context("親ディレクトリが取得できません")?
        .to_path_buf();

    let target_path = file_path.clone();

    // tokio::sync::mpscでnotifyからtokioにブリッジ
    let (tx, mut rx) = mpsc::channel::<WatcherMessage>(32);

    // 初期化エラーを親タスクに伝播するための oneshot チャネル
    let (init_tx, init_rx) = tokio::sync::oneshot::channel::<std::result::Result<(), String>>();
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let thread_shutdown_flag = shutdown_flag.clone();

    // debouncerをstd::threadで起動（notifyはsyncスレッドで動作）
    let watcher_thread = std::thread::Builder::new()
        .name("markdown-view-watcher-file".to_string())
        .spawn(move || {
            let rt_tx = tx;
            let panic_tx = rt_tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let debouncer = new_debouncer(
                    Duration::from_millis(DEBOUNCE_MS),
                    move |res: std::result::Result<
                        Vec<notify_debouncer_mini::DebouncedEvent>,
                        notify::Error,
                    >| {
                        match res {
                            Ok(events) => {
                                for event in events {
                                    if is_content_change_event(&event.kind) {
                                        // 対象ファイルの変更のみ通知
                                        if is_target_file(&event.path, &target_path) {
                                            send_watcher_message(
                                                &rt_tx,
                                                WatcherMessage::FileChanged(event.path.clone()),
                                                "単一ファイル更新",
                                            );
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("[markdown-view] ファイル監視エラー: {}", e);
                                send_watcher_message(
                                    &rt_tx,
                                    WatcherMessage::WatchError(e.to_string()),
                                    "単一ファイル監視エラー",
                                );
                            }
                        }
                    },
                );

                let mut debouncer = match debouncer {
                    Ok(d) => d,
                    Err(e) => {
                        if init_tx
                            .send(Err(format!("debouncerの初期化に失敗: {}", e)))
                            .is_err()
                        {
                            tracing::warn!(
                                "[markdown-view] 初期化エラーの通知先が既に閉じています"
                            );
                        }
                        return;
                    }
                };

                if let Err(e) = debouncer
                    .watcher()
                    .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
                {
                    if init_tx
                        .send(Err(format!("ファイル監視の開始に失敗: {}", e)))
                        .is_err()
                    {
                        tracing::warn!("[markdown-view] 初期化エラーの通知先が既に閉じています");
                    }
                    return;
                }

                if init_tx.send(Ok(())).is_err() {
                    tracing::warn!("[markdown-view] 初期化成功の通知先が既に閉じています");
                }

                // スレッドを維持（debouncerのlifetimeのため、spurious wakeupで再parkする）
                while !thread_shutdown_flag.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(WATCHER_THREAD_PARK_MS));
                }
            }));

            if result.is_err() {
                tracing::error!("[markdown-view] 単一ファイル監視スレッドがパニックで停止しました");
                send_watcher_message(
                    &panic_tx,
                    WatcherMessage::WatchError("監視スレッドがパニックで停止しました".to_string()),
                    "単一ファイル監視パニック",
                );
            }
        })
        .context("監視スレッドの起動に失敗")?;

    // 初期化結果を待機
    match init_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => anyhow::bail!(e),
        Err(_) => anyhow::bail!("ファイル監視スレッドが予期せず終了しました"),
    }

    let notify_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                WatcherMessage::FileChanged(changed_path) => {
                    notify_update(&state, &changed_path).await;
                }
                WatcherMessage::WatchError(error_msg) => {
                    broadcast_error(&state, &error_msg);
                }
            }
        }
        tracing::warn!(
            "[markdown-view] ファイル変更通知タスクが終了しました。ライブリロードは無効です"
        );
    });

    Ok(WatchHandle::new(shutdown_flag, watcher_thread, notify_task))
}

/// ディレクトリの再帰監視
async fn watch_directory(state: Arc<AppState>, dir_path: PathBuf) -> Result<WatchHandle> {
    let (tx, mut rx) = mpsc::channel::<WatcherMessage>(32);

    let (init_tx, init_rx) = tokio::sync::oneshot::channel::<std::result::Result<(), String>>();

    let watch_dir = dir_path.clone();
    // イベントコールバック内で相対パスの隠しファイル判定に使用
    // dir_pathはAppMode::new_directory()でcanonicalize済み
    let base_dir = dir_path.clone();
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let thread_shutdown_flag = shutdown_flag.clone();

    let watcher_thread = std::thread::Builder::new()
        .name("markdown-view-watcher-dir".to_string())
        .spawn(move || {
            let rt_tx = tx;
            let panic_tx = rt_tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let debouncer = new_debouncer(
                    Duration::from_millis(DEBOUNCE_MS),
                    move |res: std::result::Result<
                        Vec<notify_debouncer_mini::DebouncedEvent>,
                        notify::Error,
                    >| {
                        match res {
                            Ok(events) => {
                                // 変更された.mdファイルを収集（重複排除）
                                let mut notified: std::collections::HashSet<PathBuf> =
                                    std::collections::HashSet::new();
                                for event in events {
                                    if !is_content_change_event(&event.kind) {
                                        continue;
                                    }
                                    // .md拡張子フィルタ
                                    let is_md = event
                                        .path
                                        .extension()
                                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
                                    if !is_md {
                                        continue;
                                    }
                                    // パスがベースディレクトリ内であることを確認する。
                                    // 削除イベントではcanonicalizeできないため、語彙的正規化でフォールバックする。
                                    if !is_within_base_dir(&event.path, &base_dir) {
                                        tracing::warn!(
                                            "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                                            event.path.display()
                                        );
                                        continue;
                                    }
                                    // 隠しファイル除外（canonicalize後のパスで判定）
                                    // symlink経由で隠しディレクトリ内のファイルにアクセスするケースを防止
                                    if is_hidden_relative(&event.path, &base_dir) {
                                        continue;
                                    }
                                    let normalized_event_path = normalize_lexical_path(&event.path);
                                    if notified.insert(normalized_event_path.clone()) {
                                        send_watcher_message(
                                            &rt_tx,
                                            WatcherMessage::FileChanged(normalized_event_path),
                                            "ディレクトリ更新",
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("[markdown-view] ディレクトリ監視エラー: {}", e);
                                send_watcher_message(
                                    &rt_tx,
                                    WatcherMessage::WatchError(e.to_string()),
                                    "ディレクトリ監視エラー",
                                );
                            }
                        }
                    }
                );

                let mut debouncer = match debouncer {
                    Ok(d) => d,
                    Err(e) => {
                        if init_tx
                            .send(Err(format!("debouncerの初期化に失敗: {}", e)))
                            .is_err()
                        {
                            tracing::warn!("[markdown-view] 初期化エラーの通知先が既に閉じています");
                        }
                        return;
                    }
                };

                // ディレクトリモードでは再帰監視
                if let Err(e) = debouncer
                    .watcher()
                    .watch(&watch_dir, notify::RecursiveMode::Recursive)
                {
                    if init_tx
                        .send(Err(format!("ディレクトリ監視の開始に失敗: {}", e)))
                        .is_err()
                    {
                        tracing::warn!("[markdown-view] 初期化エラーの通知先が既に閉じています");
                    }
                    return;
                }

                if init_tx.send(Ok(())).is_err() {
                    tracing::warn!("[markdown-view] 初期化成功の通知先が既に閉じています");
                }

                // スレッドを維持（debouncerのlifetimeのため、spurious wakeupで再parkする）
                while !thread_shutdown_flag.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(WATCHER_THREAD_PARK_MS));
                }
            }));

            if result.is_err() {
                tracing::error!("[markdown-view] ディレクトリ監視スレッドがパニックで停止しました");
                send_watcher_message(
                    &panic_tx,
                    WatcherMessage::WatchError("監視スレッドがパニックで停止しました".to_string()),
                    "ディレクトリ監視パニック",
                );
            }
        })
        .context("監視スレッドの起動に失敗")?;

    match init_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => anyhow::bail!(e),
        Err(_) => anyhow::bail!("ディレクトリ監視スレッドが予期せず終了しました"),
    }

    let notify_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                WatcherMessage::FileChanged(changed_path) => {
                    notify_update(&state, &changed_path).await;
                }
                WatcherMessage::WatchError(error_msg) => {
                    broadcast_error(&state, &error_msg);
                }
            }
        }
        tracing::warn!(
            "[markdown-view] ディレクトリ変更通知タスクが終了しました。ライブリロードは無効です"
        );
    });

    Ok(WatchHandle::new(shutdown_flag, watcher_thread, notify_task))
}

/// レンダリング更新が必要なイベント種別か判定する
fn is_content_change_event(kind: &DebouncedEventKind) -> bool {
    matches!(
        kind,
        DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous
    )
}

/// ベースディレクトリからの相対パスに隠しコンポーネントが含まれるか判定する
///
/// ベースディレクトリ自体が`.`で始まるパスに含まれる場合でも
/// 正しく動作するよう、相対パス部分のみをチェックする。
///
/// ## Fail-safe動作
/// `strip_prefix`とcanonicalizeの両方に失敗した場合は`true`を返し、
/// 安全側に倒す（隠しファイルとして扱い処理をスキップする）。
fn is_hidden_relative(path: &Path, base: &Path) -> bool {
    match path.strip_prefix(base) {
        Ok(relative) => relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        Err(_) => {
            // strip_prefix失敗時はcanonicalizeして再試行
            let canonical_path = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: パス正規化失敗（元パスで再試行）: {} ({})",
                        path.display(), e
                    );
                    path.to_path_buf()
                }
            };
            let canonical_base = match base.canonicalize() {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: ベース正規化失敗（元パスで再試行）: {} ({})",
                        base.display(), e
                    );
                    base.to_path_buf()
                }
            };
            match canonical_path.strip_prefix(&canonical_base) {
                Ok(relative) => relative
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
                Err(_) => {
                    // 相対パスが算出できない場合は安全側に倒す（隠しファイルとして除外）
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                        path.display()
                    );
                    true
                }
            }
        }
    }
}

/// パスが監視対象ファイルと一致するか判定する
///
/// target_pathは起動時にcanonicalize済みの絶対パス。
/// event_pathもcanonicalizeして比較し、失敗時はファイル名と親ディレクトリの両方で比較する。
fn is_target_file(event_path: &Path, target_path: &Path) -> bool {
    match event_path.canonicalize() {
        Ok(canonical) => canonical == *target_path,
        Err(e) => {
            tracing::warn!(
                "[markdown-view] パス正規化に失敗（ファイル名比較にフォールバック）: {} ({})",
                event_path.display(),
                e
            );
            // フォールバック:
            // 1) ファイル名一致
            // 2) 親ディレクトリを可能な限り正規化して一致判定
            if event_path.file_name() != target_path.file_name() {
                return false;
            }
            let Some(event_parent) = event_path.parent() else {
                return false;
            };
            let Some(target_parent) = target_path.parent() else {
                return false;
            };

            let event_parent_normalized = event_parent
                .canonicalize()
                .unwrap_or_else(|_| normalize_lexical_path(event_parent));
            let target_parent_normalized = target_parent
                .canonicalize()
                .unwrap_or_else(|_| normalize_lexical_path(target_parent));

            event_parent_normalized == target_parent_normalized
        }
    }
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn is_within_base_dir(path: &Path, base: &Path) -> bool {
    match path.canonicalize() {
        Ok(canonical_path) => canonical_path.starts_with(base),
        Err(_) => {
            let normalized_path = normalize_lexical_path(path);
            let normalized_base = normalize_lexical_path(base);
            normalized_path.starts_with(&normalized_base)
        }
    }
}

/// 監視エラーをbroadcastチャネル経由でWebSocketクライアントに通知する
///
/// `server.rs:notify_update`のエラーJSON送信パターンに合わせた形式で送信する。
/// 受信者がいない場合は正常（クライアント未接続時）。
fn broadcast_error(state: &AppState, error_msg: &str) {
    let _ = state.tx().send(BroadcastMessage::Error(format!(
        "ファイル監視エラー: {}",
        error_msg
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::AppMode;
    use tokio::sync::broadcast;

    #[test]
    fn test_連続更新イベントも更新対象に含まれる() {
        assert!(is_content_change_event(&DebouncedEventKind::Any));
        assert!(is_content_change_event(&DebouncedEventKind::AnyContinuous));
    }

    #[test]
    fn test_broadcast_errorがエラーjsonを送信する() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let file_path = tmp_dir.path().join("test.md");
        std::fs::write(&file_path, "# test").unwrap();

        let (tx, _rx) = broadcast::channel(16);
        let state = Arc::new(AppState::new(
            AppMode::new_single_file(&file_path).unwrap(),
            false,
            None,
            tx,
        ));
        let mut rx = state.tx().subscribe();

        broadcast_error(&state, "テストエラーメッセージ");

        let received = rx.try_recv().unwrap();
        match received {
            BroadcastMessage::Error(msg) => {
                assert_eq!(msg, "ファイル監視エラー: テストエラーメッセージ");
            }
            other => panic!("Errorを期待したが {:?} を受信", other),
        }
    }

    #[test]
    fn test_broadcast_errorは受信者なしでもパニックしない() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let file_path = tmp_dir.path().join("test.md");
        std::fs::write(&file_path, "# test").unwrap();

        let (tx, _rx) = broadcast::channel(16);
        let state = Arc::new(AppState::new(
            AppMode::new_single_file(&file_path).unwrap(),
            false,
            None,
            tx,
        ));
        // _rxをドロップして受信者をゼロにする
        drop(_rx);

        // パニックしないことを確認
        broadcast_error(&state, "受信者なしエラー");
    }

    #[tokio::test]
    async fn test_mpscチャネルでwatchermessageを送受信できる() {
        let (tx, mut rx) = mpsc::channel::<WatcherMessage>(32);

        // FileChanged variant
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, "# test").unwrap();
        tx.send(WatcherMessage::FileChanged(path.clone()))
            .await
            .unwrap();
        match rx.recv().await.unwrap() {
            WatcherMessage::FileChanged(p) => assert_eq!(p, path),
            WatcherMessage::WatchError(_) => panic!("FileChangedを期待したがWatchErrorを受信"),
        }

        // WatchError variant
        tx.send(WatcherMessage::WatchError("テストエラー".to_string()))
            .await
            .unwrap();
        match rx.recv().await.unwrap() {
            WatcherMessage::WatchError(msg) => assert_eq!(msg, "テストエラー"),
            WatcherMessage::FileChanged(_) => panic!("WatchErrorを期待したがFileChangedを受信"),
        }
    }

    #[test]
    fn test_send_watcher_message_チャネル満杯時はメッセージを破棄してブロックしない() {
        let (tx, mut rx) = mpsc::channel::<WatcherMessage>(1);
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        std::fs::write(&first, "# first").unwrap();
        // チャネルを満杯にする
        tx.blocking_send(WatcherMessage::FileChanged(first.clone()))
            .unwrap();

        // 満杯時にtry_sendで即座に破棄される（ブロックしない）
        send_watcher_message(
            &tx,
            WatcherMessage::WatchError("満杯時テスト".to_string()),
            "満杯時テスト",
        );

        // 最初のメッセージのみ受信できる
        match rx.blocking_recv().unwrap() {
            WatcherMessage::FileChanged(path) => {
                assert_eq!(path, first);
            }
            WatcherMessage::WatchError(_) => {
                panic!("最初のメッセージはFileChangedを期待")
            }
        }

        // 満杯時のメッセージは破棄されているため、追加メッセージはない
        assert!(
            rx.try_recv().is_err(),
            "満杯時のメッセージは破棄されているはず"
        );
    }

    #[test]
    fn test_is_target_file_正規化成功時は完全一致のみtrue() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.md");
        let other = dir.path().join("other.md");
        std::fs::write(&target, "# target").unwrap();
        std::fs::write(&other, "# other").unwrap();

        let canonical_target = target.canonicalize().unwrap();
        assert!(is_target_file(&target, &canonical_target));
        assert!(!is_target_file(&other, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗時は同名かつ同一親ディレクトリでフォールバック一致() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.md");
        std::fs::write(&target, "# target").unwrap();
        let canonical_target = target.canonicalize().unwrap();

        // event_pathのcanonicalizeを失敗させるために削除
        std::fs::remove_file(&target).unwrap();

        assert!(is_target_file(&target, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗時は非正規化親パスでも一致判定できる() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.md");
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(&target, "# target").unwrap();
        let canonical_target = target.canonicalize().unwrap();

        // event_pathのcanonicalizeを失敗させるために削除
        std::fs::remove_file(&target).unwrap();
        let non_normalized = dir.path().join("sub/../target.md");

        assert!(is_target_file(&non_normalized, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗フォールバックでも親ディレクトリ不一致はfalse() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.md");
        std::fs::write(&target, "# target").unwrap();
        let canonical_target = target.canonicalize().unwrap();

        let other_dir = tempfile::tempdir().unwrap();
        let other_path_same_name = other_dir.path().join("target.md");
        // ファイルを作らずcanonicalize失敗にする
        assert!(!is_target_file(&other_path_same_name, &canonical_target));
    }

    #[test]
    fn test_隠しファイル判定_相対パスのみチェック() {
        // ベースディレクトリ自体がドットで始まるパスに含まれるケース
        let base = Path::new("/home/user/.config/docs");
        let visible_file = Path::new("/home/user/.config/docs/README.md");
        let hidden_file = Path::new("/home/user/.config/docs/.secret/notes.md");
        let hidden_dotfile = Path::new("/home/user/.config/docs/.hidden.md");

        // ベースディレクトリの.configはチェック対象外
        assert!(!is_hidden_relative(visible_file, base));
        // 相対パス部分の.secretは隠しディレクトリ
        assert!(is_hidden_relative(hidden_file, base));
        // 相対パス部分の.hidden.mdは隠しファイル
        assert!(is_hidden_relative(hidden_dotfile, base));
    }

    #[test]
    fn test_隠しファイル判定_通常のベースディレクトリ() {
        let base = Path::new("/home/user/docs");
        let visible = Path::new("/home/user/docs/guide.md");
        let hidden = Path::new("/home/user/docs/.draft/wip.md");

        assert!(!is_hidden_relative(visible, base));
        assert!(is_hidden_relative(hidden, base));
    }

    #[test]
    fn test_隠しファイル判定_相対パス算出不可時は安全側で除外() {
        // ベースと完全に無関係なパス（strip_prefixもcanonicalizeも失敗するケース）
        let base = Path::new("/nonexistent/base/dir");
        let unrelated = Path::new("/completely/different/path/file.md");

        // fail-safe: trueを返す（隠しファイルとして除外）
        assert!(is_hidden_relative(unrelated, base));
    }

    #[test]
    fn test_is_within_base_dir_削除済みパスでもベース配下ならtrue() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub/../target.md");
        assert!(is_within_base_dir(&target, dir.path()));
    }

    #[test]
    fn test_is_within_base_dir_ベース外パスはfalse() {
        let base = Path::new("/tmp/base");
        let outside = Path::new("/tmp/other/target.md");
        assert!(!is_within_base_dir(outside, base));
    }

    #[tokio::test]
    async fn test_watchhandle_shutdownはタイムアウト後にabortする() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let thread_flag = shutdown_flag.clone();
        let watcher_thread = std::thread::spawn(move || {
            while !thread_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        });
        let notify_task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let handle = WatchHandle::new(shutdown_flag, watcher_thread, notify_task);
        let start = std::time::Instant::now();
        handle.shutdown().await;
        assert!(start.elapsed() >= Duration::from_secs(SHUTDOWN_TIMEOUT_SECS));
    }

    #[tokio::test]
    async fn test_watchhandle_dropはフォールバック停止でabortする() {
        struct TaskDropFlag(Arc<AtomicBool>);
        impl Drop for TaskDropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let thread_flag = shutdown_flag.clone();
        let watcher_thread = std::thread::spawn(move || {
            while !thread_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        });
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_for_task = dropped.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let notify_task = tokio::spawn(async move {
            let _guard = TaskDropFlag(dropped_for_task);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });

        let handle = WatchHandle::new(shutdown_flag, watcher_thread, notify_task);
        started_rx.await.unwrap();
        drop(handle);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
