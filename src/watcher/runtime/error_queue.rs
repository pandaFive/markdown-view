use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use super::super::WatchError;
use super::dispatch::send_error_event;

pub(super) const WATCHER_ERROR_QUEUE_CAPACITY: usize = 64;
/// ring queue の evict 境界を短いテストで固定するための小容量。
#[cfg(test)]
pub(super) const WATCHER_ERROR_MESSAGE_BUFFER: usize = 8;
pub(super) struct PriorityErrorSender {
    pub(super) queue: Arc<PriorityErrorQueue>,
}

impl PriorityErrorSender {
    fn new(queue: Arc<PriorityErrorQueue>) -> Self {
        queue.sender_count.fetch_add(1, Ordering::Relaxed);
        Self { queue }
    }

    pub(super) fn send(&self, error: WatchError, label: &str) {
        send_error_event(self, error, label);
    }
}

impl Clone for PriorityErrorSender {
    fn clone(&self) -> Self {
        Self::new(self.queue.clone())
    }
}

impl Drop for PriorityErrorSender {
    fn drop(&mut self) {
        if self.queue.sender_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            let mut state = self.queue.state.lock().expect("priority error queue mutex");
            state.closed = true;
            drop(state);
            self.queue.notify.notify_waiters();
        }
    }
}

pub(super) struct PriorityErrorReceiver {
    pub(super) queue: Arc<PriorityErrorQueue>,
}

impl PriorityErrorReceiver {
    pub(super) async fn recv(&mut self) -> Option<WatchError> {
        loop {
            let notified = {
                let mut state = self.queue.state.lock().expect("priority error queue mutex");
                if let Some(error) = state.items.pop_front() {
                    return Some(error);
                }
                if state.closed {
                    return None;
                }
                self.queue.notify.notified()
            };
            notified.await;
        }
    }

    pub(super) fn try_recv(
        &mut self,
    ) -> std::result::Result<WatchError, mpsc::error::TryRecvError> {
        let mut state = self.queue.state.lock().expect("priority error queue mutex");
        if let Some(error) = state.items.pop_front() {
            Ok(error)
        } else if state.closed {
            Err(mpsc::error::TryRecvError::Disconnected)
        } else {
            Err(mpsc::error::TryRecvError::Empty)
        }
    }
}

impl Drop for PriorityErrorReceiver {
    fn drop(&mut self) {
        let mut state = self.queue.state.lock().expect("priority error queue mutex");
        state.closed = true;
        state.items.clear();
        drop(state);
        self.queue.notify.notify_waiters();
    }
}

pub(super) struct PriorityErrorQueue {
    pub(super) state: Mutex<PriorityErrorQueueState>,
    pub(super) notify: tokio::sync::Notify,
    sender_count: AtomicUsize,
    pub(super) capacity: usize,
}

pub(super) struct PriorityErrorQueueState {
    pub(super) items: VecDeque<WatchError>,
    pub(super) closed: bool,
}

pub(super) fn priority_error_channel(
    capacity: usize,
) -> (PriorityErrorSender, PriorityErrorReceiver) {
    let queue = Arc::new(PriorityErrorQueue {
        state: Mutex::new(PriorityErrorQueueState {
            items: VecDeque::with_capacity(capacity),
            closed: false,
        }),
        notify: tokio::sync::Notify::new(),
        sender_count: AtomicUsize::new(0),
        capacity,
    });
    (
        PriorityErrorSender::new(queue.clone()),
        PriorityErrorReceiver { queue },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    use crate::watcher::WatchError;

    #[traced_test]
    #[test]
    fn test_send_error_eventはreceiver_drop後にclosedとして扱い未配送errorを保持しない() {
        let (error_tx, error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);

        error_tx.send(
            WatchError::notify("queued before receiver drop"),
            "error receiver closedテスト",
        );
        assert_eq!(
            error_tx
                .queue
                .state
                .lock()
                .expect("priority error queue mutex")
                .items
                .len(),
            1
        );
        drop(error_rx);

        error_tx.send(
            WatchError::notify("secret receiver closed detail"),
            "error receiver closedテスト",
        );

        let state = error_tx
            .queue
            .state
            .lock()
            .expect("priority error queue mutex");
        assert!(state.closed);
        assert!(state.items.is_empty());
        drop(state);
        assert!(logs_contain(
            "watcher error channel が閉じているため異常通知を破棄しました"
        ));
        assert!(logs_contain("error_kind=Notify"));
        assert!(!logs_contain("secret receiver closed detail"));
        assert!(!logs_contain("queued before receiver drop"));
    }

    #[test]
    fn test_send_error_eventはcapacity超過時に最古をevictして最新を保持する() {
        let (error_tx, mut error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);

        for index in 0..super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER + 4 {
            error_tx.send(
                WatchError::notify(format!("error-{index}")),
                "error ring queueテスト",
            );
        }

        let mut received = Vec::new();
        while let Ok(error) = error_rx.try_recv() {
            received.push(error.detail().to_string());
        }

        assert_eq!(
            received.len(),
            super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER
        );
        assert_eq!(received.first().map(String::as_str), Some("error-4"));
        assert_eq!(received.last().map(String::as_str), Some("error-11"));
    }
}
