use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::broadcast::{spawn_watch_event_forwarder, WatchForwarderHandle};
use super::state::AppState;
use crate::watcher::{Watcher, WatcherHealth, WATCH_SHUTDOWN_TIMEOUT_SECS};

/// 監視スレッドと転送タスクを束ねるサービス
pub struct WatchService {
    watcher: Option<Watcher>,
    watch_forwarder: Option<WatchForwarderHandle>,
}

impl WatchService {
    /// 状態に対応するファイル監視と転送タスクを起動する
    pub async fn start(state: Arc<AppState>) -> Result<Self> {
        let (watcher, watch_events) = Watcher::spawn(state.mode().clone()).await?;
        let watch_forwarder = spawn_watch_event_forwarder(state, watch_events);
        Ok(Self {
            watcher: Some(watcher),
            watch_forwarder: Some(watch_forwarder),
        })
    }

    /// watcher の現在状態を返す
    pub fn health(&self) -> WatcherHealth {
        self.watcher
            .as_ref()
            .map(Watcher::health)
            .unwrap_or(WatcherHealth::Stopped)
    }

    /// watcher が正常稼働中なら true
    pub fn is_alive(&self) -> bool {
        matches!(self.health(), WatcherHealth::Alive)
    }

    /// 監視スレッドと転送タスクを停止し、watcher の最終状態を返す
    pub async fn shutdown(mut self) -> WatcherHealth {
        let health = if let Some(watcher) = self.watcher.take() {
            watcher.shutdown().await
        } else {
            WatcherHealth::Stopped
        };
        if matches!(health, WatcherHealth::Failed(_)) {
            tracing::warn!(
                "[markdown-view] 失敗状態のwatcher serviceを停止しました: {:?}",
                health
            );
        }
        if let Some(watch_forwarder) = self.watch_forwarder.take() {
            shutdown_watch_forwarder(watch_forwarder).await;
        }
        health
    }
}

async fn shutdown_watch_forwarder(watch_forwarder: WatchForwarderHandle) {
    shutdown_watch_forwarder_with_timeout_secs(
        watch_forwarder,
        watch_forwarder_shutdown_timeout_secs(),
    )
    .await;
}

fn watch_forwarder_shutdown_timeout_secs() -> u64 {
    WATCH_SHUTDOWN_TIMEOUT_SECS
}

async fn shutdown_watch_forwarder_with_timeout_secs(
    watch_forwarder: WatchForwarderHandle,
    timeout_secs: u64,
) {
    let WatchForwarderHandle {
        mut task,
        diagnostics,
    } = watch_forwarder;
    let start = Instant::now();
    match tokio::time::timeout(Duration::from_secs(timeout_secs), &mut task).await {
        Ok(join_result) => {
            if let Err(e) = join_result {
                tracing::warn!(
                    "[markdown-view] 監視イベント転送タスクの終了待機に失敗: {}",
                    e
                );
            }
        }
        Err(_) => {
            let elapsed_ms = start.elapsed().as_millis();
            let snapshot = diagnostics.snapshot();
            tracing::warn!(
                elapsed_ms,
                timeout_secs,
                last_event_kind = ?snapshot.last_event_kind,
                receiver_count = snapshot.receiver_count,
                "[markdown-view] 監視イベント転送タスク停止がタイムアウトしたためabortします"
            );
            task.abort();
            if let Err(e) = task.await {
                if e.is_cancelled() {
                    return;
                }
                tracing::warn!(
                    "[markdown-view] 監視イベント転送タスクの終了待機に失敗: {}",
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::broadcast;
    use tokio::task;
    use tracing_test::traced_test;

    use super::super::broadcast::{
        WatchForwarderDiagnostics, WatchForwarderEventKind, WatchForwarderHandle,
    };
    use super::{
        shutdown_watch_forwarder_with_timeout_secs, watch_forwarder_shutdown_timeout_secs,
        WatchService,
    };
    use crate::server::AppMode;
    use crate::server::AppState;
    use crate::watcher::{WatcherHealth, WATCH_SHUTDOWN_TIMEOUT_SECS};

    #[tokio::test]
    async fn test_watch_service_開始と停止ができる() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("watch.md");
        std::fs::write(&file_path, "# watch").unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = Arc::new(AppState::new_with_tokio_memo_fs(
            AppMode::new_single_file(&file_path).unwrap(),
            false,
            None,
            tx,
        ));

        let service = WatchService::start(state).await.unwrap();
        let start = std::time::Instant::now();
        assert_eq!(service.shutdown().await, WatcherHealth::Stopped);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "watch service shutdown should not wait for watcher timeout"
        );
    }

    #[tokio::test]
    async fn test_watch_service_health_開始後はaliveを返す() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("watch-health.md");
        std::fs::write(&file_path, "# watch").unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = Arc::new(AppState::new_with_tokio_memo_fs(
            AppMode::new_single_file(&file_path).unwrap(),
            false,
            None,
            tx,
        ));

        let service = WatchService::start(state).await.unwrap();

        assert_eq!(service.health(), WatcherHealth::Alive);
        assert!(service.is_alive());

        assert_eq!(service.shutdown().await, WatcherHealth::Stopped);
    }

    #[test]
    fn test_watch_service_health_watcherなしはstoppedを返す() {
        let service = WatchService {
            watcher: None,
            watch_forwarder: None,
        };

        assert_eq!(service.health(), WatcherHealth::Stopped);
        assert!(!service.is_alive());
    }

    #[test]
    fn test_shutdown_watch_forwarderはwatcher共通timeout秒数を使う() {
        assert_eq!(
            watch_forwarder_shutdown_timeout_secs(),
            WATCH_SHUTDOWN_TIMEOUT_SECS
        );
    }

    #[traced_test]
    #[tokio::test]
    async fn test_shutdown_watch_forwarder_timeoutログに診断情報を含める() {
        let (tx, _rx) = broadcast::channel(4);
        let diagnostics = WatchForwarderDiagnostics::new(tx);
        diagnostics.record(WatchForwarderEventKind::FileChanged);
        let handle = WatchForwarderHandle {
            task: task::spawn(async {
                std::future::pending::<()>().await;
            }),
            diagnostics,
        };

        shutdown_watch_forwarder_with_timeout_secs(handle, 0).await;

        assert!(logs_contain(
            "監視イベント転送タスク停止がタイムアウトしたためabortします"
        ));
        assert!(logs_contain("elapsed_ms="));
        assert!(logs_contain("timeout_secs=0"));
        assert!(logs_contain("last_event_kind=Some(FileChanged)"));
        assert!(logs_contain("receiver_count=1"));
    }
}
