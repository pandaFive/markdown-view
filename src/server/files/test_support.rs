//! memo 経路テストの共通基盤。
//!
//! [`TempWorkspace`] は tempdir ベースのワークスペースを提供する。

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::server::messages::BroadcastMessage;
use crate::server::state::{AppMode, AppState};

use super::memo_fs::{
    always_ok_before_rename, before_rename_future, BeforeRenameCheck, MemoFs, MemoReadError,
    MemoWriteError, TokioMemoFs,
};

/// tempdir ベースのテスト用ワークスペース。
pub(crate) struct TempWorkspace {
    dir: tempfile::TempDir,
}

impl TempWorkspace {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            dir: tempfile::tempdir()?,
        })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// 相対パスにファイルを書き込む（必要なら親ディレクトリを作成）
    pub fn write_file(&self, rel: &Path, content: &str) -> io::Result<PathBuf> {
        if !rel.is_relative()
            || rel
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path must be relative and must not contain parent directory components",
            ));
        }

        let full = self.dir.path().join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, content)?;
        Ok(full)
    }

    /// 拡張子を補わずに `.md` ファイルを書く糖衣
    pub fn write_md(&self, rel: &Path, content: &str) -> io::Result<PathBuf> {
        self.write_file(rel, content)
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Op {
    TryExists,
    Metadata,
    Read,
    CreateDirAll,
    WriteAtomic,
    AtomicRename,
    RemoveFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpEvent {
    WriteAtomic(PathBuf),
    AtomicRename(PathBuf),
    RemoveFile(PathBuf),
}

#[derive(Debug, Default)]
pub(crate) struct MockMemoFs {
    inner: TokioMemoFs,
    failures: Mutex<HashMap<(Op, PathBuf), io::ErrorKind>>,
    atomic_write_observer: Mutex<Vec<(PathBuf, Vec<u8>)>>,
    operations: Mutex<Vec<OpEvent>>,
}

impl MockMemoFs {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn fail_at(&self, op: Op, path: impl Into<PathBuf>, kind: io::ErrorKind) -> &Self {
        self.failures
            .lock()
            .expect("failures mutex poisoned")
            .insert((op, path.into()), kind);
        self
    }

    pub async fn atomic_writes(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.atomic_write_observer
            .lock()
            .expect("atomic write observer mutex poisoned")
            .clone()
    }

    pub async fn operations(&self) -> Vec<OpEvent> {
        self.operations
            .lock()
            .expect("operations mutex poisoned")
            .clone()
    }

    fn lookup_failure(&self, op: Op, path: &Path) -> Option<io::ErrorKind> {
        self.failures
            .lock()
            .expect("failures mutex poisoned")
            .get(&(op, path.to_path_buf()))
            .copied()
    }
}

#[async_trait]
impl MemoFs for MockMemoFs {
    async fn try_exists(&self, path: &Path) -> io::Result<bool> {
        if let Some(kind) = self.lookup_failure(Op::TryExists, path) {
            return Err(io::Error::from(kind));
        }

        self.inner.try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        if let Some(kind) = self.lookup_failure(Op::Metadata, path) {
            return Err(io::Error::from(kind));
        }

        self.inner.metadata(path).await
    }

    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        if let Some(kind) = self.lookup_failure(Op::Read, path) {
            return Err(MemoReadError::Read(io::Error::from(kind)));
        }

        self.inner.read_with_limit(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::CreateDirAll, path) {
            return Err(io::Error::from(kind));
        }

        self.inner.create_dir_all(path).await
    }

    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck<'_>,
    ) -> Result<(), MemoWriteError> {
        if let Some(kind) = self.lookup_failure(Op::WriteAtomic, path) {
            return Err(MemoWriteError::Io(io::Error::from(kind)));
        }

        let tmp_path = path.with_extension("memo-atomic-test-tmp");
        match tokio::fs::remove_file(&tmp_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(MemoWriteError::Io(error)),
        }

        if let Err(error) = tokio::fs::write(&tmp_path, content).await {
            return Err(MemoWriteError::Io(error));
        }
        self.operations
            .lock()
            .expect("operations mutex poisoned")
            .push(OpEvent::WriteAtomic(path.to_path_buf()));
        self.atomic_write_observer
            .lock()
            .expect("atomic write observer mutex poisoned")
            .push((path.to_path_buf(), content.to_vec()));

        if let Err(error) = before_rename(path, &tmp_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(MemoWriteError::BeforeRename(error));
        }

        if let Some(kind) = self.lookup_failure(Op::AtomicRename, path) {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(MemoWriteError::Io(io::Error::from(kind)));
        }

        if let Err(error) = tokio::fs::rename(&tmp_path, path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(MemoWriteError::Io(error));
        }

        self.operations
            .lock()
            .expect("operations mutex poisoned")
            .push(OpEvent::AtomicRename(path.to_path_buf()));
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::RemoveFile, path) {
            return Err(io::Error::from(kind));
        }

        self.operations
            .lock()
            .expect("operations mutex poisoned")
            .push(OpEvent::RemoveFile(path.to_path_buf()));
        self.inner.remove_file(path).await
    }
}

