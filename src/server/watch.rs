use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;

use super::broadcast::{spawn_watch_event_forwarder, WatchForwarderHandle};
use super::state::AppState;
use crate::watcher::{Watcher, WatcherHealth};

/// 監視スレッドと転送タスクを束ねるサービス
pub struct WatchService {
    watcher: Option<Watcher>,
    watch_forwarder: Option<WatchForwarderHandle>,
}

/// 監視イベント転送タスクの停止待機秒数
const WATCH_FORWARDER_SHUTDOWN_TIMEOUT_SECS: u64 = 2;

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
            watcher.shutdown()
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
    let WatchForwarderHandle {
        mut task,
        diagnostics: _,
    } = watch_forwarder;
    match tokio::time::timeout(
        Duration::from_secs(WATCH_FORWARDER_SHUTDOWN_TIMEOUT_SECS),
        &mut task,
    )
    .await
    {
        Ok(join_result) => {
            if let Err(e) = join_result {
                tracing::warn!(
                    "[markdown-view] 監視イベント転送タスクの終了待機に失敗: {}",
                    e
                );
            }
        }
        Err(_) => {
            tracing::warn!(
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

    use super::WatchService;
    use crate::server::AppMode;
    use crate::server::AppState;
    use crate::watcher::WatcherHealth;

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
        assert_eq!(service.shutdown().await, WatcherHealth::Stopped);
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
}
