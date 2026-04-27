//! memo 経路テストの共通基盤。
//!
//! [`TempWorkspace`] は tempdir + 権限戻しガードを提供する。

#![cfg(test)]

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// tempdir + 権限戻しガード付きワークスペース。
///
/// `Drop` で記録された権限を逆順に戻してから tempdir を削除する。
/// `MockMemoFs` 経由のエラー注入を主な手段とするため、本来 `chmod` は使わないが、
/// 万一テスト本体が権限を変更しても tempdir 削除がブロックされないよう保険として保持する。
pub(crate) struct TempWorkspace {
    dir: tempfile::TempDir,
    permission_resets: Mutex<Vec<(PathBuf, std::fs::Permissions)>>,
}

impl TempWorkspace {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            dir: tempfile::tempdir()?,
            permission_resets: Mutex::new(Vec::new()),
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

    /// `Drop` で復元する権限を記録する（chmod を使う既存テスト互換用、将来的には未使用化を期待）
    #[cfg(unix)]
    #[allow(dead_code)]
    pub fn record_permissions(&self, path: &Path) -> io::Result<()> {
        let perms = std::fs::metadata(path)?.permissions();
        self.permission_resets
            .lock()
            .expect("permission_resets mutex poisoned")
            .push((path.to_path_buf(), perms));
        Ok(())
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        if let Ok(mut resets) = self.permission_resets.lock() {
            while let Some((path, perms)) = resets.pop() {
                let _ = std::fs::set_permissions(&path, perms);
            }
        }
    }
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
