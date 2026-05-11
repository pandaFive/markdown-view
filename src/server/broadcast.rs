//! WebSocket向けの更新ブロードキャストを管理する。

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::files::{
    build_change_broadcast_message, build_change_error_log_message_without_receivers,
};
use super::messages::BroadcastMessage;
use super::state::AppState;
use crate::watcher::{WatchError, WatchEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchForwarderEventKind {
    FileChanged,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WatchForwarderSnapshot {
    pub last_event_kind: Option<WatchForwarderEventKind>,
    pub receiver_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct WatchForwarderDiagnostics {
    last_event_kind: Arc<AtomicU8>,
    tx: tokio::sync::broadcast::Sender<BroadcastMessage>,
}

impl WatchForwarderDiagnostics {
    const NONE: u8 = 0;
    const FILE_CHANGED: u8 = 1;
    const ERROR: u8 = 2;

    pub(super) fn new(tx: tokio::sync::broadcast::Sender<BroadcastMessage>) -> Self {
        Self {
            last_event_kind: Arc::new(AtomicU8::new(Self::NONE)),
            tx,
        }
    }

    pub(super) fn record(&self, kind: WatchForwarderEventKind) {
        let raw = match kind {
            WatchForwarderEventKind::FileChanged => Self::FILE_CHANGED,
            WatchForwarderEventKind::Error => Self::ERROR,
        };
        self.last_event_kind.store(raw, Ordering::Release);
    }

    pub(super) fn snapshot(&self) -> WatchForwarderSnapshot {
        let last_event_kind = match self.last_event_kind.load(Ordering::Acquire) {
            Self::NONE => None,
            Self::FILE_CHANGED => Some(WatchForwarderEventKind::FileChanged),
            Self::ERROR => Some(WatchForwarderEventKind::Error),
            invalid => {
                debug_assert!(false, "不正なforwarder event kind: {}", invalid);
                None
            }
        };
        WatchForwarderSnapshot {
            last_event_kind,
            receiver_count: self.tx.receiver_count(),
        }
    }
}

pub(super) struct WatchForwarderHandle {
    pub(super) task: JoinHandle<()>,
    pub(super) diagnostics: WatchForwarderDiagnostics,
}

/// ファイル変更時にbroadcastで全クライアントに通知する
///
/// ディレクトリモードでは変更ファイルの相対パスを`file`フィールドに含め、
/// クライアント側でアクティブタブの更新判定に使用する。
/// ファイル検証や読み込みに失敗した場合はエラーメッセージをbroadcastする。
/// 受信者がゼロの場合は検証・読み込み前エラーだけをログに残す。
/// ログ記録中に受信者が増えた場合は、通常のbroadcast経路にフォールバックする。
pub async fn notify_update(state: &AppState, changed_file: &Path) {
    if state.tx().receiver_count() == 0 {
        log_change_error_without_receivers(state, changed_file).await;
        if state.tx().receiver_count() == 0 {
            return;
        }
    }

    if let Some(message) = build_change_broadcast_message(state, changed_file).await {
        send_broadcast_message(state.tx(), message, "ファイル変更通知");
    }
}

/// WebSocket受信者がいない変更イベントのエラーをローカルログに残す。
async fn log_change_error_without_receivers(state: &AppState, changed_file: &Path) {
    if let Some(message) =
        build_change_error_log_message_without_receivers(state, changed_file).await
    {
        tracing::warn!(
            message = %message,
            "[markdown-view] WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        );
    }
}

fn send_broadcast_message(
    tx: &tokio::sync::broadcast::Sender<BroadcastMessage>,
    message: BroadcastMessage,
    context: &'static str,
) {
    if let Err(error) = tx.send(message) {
        let summary = error.0.log_summary();
        tracing::warn!(
            message_kind = summary.message_kind,
            has_file = summary.has_file,
            "[markdown-view] {}の送信に失敗しました",
            context
        );
    }
}

/// 監視イベントをWebSocketブロードキャストへ転送する
///
/// ファイルシステムの監視イベントを受け取り、以下のように処理する：
/// - `WatchEvent::FileChanged` → `notify_update` により変更内容を再描画してブロードキャスト
/// - `WatchEvent::Error` → `broadcast_error` によりエラーメッセージをブロードキャスト
///
/// mpscチャネル `rx` が閉じられるとループを終了し、タスクは完了する。
pub(super) fn spawn_watch_event_forwarder(
    state: Arc<AppState>,
    mut rx: mpsc::Receiver<WatchEvent>,
) -> WatchForwarderHandle {
    let diagnostics = WatchForwarderDiagnostics::new(state.tx().clone());
    let task_diagnostics = diagnostics.clone();
    let task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                WatchEvent::FileChanged(changed_path) => {
                    task_diagnostics.record(WatchForwarderEventKind::FileChanged);
                    notify_update(&state, &changed_path).await;
                }
                WatchEvent::Error(error) => {
                    task_diagnostics.record(WatchForwarderEventKind::Error);
                    broadcast_error(&state, &error);
                }
            }
        }
        let snapshot = task_diagnostics.snapshot();
        tracing::info!(
            last_event_kind = ?snapshot.last_event_kind,
            receiver_count = snapshot.receiver_count,
            "[markdown-view] ファイル変更通知タスクが終了しました"
        );
    });
    WatchForwarderHandle { task, diagnostics }
}

