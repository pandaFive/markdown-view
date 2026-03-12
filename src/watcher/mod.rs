mod error;
mod runtime;
mod strategy;

pub use self::error::{WatchError, WatchErrorKind};
pub use self::runtime::Watcher;

use std::path::PathBuf;

/// ファイル監視が外部へ公開するイベント
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// ファイル変更検知
    ///
    /// 削除イベントではcanonicalizeに失敗しうるため、生のパスを保持する。
    FileChanged(PathBuf),
    /// 監視ランタイムエラー（notify debouncerコールバック由来）
    Error(WatchError),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::sync::mpsc;

    use super::error::WatchErrorKind;
    use super::{WatchError, WatchEvent};

    #[tokio::test]
    async fn test_mpscチャネルでwatcheventを送受信できる() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(32);

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

        tx.send(WatchEvent::Error(WatchError::notify("テストエラー")))
            .await
            .unwrap();
        match rx.recv().await.unwrap() {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "テストエラー");
            }
            WatchEvent::FileChanged(_) => panic!("Errorを期待したがFileChangedを受信"),
        }
    }

    #[test]
    fn test_filechangedはpathbufを保持する() {
        let path = PathBuf::from("notes.md");
        assert_eq!(
            WatchEvent::FileChanged(path.clone()),
            WatchEvent::FileChanged(path)
        );
    }
}