/// テスト用 `AppState` を組み立てる。
/// 既存の `create_*_state` ヘルパーは tests.rs 内に残置するが、
/// 新仕様でメモ用 `MemoFs` を差し替えるテストは本ヘルパーを経由する。
pub(crate) fn make_test_app_state(mode: AppMode, memo_fs: Arc<dyn MemoFs>) -> AppState {
    let (tx, _rx) = broadcast::channel::<BroadcastMessage>(4);
    AppState::new(mode, false, None, tx).with_memo_fs(memo_fs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_fileは絶対パスを拒否する() {
        let workspace = TempWorkspace::new().expect("workspace should be created");
        let err = workspace
            .write_file(Path::new("/tmp/outside.md"), "outside")
            .expect_err("absolute path should be rejected");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn write_fileは親ディレクトリ参照を拒否する() {
        let workspace = TempWorkspace::new().expect("workspace should be created");
        let err = workspace
            .write_file(Path::new("../outside.md"), "outside")
            .expect_err("parent dir path should be rejected");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn mock_memo_fsはatomic_writeとremove_fileの操作順を記録する() {
        let workspace = TempWorkspace::new().expect("workspace should be created");
        let memo_path = workspace
            .write_md(Path::new("memo.md"), "old")
            .expect("memo should be written");
        let memo_fs = MockMemoFs::new();
        let memo_path_for_check = memo_path.clone();

        memo_fs
            .write_atomic(&memo_path, b"new", &move |final_path, tmp_path| {
                let final_path = final_path.to_path_buf();
                let tmp_path = tmp_path.to_path_buf();
                let memo_path_for_check = memo_path_for_check.clone();
                before_rename_future(async move {
                    assert_eq!(final_path.as_path(), memo_path_for_check.as_path());
                    assert_eq!(tmp_path.parent(), final_path.parent());
                    assert!(tmp_path.exists(), "tmp file should exist before rename");
                    Ok(())
                })
            })
            .await
            .expect("atomic write should succeed");
        memo_fs
            .remove_file(&memo_path)
            .await
            .expect("remove file should succeed");

        assert_eq!(
            memo_fs.atomic_writes().await,
            vec![(memo_path.clone(), b"new".to_vec())]
        );
        assert_eq!(
            memo_fs.operations().await,
            vec![
                OpEvent::WriteAtomic(memo_path.clone()),
                OpEvent::AtomicRename(memo_path.clone()),
                OpEvent::RemoveFile(memo_path),
            ]
        );
    }

    #[tokio::test]
    async fn mock_memo_fsはatomic_writeとrename失敗を注入できる() {
        let workspace = TempWorkspace::new().expect("workspace should be created");
        let write_path = workspace
            .write_md(Path::new("write.md"), "old")
            .expect("memo should be written");
        let rename_path = workspace
            .write_md(Path::new("rename.md"), "old")
            .expect("memo should be written");
        let memo_fs = MockMemoFs::new();

        memo_fs.fail_at(
            Op::WriteAtomic,
            &write_path,
            io::ErrorKind::PermissionDenied,
        );
        memo_fs.fail_at(Op::AtomicRename, &rename_path, io::ErrorKind::AlreadyExists);

        let write_err = memo_fs
            .write_atomic(&write_path, b"new", &always_ok_before_rename)
            .await
            .expect_err("write_atomic failure should be injected");
        let rename_err = memo_fs
            .write_atomic(&rename_path, b"new", &always_ok_before_rename)
            .await
            .expect_err("atomic rename failure should be injected");

        assert!(matches!(
            write_err,
            MemoWriteError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied
        ));
        assert!(matches!(
            rename_err,
            MemoWriteError::Io(error) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(
            memo_fs.atomic_writes().await,
            vec![(rename_path.clone(), b"new".to_vec())]
        );
        assert_eq!(
            memo_fs.operations().await,
            vec![OpEvent::WriteAtomic(rename_path)]
        );
    }
}