/// ファイル監視エラーをブロードキャストする
///
/// `notify_update` の受信者なし経路は変更由来の読込前エラーをログに留めるが、
/// 監視エラー通知は受信者の有無に関わらず送信を試みる。
fn broadcast_error(state: &AppState, error: &WatchError) {
    let receiver_count = state.tx().receiver_count();
    if receiver_count == 0 {
        tracing::warn!(
            receiver_count,
            watch_error_kind = ?error.kind(),
            "[markdown-view] WebSocket受信者がいないため監視エラーをローカル記録しました"
        );
    }
    send_broadcast_message(
        state.tx(),
        BroadcastMessage::Error(format!("ファイル監視エラー: {}", error.user_message())),
        "監視エラー通知",
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use tokio::sync::{broadcast, mpsc};
    use tokio::time::timeout;
    use tracing_test::traced_test;

    use super::*;
    use crate::server::{AppMode, AppState, MAX_FILE_SIZE};

    #[test]
    fn test_watch_forwarder_diagnostics_初期状態は最後のイベントなし() {
        let (tx, rx1) = broadcast::channel(4);
        let rx2 = tx.subscribe();
        let rx3 = tx.subscribe();
        let diagnostics = WatchForwarderDiagnostics::new(tx);

        let snapshot = diagnostics.snapshot();
        drop((rx1, rx2, rx3));

        assert_eq!(snapshot.last_event_kind, None);
        assert_eq!(snapshot.receiver_count, 3);
    }

    #[test]
    fn test_watch_forwarder_diagnostics_filechangedを記録できる() {
        let (tx, _rx) = broadcast::channel(4);
        let diagnostics = WatchForwarderDiagnostics::new(tx);

        diagnostics.record(WatchForwarderEventKind::FileChanged);
        let snapshot = diagnostics.snapshot();

        assert_eq!(
            snapshot.last_event_kind,
            Some(WatchForwarderEventKind::FileChanged)
        );
        assert_eq!(snapshot.receiver_count, 1);
    }

    #[test]
    fn test_watch_forwarder_diagnostics_errorを記録できる() {
        let (tx, _rx) = broadcast::channel(4);
        let diagnostics = WatchForwarderDiagnostics::new(tx);

        diagnostics.record(WatchForwarderEventKind::Error);
        let snapshot = diagnostics.snapshot();

        assert_eq!(
            snapshot.last_event_kind,
            Some(WatchForwarderEventKind::Error)
        );
        assert_eq!(snapshot.receiver_count, 1);
    }

    #[traced_test]
    #[test]
    fn test_send_broadcast_message_送信失敗は本文を伏せてwarnログに残す() {
        let (tx, rx) = broadcast::channel(1);
        drop(rx);

        send_broadcast_message(
            &tx,
            BroadcastMessage::Error("secret broadcast error".to_string()),
            "ファイル変更通知",
        );

        assert!(logs_contain("ファイル変更通知の送信に失敗しました"));
        assert!(logs_contain("message_kind"));
        assert!(logs_contain("error"));
        assert!(!logs_contain("secret broadcast error"));
    }

    #[traced_test]
    #[test]
    fn test_send_broadcast_message_update送信失敗はhtml本文をログに出さない() {
        let (tx, rx) = broadcast::channel(1);
        drop(rx);
        let message = BroadcastMessage::Update(crate::template::UpdateMessage::new(
            crate::renderer::render_markdown("secret markdown body"),
            crate::toc::generate_toc("# secret heading"),
            Some("secret/path.md".to_string()),
        ));

        send_broadcast_message(&tx, message, "ファイル変更通知");

        assert!(logs_contain("ファイル変更通知の送信に失敗しました"));
        assert!(logs_contain("message_kind"));
        assert!(logs_contain("update"));
        assert!(logs_contain("has_file=true"));
        assert!(!logs_contain("secret markdown body"));
        assert!(!logs_contain("secret heading"));
        assert!(!logs_contain("secret/path.md"));
    }

    #[traced_test]
    #[test]
    fn test_broadcast_error_送信失敗はエラー詳細をログに出さない() {
        let base_dir = tempfile::tempdir().unwrap();
        let state = create_directory_state(base_dir.path());

        broadcast_error(&state, &WatchError::notify("secret watch failure"));

        assert!(logs_contain("監視エラー通知の送信に失敗しました"));
        assert!(logs_contain("message_kind"));
        assert!(logs_contain("error"));
        assert!(!logs_contain("secret watch failure"));
    }

    #[traced_test]
    #[test]
    fn test_broadcast_error_受信者ゼロ時はkindだけログに残す() {
        let base_dir = tempfile::tempdir().unwrap();
        let state = create_directory_state(base_dir.path());

        broadcast_error(&state, &WatchError::notify("secret watch failure"));

        assert!(logs_contain(
            "WebSocket受信者がいないため監視エラーをローカル記録しました"
        ));
        assert!(logs_contain("receiver_count=0"));
        assert!(logs_contain("watch_error_kind=Notify"));
        assert!(!logs_contain("secret watch failure"));
    }

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードでbase外パスは検証エラーを送信する() {
        let base_dir = tempfile::tempdir().unwrap();
        std::fs::write(base_dir.path().join("README.md"), "# README").unwrap();

        let outside_dir = tempfile::tempdir().unwrap();
        let outside_file = outside_dir.path().join("outside.md");
        std::fs::write(&outside_file, "# outside").unwrap();
        let outside_canonical = outside_file.canonicalize().unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        notify_update(&state, &outside_canonical).await;

        let received = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("Updateのbroadcastを期待")
            .unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル検証エラー"));
                assert!(
                    message.contains("ディレクトリ外へのアクセスは禁止されています"),
                    "Traversalのエラー文言を期待: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードで削除済みファイルは送信をスキップする() {
        let base_dir = tempfile::tempdir().unwrap();
        let target = base_dir.path().join("README.md");
        std::fs::write(&target, "# before").unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&target).unwrap();
        notify_update(&state, &target).await;

        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードではchanged_fileでなくexpectedを更新する() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("dummy.md");
        std::fs::write(&file_path, "# dummy").unwrap();

        let state = create_single_file_state(&file_path);
        let mut rx = state.tx().subscribe();

        notify_update(&state, Path::new("/")).await;

        let received = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("検証エラーのbroadcastを期待")
            .unwrap();
        match received {
            BroadcastMessage::Update(update) => {
                assert!(update.content().as_str().contains("dummy"));
                assert!(update.file().is_none());
            }
            other => panic!("Updateメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで削除済みファイルは検証エラーを送信する() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        let received = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("検証エラーのbroadcastを期待")
            .unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル検証エラー"));
                assert!(message.contains("ファイルが見つかりません"));
                assert!(message.contains("test.md"));
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードでサイズ超過時はtoo_largeエラーを送信する() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("large.md", "# large");
        let mut rx = state.tx().subscribe();

        let file = std::fs::File::options()
            .write(true)
            .open(&file_path)
            .unwrap();
        // metadata のサイズ上限分岐を固定する。open可否分岐は環境依存が強いため別対象。
        file.set_len(MAX_FILE_SIZE + 1).unwrap();

        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイルサイズが上限"));
                assert!(message.contains("large.md"));
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで非utf8ファイルはnot_utf8エラーを送信する() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("binary.md", "# valid");
        let mut rx = state.tx().subscribe();

        std::fs::write(&file_path, b"\xff\xfe\x80\x81").unwrap();
        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("UTF-8"));
                assert!(message.contains("binary.md"));
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで受信者ゼロ時は送信をスキップする() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let rx = state.tx().subscribe();
        drop(rx);

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        let mut rx = state.tx().subscribe();
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_単一ファイルモード受信者ゼロ時のnot_foundはwarnログに残す() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("missing.md", "# missing");
        let rx = state.tx().subscribe();
        drop(rx);

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        assert!(logs_contain(
            "WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        ));
        assert!(logs_contain("更新時ファイル検証失敗"));
        assert!(logs_contain("ファイルが見つかりません"));
        assert!(!logs_contain("更新時読み込みエラー"));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_受信者ゼロ時のサイズ超過はwarnログに残す() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("large.md", "# large");
        let rx = state.tx().subscribe();
        drop(rx);

        let file = std::fs::File::options()
            .write(true)
            .open(&file_path)
            .unwrap();
        file.set_len(MAX_FILE_SIZE + 1).unwrap();

        notify_update(&state, &file_path).await;

        assert!(logs_contain(
            "WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        ));
        assert!(logs_contain("更新時読み込みエラー"));
        assert!(logs_contain("ファイルサイズが上限"));
        assert!(logs_contain("large.md"));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_ディレクトリモード受信者ゼロ時の削除済みファイルはwarnログに残さない(
    ) {
        let base_dir = tempfile::tempdir().unwrap();
        let nested_dir = base_dir.path().join("docs");
        std::fs::create_dir(&nested_dir).unwrap();
        let target = nested_dir.join("guide.md");
        std::fs::write(&target, "# guide").unwrap();

        let state = create_directory_state(base_dir.path());
        let rx = state.tx().subscribe();
        drop(rx);

        std::fs::remove_file(&target).unwrap();
        notify_update(&state, &target).await;

        assert!(!logs_contain(
            "WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        ));
        assert!(!logs_contain("更新時ファイル検証失敗"));
        assert!(!logs_contain("更新時読み込みエラー"));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_ディレクトリモード受信者ゼロ時のmdディレクトリはwarnログに残す() {
        let base_dir = tempfile::tempdir().unwrap();
        let target = base_dir.path().join("docs.md");
        std::fs::create_dir(&target).unwrap();

        let state = create_directory_state(base_dir.path());
        let rx = state.tx().subscribe();
        drop(rx);

        notify_update(&state, &target).await;

        assert!(logs_contain(
            "WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        ));
        assert!(logs_contain("更新時ファイル検証失敗"));
        assert!(logs_contain("通常ファイルではありません"));
        assert!(!logs_contain("更新時読み込みエラー"));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_受信者ゼロ時の正常更新はwarnログに残さない() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("normal.md", "# normal");
        let rx = state.tx().subscribe();
        drop(rx);

        notify_update(&state, &file_path).await;

        assert!(!logs_contain(
            "WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        ));
        assert!(!logs_contain("更新時読み込みエラー"));
        assert!(!logs_contain("更新時ファイル検証失敗"));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_受信者ゼロ時の非utf8は本文読込せずwarnログに残さない() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("binary.md", "# valid");
        let rx = state.tx().subscribe();
        drop(rx);

        std::fs::write(&file_path, b"\xff\xfe\x80\x81").unwrap();
        notify_update(&state, &file_path).await;

        assert!(!logs_contain(
            "WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        ));
        assert!(!logs_contain("UTF-8"));
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで正常更新時はupdateを送信する() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("hello.md", "# hello world");
        let mut rx = state.tx().subscribe();

        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Update(update) => {
                assert!(update.content().as_str().contains("hello world"));
                assert!(update.toc().as_str().contains("hello-world"));
                assert!(update.file().is_none());
            }
            other => panic!("Updateメッセージを期待したが {:?} を受信", other),
        }
    }

    #[traced_test]
    #[tokio::test]
    async fn test_watch_forwarder_自然終了ログに診断情報を含める() {
        let base_dir = tempfile::tempdir().unwrap();
        std::fs::write(base_dir.path().join("README.md"), "# before").unwrap();
        let state = Arc::new(create_directory_state(base_dir.path()));
        let (tx, rx) = mpsc::channel(4);
        let handle = spawn_watch_event_forwarder(state, rx);

        tx.send(WatchEvent::Error(WatchError::notify("forwarder-log-test")))
            .await
            .unwrap();
        drop(tx);

        handle.task.await.unwrap();

        assert!(logs_contain("ファイル変更通知タスクが終了しました"));
        assert!(logs_contain("last_event_kind=Some(Error)"));
        assert!(logs_contain("receiver_count=0"));
    }

    #[tokio::test]
    async fn test_watch_forwarder_filechanged実経路で診断状態を更新する() {
        let base_dir = tempfile::tempdir().unwrap();
        let target = base_dir.path().join("README.md");
        std::fs::write(&target, "# changed").unwrap();
        let state = Arc::new(create_directory_state(base_dir.path()));
        let mut broadcast_rx = state.tx().subscribe();
        let (tx, rx) = mpsc::channel(4);
        let handle = spawn_watch_event_forwarder(state, rx);

        tx.send(WatchEvent::FileChanged(target)).await.unwrap();

        let received = timeout(Duration::from_secs(1), broadcast_rx.recv())
            .await
            .expect("Updateのbroadcastを期待")
            .unwrap();
        assert!(matches!(received, BroadcastMessage::Update(_)));
        assert_eq!(
            handle.diagnostics.snapshot().last_event_kind,
            Some(WatchForwarderEventKind::FileChanged)
        );

        drop(tx);
        handle.task.await.unwrap();
    }

    #[tokio::test]
    async fn test_spawn_watch_event_forwarder_監視エラーをbroadcastする() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let state = Arc::new(state);
        let mut broadcast_rx = state.tx().subscribe();
        let (tx, rx) = mpsc::channel(4);
        let forwarder = spawn_watch_event_forwarder(state.clone(), rx);

        tx.send(WatchEvent::Error(WatchError::notify("テストエラー")))
            .await
            .unwrap();
        drop(tx);

        let received = broadcast_rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains(
                    "ファイル監視エラー: 通知ライブラリエラーが発生しました: テストエラー"
                ));
                assert!(file_path.exists());
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }

        forwarder.task.await.unwrap();
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn create_single_file_state(file_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new_with_tokio_memo_fs(
            AppMode::new_single_file(file_path).unwrap(),
            false,
            None,
            tx,
        )
    }

    fn create_directory_state(dir_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new_with_tokio_memo_fs(AppMode::new_directory(dir_path).unwrap(), false, None, tx)
    }

    fn create_single_file_state_with_fixture(
        name: &str,
        content: &str,
    ) -> (tempfile::TempDir, PathBuf, AppState) {
        let (dir, file_path) = create_markdown_fixture(name, content);
        let state = create_single_file_state(&file_path);
        (dir, file_path, state)
    }
}
