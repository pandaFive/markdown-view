//! WebSocket向けの更新ブロードキャストを管理する。

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;

use super::files::{
    build_change_broadcast_message, build_change_error_log_message_without_receivers,
};
use super::messages::BroadcastMessage;
use super::state::AppState;
use crate::watcher::{WatchError, WatchEvent};

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
        send_broadcast_message(state.tx(), message);
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
) {
    if let Err(error) = tx.send(message) {
        let failed_message = error.0;
        tracing::warn!(
            failed_message = ?failed_message,
            "[markdown-view] ファイル変更通知の送信に失敗しました"
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
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                WatchEvent::FileChanged(changed_path) => {
                    notify_update(&state, &changed_path).await;
                }
                WatchEvent::Error(error) => {
                    broadcast_error(&state, &error);
                }
            }
        }
        tracing::info!("[markdown-view] ファイル変更通知タスクが終了しました");
    })
}

/// ファイル監視エラーをブロードキャストする
///
/// `notify_update` の受信者なし経路は変更由来の読込前エラーをログに留めるが、
/// 監視エラー通知は受信者の有無に関わらず送信を試みる。
fn broadcast_error(state: &AppState, error: &WatchError) {
    send_broadcast_message(
        state.tx(),
        BroadcastMessage::Error(format!("ファイル監視エラー: {}", error.user_message())),
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::sync::{broadcast, mpsc};
    use tracing_test::traced_test;

    use super::*;
    use crate::server::{AppMode, AppState, MAX_FILE_SIZE};

    #[traced_test]
    #[test]
    fn test_send_broadcast_message_送信失敗はwarnログに残す() {
        let (tx, rx) = broadcast::channel(1);
        drop(rx);

        send_broadcast_message(&tx, BroadcastMessage::Error("test error".to_string()));

        assert!(logs_contain("ファイル変更通知の送信に失敗しました"));
        assert!(logs_contain("test error"));
    }

    #[traced_test]
    #[test]
    fn test_broadcast_error_送信失敗はwarnログに残す() {
        let base_dir = tempfile::tempdir().unwrap();
        let state = create_directory_state(base_dir.path());

        broadcast_error(&state, &WatchError::notify("watch failure"));

        assert!(logs_contain("ファイル変更通知の送信に失敗しました"));
        assert!(logs_contain("watch failure"));
    }

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードで相対パス算出失敗時は送信をスキップ() {
        let base_dir = tempfile::tempdir().unwrap();
        std::fs::write(base_dir.path().join("README.md"), "# README").unwrap();

        let outside_dir = tempfile::tempdir().unwrap();
        let outside_file = outside_dir.path().join("outside.md");
        std::fs::write(&outside_file, "# outside").unwrap();
        let outside_canonical = outside_file.canonicalize().unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        notify_update(&state, &outside_canonical).await;
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードで読み込み失敗時はerrorを送信する() {
        let base_dir = tempfile::tempdir().unwrap();
        let target = base_dir.path().join("README.md");
        std::fs::write(&target, "# before").unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&target).unwrap();
        notify_update(&state, &target).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル読み込みエラー"));
                assert!(
                    message.contains("README.md"),
                    "エラーメッセージにファイル名が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_ファイル名不明時はdisplay表示がエラーに含まれる() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("dummy.md");
        std::fs::write(&file_path, "# dummy").unwrap();

        let state = create_single_file_state(&file_path);
        let mut rx = state.tx().subscribe();

        notify_update(&state, Path::new("/")).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(
                    message.contains("/"),
                    "ファイル名不明時はdisplay()表示が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで読み込み失敗時はファイル名を含むエラーを送信する(
    ) {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル検証エラー"));
                assert!(message.contains("test.md"));
                assert!(!message.contains("No such file"));
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
    async fn test_notify_update_受信者ゼロ時の検証エラーはwarnログに残す() {
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
        assert!(logs_contain("missing.md"));
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
    async fn test_notify_update_ディレクトリモード受信者ゼロ時の読込失敗は相対パスでwarnログに残す()
    {
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

        assert!(logs_contain(
            "WebSocket受信者がいないため更新時ファイル変更エラーをローカル記録しました"
        ));
        assert!(logs_contain("更新時読み込みエラー"));
        assert!(!logs_contain("更新時ファイル検証失敗"));
        assert!(logs_contain("docs/guide.md"));
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

        forwarder.await.unwrap();
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn create_single_file_state(file_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new(
            AppMode::new_single_file(file_path).unwrap(),
            false,
            None,
            tx,
        )
    }

    fn create_directory_state(dir_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new(AppMode::new_directory(dir_path).unwrap(), false, None, tx)
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
