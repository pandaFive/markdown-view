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
    /// ファイル変更検知（canonicalize済みパス）
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

/// 監視実行中ハンドル
///
/// Drop時に監視スレッドと通知タスクを停止する。
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

            if tokio::time::timeout(Duration::from_secs(2), &mut runtime.notify_task)
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
        unreachable!("AppModeは単一ファイルまたはディレクトリのいずれか")
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
                                    let path = match event.path.canonicalize() {
                                        Ok(p) => p,
                                        Err(e) => {
                                            tracing::warn!(
                                                "[markdown-view] イベントパスの正規化に失敗（スキップ）: {} ({})",
                                                event.path.display(),
                                                e
                                            );
                                            continue;
                                        }
                                    };
                                    send_watcher_message(
                                        &rt_tx,
                                        WatcherMessage::FileChanged(path),
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
                    tracing::warn!("[markdown-view] 初期化エラーの通知先が既に閉じています");
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
    let canonical_base_dir = dir_path.clone();
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let thread_shutdown_flag = shutdown_flag.clone();

    let watcher_thread = std::thread::Builder::new()
        .name("markdown-view-watcher-dir".to_string())
        .spawn(move || {
        let rt_tx = tx;
        let debouncer = new_debouncer(
            Duration::from_millis(DEBOUNCE_MS),
            move |res: std::result::Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify::Error,
            >| {
                match res {
                    Ok(events) => {
                        // 変更された.mdファイルを収集（重複排除）
                        let mut notified = std::collections::HashSet::new();
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
                            let path = match event.path.canonicalize() {
                                Ok(p) => p,
                                Err(e) => {
                                    tracing::warn!(
                                        "[markdown-view] イベントパスの正規化に失敗（スキップ）: {} ({})",
                                        event.path.display(),
                                        e
                                    );
                                    continue;
                                }
                            };
                            // canonicalize後のパスがベースディレクトリ内であることを確認
                            // （symlink経由でディレクトリ外のファイルが変更された場合を防止）
                            if !path.starts_with(&canonical_base_dir) {
                                tracing::warn!(
                                    "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                                    path.display()
                                );
                                continue;
                            }
                            // 隠しファイル除外（canonicalize後のパスで判定）
                            // symlink経由で隠しディレクトリ内のファイルにアクセスするケースを防止
                            if is_hidden_relative(&path, &canonical_base_dir) {
                                continue;
                            }
                            if notified.insert(path.clone()) {
                                send_watcher_message(
                                    &rt_tx,
                                    WatcherMessage::FileChanged(path),
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
            },
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
            // フォールバック: ファイル名と親ディレクトリが一致するかで判定
            event_path.file_name() == target_path.file_name()
                && event_path.parent() == target_path.parent()
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
        let path = PathBuf::from("/tmp/test.md");
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
}
