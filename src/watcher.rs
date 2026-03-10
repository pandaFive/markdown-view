use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, DebouncedEventKind};
use tokio::sync::{mpsc, oneshot};

use crate::server::AppMode;

/// ファイル監視が外部へ公開するイベント
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// ファイル変更検知
    ///
    /// 削除イベントではcanonicalizeに失敗しうるため、生のパスを保持する。
    FileChanged(PathBuf),
    /// 監視ランタイムエラー（notify debouncerコールバック由来）
    Error(String),
}

#[derive(Debug, Clone)]
enum WatchStrategy {
    SingleFile { target_path: PathBuf },
    Directory { base_dir: PathBuf },
}

#[derive(Clone)]
struct WatchConfig {
    watch_dir: PathBuf,
    recursive_mode: RecursiveMode,
    thread_name: &'static str,
    unexpected_exit: &'static str,
    start_error_prefix: &'static str,
    watch_error_prefix: &'static str,
    panic_message: &'static str,
    change_label: &'static str,
    error_label: &'static str,
    strategy: WatchStrategy,
}

fn send_watch_event(tx: &mpsc::Sender<WatchEvent>, event: WatchEvent, label: &str) {
    match tx.try_send(event) {
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
/// notify から tokio へ橋渡しするチャネル容量
const WATCHER_MESSAGE_BUFFER: usize = 32;

type InitResult = std::result::Result<(), String>;

/// 監視実行中ランタイム
///
/// `shutdown()` は監視スレッドを停止する。
/// `Drop` は待機せず同じ停止処理を行う。
pub struct Watcher {
    runtime: Option<WatchRuntime>,
}

/// shutdown() のグレースフル停止待機秒数
const SHUTDOWN_TIMEOUT_SECS: u64 = 2;

struct WatchRuntime {
    shutdown_flag: Arc<AtomicBool>,
    watcher_thread: std::thread::JoinHandle<()>,
}

impl WatchRuntime {
    /// 監視スレッドに停止を通知し、完了を待機する
    ///
    /// `SHUTDOWN_TIMEOUT_SECS` 以内にスレッドが終了しない場合はリークさせる
    /// （プロセス終了時にOSが回収する）。
    fn stop(self) {
        self.shutdown_flag.store(true, Ordering::Release);
        self.watcher_thread.thread().unpark();

        let start = std::time::Instant::now();
        while !self.watcher_thread.is_finished() {
            if start.elapsed() > Duration::from_secs(SHUTDOWN_TIMEOUT_SECS) {
                tracing::warn!("[markdown-view] 監視スレッドの停止がタイムアウトしました");
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Err(e) = self.watcher_thread.join() {
            tracing::warn!(
                "[markdown-view] 監視スレッドの停止中にパニックを検出: {:?}",
                e
            );
        }
    }
}

impl Watcher {
    /// 監視を開始し、監視イベント受信用チャネルを返す
    pub async fn spawn(mode: AppMode) -> Result<(Self, mpsc::Receiver<WatchEvent>)> {
        let config = WatchConfig::from_mode(&mode)?;
        let (tx, rx) = mpsc::channel::<WatchEvent>(WATCHER_MESSAGE_BUFFER);
        let (init_tx, init_rx) = oneshot::channel::<InitResult>();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let thread_shutdown_flag = shutdown_flag.clone();
        let unexpected_exit = config.unexpected_exit;
        let watcher_thread = spawn_watcher_thread(config, tx, init_tx, thread_shutdown_flag)?;

        await_watcher_init(init_rx, unexpected_exit).await?;
        Ok((Self::new(shutdown_flag, watcher_thread), rx))
    }

    fn new(shutdown_flag: Arc<AtomicBool>, watcher_thread: std::thread::JoinHandle<()>) -> Self {
        Self {
            runtime: Some(WatchRuntime {
                shutdown_flag,
                watcher_thread,
            }),
        }
    }

    /// 監視スレッドを停止する
    pub fn shutdown(mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
        }
    }
}

impl WatchConfig {
    fn from_mode(mode: &AppMode) -> Result<Self> {
        if let Some(file_path) = mode.single_file() {
            let watch_dir = file_path
                .parent()
                .context("親ディレクトリが取得できません")?
                .to_path_buf();
            Ok(Self {
                watch_dir,
                recursive_mode: RecursiveMode::NonRecursive,
                thread_name: "markdown-view-watcher-file",
                unexpected_exit: "ファイル監視スレッドが予期せず終了しました",
                start_error_prefix: "ファイル監視の開始に失敗",
                watch_error_prefix: "ファイル監視エラー",
                panic_message: "単一ファイル監視スレッドがパニックで停止しました",
                change_label: "単一ファイル更新",
                error_label: "単一ファイル監視エラー",
                strategy: WatchStrategy::SingleFile {
                    target_path: file_path.to_path_buf(),
                },
            })
        } else if let Some(dir_path) = mode.directory() {
            Ok(Self {
                watch_dir: dir_path.to_path_buf(),
                recursive_mode: RecursiveMode::Recursive,
                thread_name: "markdown-view-watcher-dir",
                unexpected_exit: "ディレクトリ監視スレッドが予期せず終了しました",
                start_error_prefix: "ディレクトリ監視の開始に失敗",
                watch_error_prefix: "ディレクトリ監視エラー",
                panic_message: "ディレクトリ監視スレッドがパニックで停止しました",
                change_label: "ディレクトリ更新",
                error_label: "ディレクトリ監視エラー",
                strategy: WatchStrategy::Directory {
                    base_dir: dir_path.to_path_buf(),
                },
            })
        } else {
            anyhow::bail!("未知のAppModeです")
        }
    }
}

fn spawn_watcher_thread(
    config: WatchConfig,
    tx: mpsc::Sender<WatchEvent>,
    init_tx: oneshot::Sender<InitResult>,
    thread_shutdown_flag: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>> {
    let thread_name = config.thread_name.to_string();
    let spawn_context = format!("監視スレッド {} の起動に失敗", config.thread_name);
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let rt_tx = tx;
            let panic_tx = rt_tx.clone();
            let watch_dir = config.watch_dir;
            let recursive_mode = config.recursive_mode;
            let start_error_prefix = config.start_error_prefix;
            let watch_error_prefix = config.watch_error_prefix;
            let panic_message = config.panic_message;
            let change_label = config.change_label;
            let error_label = config.error_label;
            let strategy = config.strategy;
            let mut init_tx = Some(init_tx);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let debouncer = new_debouncer(
                    Duration::from_millis(DEBOUNCE_MS),
                    move |res: std::result::Result<Vec<DebouncedEvent>, notify::Error>| match res {
                        Ok(events) => {
                            handle_debounced_events(&strategy, events, &rt_tx, change_label);
                        }
                        Err(e) => {
                            tracing::warn!("[markdown-view] {}: {}", watch_error_prefix, e);
                            send_watch_event(&rt_tx, WatchEvent::Error(e.to_string()), error_label);
                        }
                    },
                );

                let mut debouncer = match debouncer {
                    Ok(d) => d,
                    Err(e) => {
                        send_init_result(
                            &mut init_tx,
                            Err(format!("debouncerの初期化に失敗: {}", e)),
                        );
                        return;
                    }
                };

                if let Err(e) = debouncer.watcher().watch(&watch_dir, recursive_mode) {
                    send_init_result(&mut init_tx, Err(format!("{}: {}", start_error_prefix, e)));
                    return;
                }

                send_init_result(&mut init_tx, Ok(()));

                keep_watcher_thread_alive(&thread_shutdown_flag);
            }));

            if let Err(panic_payload) = result {
                let panic_detail = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "不明なパニック".to_string()
                };
                tracing::error!("[markdown-view] {}: {}", panic_message, panic_detail);
                send_watch_event(
                    &panic_tx,
                    WatchEvent::Error(format!(
                        "監視スレッドがパニックで停止しました: {}",
                        panic_detail
                    )),
                    error_label,
                );
            }
        })
        .context(spawn_context)
}

