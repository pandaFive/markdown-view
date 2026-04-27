//! memo 経路テストの共通基盤。
//!
//! [`TempWorkspace`] は tempdir ベースのワークスペースを提供する。

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio::sync::Mutex as AsyncMutex;

use crate::server::messages::BroadcastMessage;
use crate::server::state::{AppMode, AppState};

use super::memo_fs::{MemoFs, MemoReadError, TokioMemoFs};

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
    Write,
    RemoveFile,
}

#[derive(Debug, Default)]
pub(crate) struct MockMemoFs {
    inner: TokioMemoFs,
    failures: Mutex<HashMap<(Op, PathBuf), io::ErrorKind>>,
    write_observer: AsyncMutex<Vec<(PathBuf, Vec<u8>)>>,
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

    #[allow(dead_code)]
    pub fn clear_failures(&self) -> &Self {
        self.failures
            .lock()
            .expect("failures mutex poisoned")
            .clear();
        self
    }

    pub async fn writes(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.write_observer.lock().await.clone()
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

    async fn write(&self, path: &Path, content: &[u8]) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::Write, path) {
            return Err(io::Error::from(kind));
        }

        self.write_observer
            .lock()
            .await
            .push((path.to_path_buf(), content.to_vec()));
        self.inner.write(path, content).await
    }

    async fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::RemoveFile, path) {
            return Err(io::Error::from(kind));
        }

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
}
