use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// watcher の稼働状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatcherHealth {
    /// watcher thread 起動から init 完了まで。先行 failure があれば Alive へは遷移しない
    Starting,
    /// watcher は正常に稼働中
    Alive,
    /// watcher は監視品質が劣化、または panic で停止している
    Failed(WatcherFailureKind),
    /// 停止処理中
    Stopping,
    /// 停止完了
    Stopped,
}

/// watcher failure の分類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatcherFailureKind {
    /// notify callback がエラーを返した
    Notify,
    /// watcher thread が panic した
    ThreadPanic,
    /// shutdown を隔離する blocking task が panic した
    ShutdownTaskPanic,
    /// watcher thread の停止が timeout した
    ShutdownTimedOut,
    /// merge forwarder task が panic した
    ForwarderTaskPanic,
    /// merge forwarder が予期せず停止した
    ForwarderStopped,
}

#[derive(Debug, Clone)]
pub(super) struct WatcherHealthState {
    state: Arc<AtomicU8>,
}

impl WatcherHealthState {
    const STARTING: u8 = 0;
    const ALIVE: u8 = 1;
    const FAILED_NOTIFY: u8 = 2;
    const FAILED_THREAD_PANIC: u8 = 3;
    const STOPPING: u8 = 4;
    const STOPPED: u8 = 5;
    const FAILED_SHUTDOWN_TASK_PANIC: u8 = 6;
    const FAILED_SHUTDOWN_TIMED_OUT: u8 = 7;
    const FAILED_FORWARDER_TASK_PANIC: u8 = 8;
    const FAILED_FORWARDER_STOPPED: u8 = 9;

    pub(super) fn new_starting() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(Self::STARTING)),
        }
    }

    #[cfg(test)]
    pub(super) fn new_alive() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(Self::ALIVE)),
        }
    }

    pub(super) fn load(&self) -> WatcherHealth {
        match self.state.load(Ordering::Acquire) {
            Self::STARTING => WatcherHealth::Starting,
            Self::ALIVE => WatcherHealth::Alive,
            Self::FAILED_NOTIFY => WatcherHealth::Failed(WatcherFailureKind::Notify),
            Self::FAILED_THREAD_PANIC => WatcherHealth::Failed(WatcherFailureKind::ThreadPanic),
            Self::FAILED_SHUTDOWN_TASK_PANIC => {
                WatcherHealth::Failed(WatcherFailureKind::ShutdownTaskPanic)
            }
            Self::FAILED_SHUTDOWN_TIMED_OUT => {
                WatcherHealth::Failed(WatcherFailureKind::ShutdownTimedOut)
            }
            Self::FAILED_FORWARDER_TASK_PANIC => {
                WatcherHealth::Failed(WatcherFailureKind::ForwarderTaskPanic)
            }
            Self::FAILED_FORWARDER_STOPPED => {
                WatcherHealth::Failed(WatcherFailureKind::ForwarderStopped)
            }
            Self::STOPPING => WatcherHealth::Stopping,
            Self::STOPPED => WatcherHealth::Stopped,
            invalid => {
                debug_assert!(false, "不正なwatcher health state: {}", invalid);
                WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
            }
        }
    }

    pub(super) fn store_alive_if_starting(&self) {
        let _ = self.state.compare_exchange(
            Self::STARTING,
            Self::ALIVE,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(super) fn store_stopping_if_not_failed(&self) {
        self.store_if_not_failed(Self::STOPPING);
    }

    pub(super) fn store_stopped_if_not_failed(&self) {
        self.store_if_not_failed(Self::STOPPED);
    }

    pub(super) fn store_failed(&self, kind: WatcherFailureKind) {
        let raw = match kind {
            WatcherFailureKind::Notify => Self::FAILED_NOTIFY,
            WatcherFailureKind::ThreadPanic => Self::FAILED_THREAD_PANIC,
            WatcherFailureKind::ShutdownTaskPanic => Self::FAILED_SHUTDOWN_TASK_PANIC,
            WatcherFailureKind::ShutdownTimedOut => Self::FAILED_SHUTDOWN_TIMED_OUT,
            WatcherFailureKind::ForwarderTaskPanic => Self::FAILED_FORWARDER_TASK_PANIC,
            WatcherFailureKind::ForwarderStopped => Self::FAILED_FORWARDER_STOPPED,
        };
        self.store_if_not_failed(raw);
    }

    fn store_if_not_failed(&self, raw: u8) {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if Self::is_failed_raw(current) {
                return;
            }
            match self
                .state
                .compare_exchange(current, raw, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    fn is_failed_raw(raw: u8) -> bool {
        matches!(
            raw,
            Self::FAILED_NOTIFY
                | Self::FAILED_THREAD_PANIC
                | Self::FAILED_SHUTDOWN_TASK_PANIC
                | Self::FAILED_SHUTDOWN_TIMED_OUT
                | Self::FAILED_FORWARDER_TASK_PANIC
                | Self::FAILED_FORWARDER_STOPPED
        )
    }
}

/// 監視実行中ランタイム
///
/// 明示停止は `shutdown().await` が正規経路。
/// `Drop` は停止要求と内部転送タスクの abort だけを行い、watcher thread の join は待たない。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_alive_if_startingは先行failedを上書きしない() {
        let health_state = WatcherHealthState::new_starting();

        health_state.store_failed(WatcherFailureKind::Notify);
        health_state.store_alive_if_starting();

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
    }
}
