use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::task::JoinHandle;

use super::broadcast::spawn_watch_event_forwarder;
use super::state::AppState;
use crate::watcher::Watcher;

/// 監視スレッドと転送タスクを束ねるサービス
pub struct WatchService {
    watcher: Option<Watcher>,
    watch_forwarder: Option<JoinHandle<()>>,
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

    /// 監視スレッドと転送タスクを停止する
    pub async fn shutdown(mut self) {
        if let Some(watcher) = self.watcher.take() {
            watcher.shutdown();
        }
        if let Some(watch_forwarder) = self.watch_forwarder.take() {
            shutdown_watch_forwarder(watch_forwarder).await;
        }
    }
}

async fn shutdown_watch_forwarder(mut watch_forwarder: JoinHandle<()>) {
    match tokio::time::timeout(
        Duration::from_secs(WATCH_FORWARDER_SHUTDOWN_TIMEOUT_SECS),
        &mut watch_forwarder,
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
            watch_forwarder.abort();
            if let Err(e) = watch_forwarder.await {
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
        service.shutdown().await;
    }
}
