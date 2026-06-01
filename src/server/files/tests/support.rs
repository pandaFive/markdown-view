use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};

use crate::server::{AppMode, AppState};
use tokio::sync::broadcast;

pub(super) fn create_test_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# README").unwrap();
    std::fs::write(dir.path().join("guide.md"), "# Guide").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "text file").unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/api.md"), "# API").unwrap();
    std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
    std::fs::write(dir.path().join(".hidden/secret.md"), "# Secret").unwrap();
    std::fs::write(dir.path().join(".dotfile.md"), "# Dot").unwrap();
    dir
}

pub(super) fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join(name);
    std::fs::write(&file_path, content).unwrap();
    (dir, file_path)
}

pub(super) fn hard_link_or_skip(source: &Path, linked: &Path) -> bool {
    assert!(
        source.is_file(),
        "hardlink元は通常ファイルである必要があります"
    );
    assert!(
        !linked.exists(),
        "hardlink先は事前に存在しない必要があります"
    );
    match std::fs::hard_link(source, linked) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {
            eprintln!(
                "hardlink非対応環境のためテストをスキップします: kind={:?}",
                error.kind()
            );
            false
        }
        Err(error) => panic!("hardlink作成に失敗しました: kind={:?}", error.kind()),
    }
}

#[cfg(unix)]
pub(super) struct PermissionGuard {
    path: PathBuf,
    original_mode: u32,
}

#[cfg(unix)]
impl Drop for PermissionGuard {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.original_mode));
    }
}

#[cfg(unix)]
pub(super) fn make_dir_unsearchable(dir: &Path, probe: &Path) -> Option<PermissionGuard> {
    let original_mode = fs::metadata(dir).unwrap().permissions().mode();
    let guard = PermissionGuard {
        path: dir.to_path_buf(),
        original_mode,
    };
    fs::set_permissions(dir, fs::Permissions::from_mode(0o000)).unwrap();

    if probe.canonicalize().is_ok() {
        eprintln!(
            "chmod 0o000 後も対象パスを正規化できるため、canonicalize I/Oエラーテストをskipします"
        );
        drop(guard);
        None
    } else {
        Some(guard)
    }
}

pub(super) fn create_single_file_state(file_path: &std::path::Path) -> AppState {
    let (tx, _rx) = broadcast::channel(4);
    AppState::new_with_tokio_memo_fs(
        AppMode::new_single_file(file_path).unwrap(),
        false,
        None,
        tx,
    )
}

pub(super) fn create_directory_state(dir_path: &std::path::Path) -> AppState {
    let (tx, _rx) = broadcast::channel(4);
    AppState::new_with_tokio_memo_fs(AppMode::new_directory(dir_path).unwrap(), false, None, tx)
}