fn send_init_result(init_tx: &mut Option<oneshot::Sender<InitResult>>, result: InitResult) {
    if let Some(tx) = init_tx.take() {
        if tx.send(result).is_err() {
            tracing::warn!("[markdown-view] 初期化通知先が既に閉じています");
        }
    } else {
        tracing::warn!(
            "[markdown-view] send_init_resultが二重に呼び出されました（結果を破棄）: {:?}",
            result.err()
        );
    }
}

fn keep_watcher_thread_alive(shutdown_flag: &AtomicBool) {
    while !shutdown_flag.load(Ordering::Acquire) {
        std::thread::park_timeout(Duration::from_millis(WATCHER_THREAD_PARK_MS));
    }
}

async fn await_watcher_init(
    init_rx: oneshot::Receiver<InitResult>,
    unexpected_exit: &'static str,
) -> Result<()> {
    match init_rx.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => anyhow::bail!(e),
        Err(_) => anyhow::bail!(unexpected_exit),
    }
}

fn handle_debounced_events(
    strategy: &WatchStrategy,
    events: Vec<DebouncedEvent>,
    tx: &mpsc::Sender<WatchEvent>,
    change_label: &str,
) {
    match strategy {
        WatchStrategy::SingleFile { target_path } => {
            for event in events {
                if is_content_change_event(&event.kind) && is_target_file(&event.path, target_path)
                {
                    send_watch_event(
                        tx,
                        WatchEvent::FileChanged(event.path.clone()),
                        change_label,
                    );
                    break;
                }
            }
        }
        WatchStrategy::Directory { base_dir } => {
            let mut notified = HashSet::new();
            for event in events {
                if !is_content_change_event(&event.kind) {
                    continue;
                }
                let is_md = event
                    .path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
                if !is_md {
                    continue;
                }
                if !is_within_base_dir(&event.path, base_dir) {
                    tracing::warn!(
                        "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                        event.path.display()
                    );
                    continue;
                }
                if is_hidden_relative(&event.path, base_dir) {
                    continue;
                }
                let normalized_event_path = normalize_lexical_path(&event.path);
                if notified.insert(normalized_event_path.clone()) {
                    send_watch_event(
                        tx,
                        WatchEvent::FileChanged(normalized_event_path),
                        change_label,
                    );
                }
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::AppMode;

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn spawn_idle_watcher_thread(shutdown_flag: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || keep_watcher_thread_alive(&shutdown_flag))
    }

    #[test]
    fn test_連続更新イベントも更新対象に含まれる() {
        assert!(is_content_change_event(&DebouncedEventKind::Any));
        assert!(is_content_change_event(&DebouncedEventKind::AnyContinuous));
    }

    #[tokio::test]
    async fn test_mpscチャネルでwatcheventを送受信できる() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(32);

        // FileChanged variant
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, "# test").unwrap();
        tx.send(WatchEvent::FileChanged(path.clone()))
            .await
            .unwrap();
        match rx.recv().await.unwrap() {
            WatchEvent::FileChanged(p) => assert_eq!(p, path),
            WatchEvent::Error(_) => panic!("FileChangedを期待したがErrorを受信"),
        }

        // Error variant
        tx.send(WatchEvent::Error("テストエラー".to_string()))
            .await
            .unwrap();
        match rx.recv().await.unwrap() {
            WatchEvent::Error(msg) => assert_eq!(msg, "テストエラー"),
            WatchEvent::FileChanged(_) => panic!("Errorを期待したがFileChangedを受信"),
        }
    }

    #[test]
    fn test_send_watch_event_チャネル満杯時はメッセージを破棄してブロックしない() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        // チャネルを満杯にする
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .unwrap();

        // 満杯時にtry_sendで即座に破棄される（ブロックしない）
        send_watch_event(
            &tx,
            WatchEvent::Error("満杯時テスト".to_string()),
            "満杯時テスト",
        );

        // 最初のメッセージのみ受信できる
        match rx.blocking_recv().unwrap() {
            WatchEvent::FileChanged(path) => {
                assert_eq!(path, first);
            }
            WatchEvent::Error(_) => {
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
        let (dir, target) = create_markdown_fixture("target.md", "# target");
        let other = dir.path().join("other.md");
        std::fs::write(&other, "# other").unwrap();

        let canonical_target = target.canonicalize().unwrap();
        assert!(is_target_file(&target, &canonical_target));
        assert!(!is_target_file(&other, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗時は同名かつ同一親ディレクトリでフォールバック一致() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let canonical_target = target.canonicalize().unwrap();

        // event_pathのcanonicalizeを失敗させるために削除
        std::fs::remove_file(&target).unwrap();

        assert!(is_target_file(&target, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗時は非正規化親パスでも一致判定できる() {
        let (dir, target) = create_markdown_fixture("target.md", "# target");
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        let canonical_target = target.canonicalize().unwrap();

        // event_pathのcanonicalizeを失敗させるために削除
        std::fs::remove_file(&target).unwrap();
        let non_normalized = dir.path().join("sub/../target.md");

        assert!(is_target_file(&non_normalized, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗フォールバックでも親ディレクトリ不一致はfalse() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
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
    async fn test_watcher_spawn_単一ファイルモードでイベント受信できる() {
        let (_dir, file_path) = create_markdown_fixture("watch.md", "# before");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (watcher, mut rx) = Watcher::spawn(mode).await.unwrap();

        tokio::fs::write(&file_path, "# after").await.unwrap();

        let received = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        match received {
            WatchEvent::FileChanged(changed_path) => {
                assert_eq!(
                    changed_path.file_name(),
                    Some(std::ffi::OsStr::new("watch.md"))
                );
            }
            WatchEvent::Error(message) => {
                panic!("FileChangedを期待したが Error({}) を受信", message)
            }
        }

        watcher.shutdown();
    }

    #[tokio::test]
    async fn test_watcher_shutdownで監視スレッドを停止できる() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag.clone(), watcher_thread);
        watcher.shutdown();
        assert!(shutdown_flag.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_watcher_dropはフォールバック停止を行う() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag.clone(), watcher_thread);

        drop(watcher);
        assert!(shutdown_flag.load(Ordering::Acquire));
    }
}
