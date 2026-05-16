#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(unix)]
use std::{fs, os::unix::fs::symlink};

use axum::http::StatusCode;

use super::support::{
    create_directory_state, create_markdown_fixture, create_single_file_state,
    make_dir_unsearchable,
};
#[cfg(unix)]
use crate::server::files::memo_fs::MemoBeforeRenameError;
use crate::server::files::memo_fs::{
    before_rename_future, BeforeRenameCheck, MemoFs, MemoReadError, MemoWriteError, TokioMemoFs,
};
use crate::server::files::memo_sidecar::SidecarMemoName;
use crate::server::files::test_support::{
    make_test_app_state, MockMemoFs, Op, OpEvent, TempWorkspace,
};
use crate::server::files::*;
use crate::server::AppMode;

#[cfg(unix)]
#[derive(Debug)]
struct SymlinkBeforeRenameMemoFs {
    inner: TokioMemoFs,
    link_target: PathBuf,
}

#[cfg(unix)]
impl SymlinkBeforeRenameMemoFs {
    fn new(link_target: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: TokioMemoFs,
            link_target,
        })
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct TmpSymlinkBeforeRenameMemoFs {
    inner: TokioMemoFs,
    link_target: PathBuf,
}

#[cfg(unix)]
impl TmpSymlinkBeforeRenameMemoFs {
    fn new(link_target: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: TokioMemoFs,
            link_target,
        })
    }
}

#[derive(Debug)]
struct MismatchedTmpParentMemoFs {
    inner: TokioMemoFs,
}

#[derive(Debug)]
struct TooLargeOnReadMemoFs {
    inner: TokioMemoFs,
    too_large_path: PathBuf,
}

impl TooLargeOnReadMemoFs {
    fn new(too_large_path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: TokioMemoFs,
            too_large_path,
        })
    }
}

impl MismatchedTmpParentMemoFs {
    fn new() -> Arc<Self> {
        Arc::new(Self { inner: TokioMemoFs })
    }
}

#[async_trait::async_trait]
impl MemoFs for MismatchedTmpParentMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        self.inner.try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        self.inner.metadata(path).await
    }

    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        self.inner.read_with_limit(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.inner.create_dir_all(path).await
    }

    async fn write_atomic(
        &self,
        path: &Path,
        _content: &[u8],
        before_rename: &BeforeRenameCheck<'_>,
    ) -> Result<(), MemoWriteError> {
        let tmp_path = path
            .parent()
            .expect("memo path should have a parent")
            .join(".other-tmp-dir")
            .join("memo.tmp");
        match before_rename(path, &tmp_path).await {
            Ok(()) => Err(MemoWriteError::Io(std::io::Error::other(
                "mismatched tmp parent should be rejected before rename",
            ))),
            Err(error) => Err(MemoWriteError::BeforeRename(error)),
        }
    }

    async fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.inner.remove_file(path).await
    }
}

#[async_trait::async_trait]
impl MemoFs for TooLargeOnReadMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        self.inner.try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        self.inner.metadata(path).await
    }

    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        if path == self.too_large_path {
            return Err(MemoReadError::TooLarge);
        }
        self.inner.read_with_limit(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.inner.create_dir_all(path).await
    }

    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck<'_>,
    ) -> Result<(), MemoWriteError> {
        self.inner.write_atomic(path, content, before_rename).await
    }

    async fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.inner.remove_file(path).await
    }
}

#[cfg(unix)]
#[async_trait::async_trait]
impl MemoFs for SymlinkBeforeRenameMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        self.inner.try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        self.inner.metadata(path).await
    }

    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        self.inner.read_with_limit(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.inner.create_dir_all(path).await
    }

    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck<'_>,
    ) -> Result<(), MemoWriteError> {
        let link_target = self.link_target.clone();
        self.inner
            .write_atomic(path, content, &move |final_path, tmp_path| {
                let final_path = final_path.to_path_buf();
                let tmp_path = tmp_path.to_path_buf();
                let link_target = link_target.clone();
                before_rename_future(async move {
                    match std::fs::remove_file(&final_path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(MemoBeforeRenameError::new(format!(
                                "テスト用メモ差し替えに失敗しました: {error}"
                            )));
                        }
                    }
                    symlink(&link_target, &final_path).map_err(|error| {
                        MemoBeforeRenameError::new(format!(
                            "テスト用メモsymlink作成に失敗しました: {error}"
                        ))
                    })?;
                    before_rename(&final_path, &tmp_path).await
                })
            })
            .await
    }

    async fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.inner.remove_file(path).await
    }
}

#[cfg(unix)]
#[async_trait::async_trait]
impl MemoFs for TmpSymlinkBeforeRenameMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        self.inner.try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        self.inner.metadata(path).await
    }

    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        self.inner.read_with_limit(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.inner.create_dir_all(path).await
    }

    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck<'_>,
    ) -> Result<(), MemoWriteError> {
        let link_target = self.link_target.clone();
        self.inner
            .write_atomic(path, content, &move |final_path, tmp_path| {
                let final_path = final_path.to_path_buf();
                let tmp_path = tmp_path.to_path_buf();
                let link_target = link_target.clone();
                before_rename_future(async move {
                    match std::fs::remove_file(&tmp_path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(MemoBeforeRenameError::new(format!(
                                "テスト用tmp差し替えに失敗しました: {error}"
                            )));
                        }
                    }
                    symlink(&link_target, &tmp_path).map_err(|error| {
                        MemoBeforeRenameError::new(format!(
                            "テスト用tmp symlink作成に失敗しました: {error}"
                        ))
                    })?;
                    before_rename(&final_path, &tmp_path).await
                })
            })
            .await
    }

    async fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.inner.remove_file(path).await
    }
}

#[tokio::test]
async fn test_load_route_memo_旧メモルートがシンボリックリンクなら空メモとして扱う() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("README.md");
    fs::write(&file_path, "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside_dir.path().join("memos")).unwrap();
    fs::write(outside_dir.path().join("memos/README.md"), "legacy memo").unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("unsafe legacy should be ignored when no sidecar exists");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_新sidecarがシンボリックリンクなら拒否する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("README.md");
    fs::write(&file_path, "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::write(outside_dir.path().join("memo.md"), "outside").unwrap();
    symlink(
        outside_dir.path().join("memo.md"),
        dir.path().join(".README.md.memo.md"),
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let result = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None)).await;

    let (status, body) = result.expect_err("unsafe primary sidecar should be rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_新メモファイルがシンボリックリンクなら拒否する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::write(outside_dir.path().join("memo.md"), "outside").unwrap();
    symlink(
        outside_dir.path().join("memo.md"),
        dir.path().join(".README.md.memo.md"),
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("symlinked memo leaf should be rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
}

#[tokio::test]
async fn test_save_route_memo_単一ファイルモードで同階層sidecarへ保存する() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("memo should save");

    assert_eq!(memo.raw(), "memo");
    assert!(file_path
        .parent()
        .unwrap()
        .join(".test.md.memo.md")
        .exists());
}

#[tokio::test]
async fn test_save_route_memo_tmp親不一致は内部状態エラーを返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, MismatchedTmpParentMemoFs::new());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("mismatched tmp parent should be rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存の内部状態が不正なため操作を中止しました"
    );
}

#[tokio::test]
async fn test_save_route_memo_10mb超過はatomic_write前に拒否する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let memo_fs = MockMemoFs::new();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs.clone());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let oversized = "a".repeat((MAX_FILE_SIZE as usize) + 1);

    let result = save_route_memo(
        &state,
        &target,
        oversized,
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("oversized memo should be rejected");
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモサイズが上限（10MB）を超えています");
    assert!(
        memo_fs.atomic_writes().await.is_empty(),
        "oversized memo should be rejected before atomic write"
    );
}

#[tokio::test]
async fn test_load_route_memo_旧パスのみ存在する場合はそのまま読み込む() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("legacy memo should load");

    assert_eq!(memo.raw(), "legacy memo");
    assert!(!dir.path().join(".README.md.memo.md").exists());
    assert!(dir.path().join(".markdown-view/memos/README.md").exists());
}

#[tokio::test]
async fn test_load_route_memo_新旧両方ある場合は新sidecarを優先する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::write(dir.path().join(".README.md.memo.md"), "new memo").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("new memo should win");

    assert_eq!(memo.raw(), "new memo");
    assert!(dir.path().join(".markdown-view/memos/README.md").exists());
}

#[tokio::test]
async fn test_save_route_memo_旧パスのみ存在する場合は新sidecarへ移行して保存する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "updated memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy memo should migrate on save");

    assert_eq!(memo.raw(), "updated memo");
    assert!(dir.path().join(".README.md.memo.md").exists());
    assert!(!dir.path().join(".markdown-view/memos/README.md").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_旧symlinkが残っていてもsidecar保存を継続できる() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("unsafe legacy should not block sidecar save");

    assert_eq!(memo.raw(), "memo");
    assert!(dir.path().join(".README.md.memo.md").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存はunsafeなlegacyがあってもsidecar削除を優先する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::write(dir.path().join(".README.md.memo.md"), "memo").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside_dir.path().join("memos")).unwrap();
    fs::write(outside_dir.path().join("memos/README.md"), "legacy memo").unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("unsafe legacy should not block sidecar delete");

    assert_eq!(memo.raw(), "");
    assert!(!dir.path().join(".README.md.memo.md").exists());
    assert!(dir.path().join(".markdown-view").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存_required_legacy安全確認io失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    workspace
        .write_file(Path::new("README.md"), "# README")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".README.md.memo.md"), "memo")
        .expect("sidecar memo should be written");
    let legacy_path = workspace.path().join(".markdown-view/memos/README.md");
    workspace
        .write_file(Path::new(".markdown-view/memos/README.md"), "legacy memo")
        .expect("legacy memo should be written");
    let locked_dir = workspace.path().join(".markdown-view/memos");

    let state = create_directory_state(workspace.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let Some(permission_guard) = make_dir_unsearchable(&locked_dir, &legacy_path) else {
        return;
    };

    let result = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) =
        result.expect_err("required legacy safety inspection failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモ保存先の安全確認に失敗しました");
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "memo");
    drop(permission_guard);
    assert!(legacy_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存_safe_legacy削除失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    workspace
        .write_file(Path::new("README.md"), "# README")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".README.md.memo.md"), "memo")
        .expect("sidecar memo should be written");
    let legacy_path = workspace.path().join(".markdown-view/memos/README.md");
    workspace
        .write_file(Path::new(".markdown-view/memos/README.md"), "legacy memo")
        .expect("legacy memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &legacy_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_directory(workspace.path()).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("safe legacy cleanup failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "memo");
    assert!(legacy_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存_safe_compat削除失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("a\\b.md"), "# separator shaped")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".a_b.md.memo.md"), "memo")
        .expect("sidecar memo should be written");
    let compat_sidecar_path = workspace
        .write_file(Path::new(".a\\b.md.memo.md"), "compat memo")
        .expect("compat sidecar memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &compat_sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("safe compat cleanup failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "memo");
    assert!(compat_sidecar_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_保存成功後のlegacy削除失敗は成功扱いにする() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    workspace
        .write_file(Path::new("README.md"), "# README")
        .expect("target markdown should be written");
    let legacy_path = workspace.path().join(".markdown-view/memos/README.md");
    workspace
        .write_file(Path::new(".markdown-view/memos/README.md"), "legacy memo")
        .expect("legacy memo should be written");
    let sidecar_path = workspace.path().join(".README.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &legacy_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_directory(workspace.path()).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let saved = save_route_memo(
        &state,
        &target,
        "updated again".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy cleanup failure should be non-fatal");

    assert_eq!(saved.raw(), "updated again");
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "updated again");
    assert!(legacy_path.exists());
}

#[tokio::test]
async fn test_save_route_memo_拡張子の大文字小文字が異なるファイルでもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("guide.md"), "# lower").unwrap();
    fs::write(dir.path().join("guide.MD"), "# upper").unwrap();
    let state = create_directory_state(dir.path());

    let lower_target = resolve_route_target(&state, RouteTargetRequest::api_memo(Some("guide.md")))
        .await
        .unwrap();
    let upper_target = resolve_route_target(&state, RouteTargetRequest::api_memo(Some("guide.MD")))
        .await
        .unwrap();

    let lower = save_route_memo(
        &state,
        &lower_target,
        "lower memo".to_string(),
        RouteTargetRequest::api_memo(Some("guide.md")),
    )
    .await
    .expect("lower memo should save");
    let upper = save_route_memo(
        &state,
        &upper_target,
        "upper memo".to_string(),
        RouteTargetRequest::api_memo(Some("guide.MD")),
    )
    .await
    .expect("upper memo should save");

    assert_eq!(lower.raw(), "lower memo");
    assert_eq!(upper.raw(), "upper memo");
    assert!(dir.path().join(".guide.md.memo.md").exists());
    assert!(dir.path().join(".guide.MD.memo.md").exists());
}

#[tokio::test]
async fn test_save_route_memo_長いファイル名でも短縮sidecarへ保存できる() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("long filename should save to shortened sidecar");

    assert_eq!(memo.raw(), "memo");
    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 1);
    assert!(memo_entries[0].len() <= 255);
}

#[tokio::test]
async fn test_load_route_memo_長いファイル名で未作成なら空メモを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("long filename should not break empty memo read");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[tokio::test]
async fn test_save_route_memo_長いファイル名のlegacyメモは空白保存で削除できる() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    let legacy_path = dir.path().join(".markdown-view/memos").join(&file_name);
    fs::write(&legacy_path, "memo").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        " \n ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy fallback memo should be deletable");

    assert_eq!(memo.raw(), "");
    assert!(!legacy_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_非utf8ファイル名でもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    let lower_name = std::ffi::OsStr::from_bytes(b"guide-\xff.md");
    let upper_name = std::ffi::OsStr::from_bytes(b"guide-\xfe.md");
    let lower_path = dir.path().join(lower_name);
    let upper_path = dir.path().join(upper_name);
    fs::write(&lower_path, "# lower").unwrap();
    fs::write(&upper_path, "# upper").unwrap();

    let lower_state = create_single_file_state(&lower_path);
    let upper_state = create_single_file_state(&upper_path);
    let lower_target = resolve_route_target(&lower_state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let upper_target = resolve_route_target(&upper_state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let lower = save_route_memo(
        &lower_state,
        &lower_target,
        "lower memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("lower memo should save");
    let upper = save_route_memo(
        &upper_state,
        &upper_target,
        "upper memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("upper memo should save");

    assert_eq!(lower.raw(), "lower memo");
    assert_eq!(upper.raw(), "upper memo");

    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 2);
    assert_ne!(memo_entries[0], memo_entries[1]);
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_正規化される短いファイル名でもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    let plain_path = dir.path().join("a_b.md");
    let separator_shaped_path = dir.path().join("a\\b.md");
    fs::write(&plain_path, "# plain").unwrap();
    fs::write(&separator_shaped_path, "# separator shaped").unwrap();

    let plain_state = create_single_file_state(&plain_path);
    let separator_shaped_state = create_single_file_state(&separator_shaped_path);
    let plain_target = resolve_route_target(&plain_state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let separator_shaped_target =
        resolve_route_target(&separator_shaped_state, RouteTargetRequest::api_memo(None))
            .await
            .unwrap();

    save_route_memo(
        &plain_state,
        &plain_target,
        "plain memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("plain memo should save");
    save_route_memo(
        &separator_shaped_state,
        &separator_shaped_target,
        "separator shaped memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("separator shaped memo should save");

    let plain = load_route_memo(
        &plain_state,
        &plain_target,
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("plain memo should load");
    let separator_shaped = load_route_memo(
        &separator_shaped_state,
        &separator_shaped_target,
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("separator shaped memo should load");

    assert_eq!(plain.raw(), "plain memo");
    assert_eq!(separator_shaped.raw(), "separator shaped memo");

    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 2);
    assert!(memo_entries.contains(&".a_b.md.memo.md".to_string()));
    assert_ne!(memo_entries[0], memo_entries[1]);
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_旧形式backslash_sidecarを読み込む() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(dir.path().join(".a\\b.md.memo.md"), "compat memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("compat sidecar memo should load");

    assert_eq!(memo.raw(), "compat memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_旧形式backslash_sidecarを新形式へ移行する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let old_sidecar = dir.path().join(".a\\b.md.memo.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&old_sidecar, "compat memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("compat sidecar should migrate on save");

    assert_eq!(memo.raw(), "new memo");
    assert_eq!(fs::read_to_string(&new_sidecar).unwrap(), "new memo");
    assert!(!old_sidecar.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_新旧backslash_sidecar両方ある場合は新形式を優先する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let old_sidecar = dir.path().join(".a\\b.md.memo.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&old_sidecar, "compat memo").unwrap();
    fs::write(&new_sidecar, "new memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("new sidecar memo should win");

    assert_eq!(memo.raw(), "new memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_sidecar優先_compat_legacy両方存在しても新sidecarを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    let compat_sidecar = dir.path().join(".a\\b.md.memo.md");
    let legacy_path = dir.path().join(".markdown-view/memos/a\\b.md");
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&new_sidecar, "new memo").unwrap();
    fs::write(&compat_sidecar, "compat memo").unwrap();
    fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    fs::write(&legacy_path, "legacy memo").unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(Some("a\\b.md")))
        .await
        .unwrap();

    let memo = load_route_memo(
        &state,
        &target,
        RouteTargetRequest::api_memo(Some("a\\b.md")),
    )
    .await
    .expect("new sidecar memo should win over compat and legacy");

    assert_eq!(memo.raw(), "new memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_compat優先_legacy存在でも新compatを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let compat_sidecar = dir.path().join(".a\\b.md.memo.md");
    let legacy_path = dir.path().join(".markdown-view/memos/a\\b.md");
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&compat_sidecar, "compat memo").unwrap();
    fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    fs::write(&legacy_path, "legacy memo").unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(Some("a\\b.md")))
        .await
        .unwrap();

    let memo = load_route_memo(
        &state,
        &target,
        RouteTargetRequest::api_memo(Some("a\\b.md")),
    )
    .await
    .expect("compat sidecar memo should win over legacy");

    assert_eq!(memo.raw(), "compat memo");
}

#[tokio::test]
async fn test_load_route_memo_sidecarがmetadata前に消えたらlegacyへフォールバックする() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "sidecar memo")
        .expect("sidecar memo should be written");
    workspace
        .write_file(Path::new(".markdown-view/memos/note.md"), "legacy memo")
        .expect("legacy memo should be written");
    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(Op::Metadata, &sidecar_path, std::io::ErrorKind::NotFound);
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("disappeared sidecar should fall back to legacy memo");

    assert_eq!(memo.raw(), "legacy memo");
}

#[tokio::test]
async fn test_load_route_memo_sidecarがread中に消えたらlegacyへフォールバックする() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "sidecar memo")
        .expect("sidecar memo should be written");
    workspace
        .write_file(Path::new(".markdown-view/memos/note.md"), "legacy memo")
        .expect("legacy memo should be written");
    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(Op::Read, &sidecar_path, std::io::ErrorKind::NotFound);
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("disappeared sidecar should fall back to legacy memo");

    assert_eq!(memo.raw(), "legacy memo");
}

#[tokio::test]
async fn test_load_route_memo_非utf8メモは422を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    fs::write(
        workspace.path().join(".note.md.memo.md"),
        [0xff, 0xfe, 0xfd],
    )
    .expect("non-utf8 memo sidecar should be written");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None)).await;

    let (status, body) = result.expect_err("non-utf8 memo should be rejected");
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモはUTF-8テキストである必要があります");
}

#[tokio::test]
async fn test_load_route_memo_read_with_limit_too_largeは413を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "small memo")
        .expect("sidecar memo should be written");
    let memo_fs = TooLargeOnReadMemoFs::new(sidecar_path);
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None)).await;

    let (status, body) = result.expect_err("read-time size overflow should be rejected");
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモサイズが上限（10MB）を超えています");
}

#[tokio::test]
async fn test_load_route_memo_全て不在なら空メモ() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("missing memo files should load as empty memo");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_単一ファイルモードでpermission_deniedなら500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::WriteAtomic,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("single file mode should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[tokio::test]
async fn test_save_route_memo_sidecar書込不可で500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::WriteAtomic,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar write failure should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[tokio::test]
async fn test_save_route_memo_atomic_rename失敗時は既存メモを保持する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    std::fs::write(&sidecar_path, "old memo").expect("existing sidecar should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::AtomicRename,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("atomic rename failure should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert_eq!(
        std::fs::read_to_string(&sidecar_path).expect("existing sidecar should remain readable"),
        "old memo"
    );
}

#[tokio::test]
async fn test_save_route_memo_atomic_write失敗時は既存メモを保持する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    std::fs::write(&sidecar_path, "old memo").expect("existing sidecar should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::WriteAtomic,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("atomic write failure should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert_eq!(
        std::fs::read_to_string(&sidecar_path).expect("existing sidecar should remain readable"),
        "old memo"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_rename直前にsidecarがsymlinkへ差し替わると403を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    std::fs::write(&sidecar_path, "old memo").expect("existing sidecar should be written");
    let outside_dir = tempfile::tempdir().expect("outside dir should be created");
    let outside_memo = outside_dir.path().join("outside.md");
    std::fs::write(&outside_memo, "outside memo").expect("outside memo should be written");

    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, SymlinkBeforeRenameMemoFs::new(outside_memo.clone()));
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("rename precheck should reject symlink replacement");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
    assert!(
        std::fs::symlink_metadata(&sidecar_path)
            .expect("replaced sidecar should still exist")
            .file_type()
            .is_symlink(),
        "test hook should replace the sidecar immediately before rename"
    );
    assert_eq!(
        std::fs::read_to_string(&outside_memo).expect("outside memo should remain readable"),
        "outside memo"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_rename直前にtmpがsymlinkへ差し替わると403を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    std::fs::write(&sidecar_path, "old memo").expect("existing sidecar should be written");
    let outside_dir = tempfile::tempdir().expect("outside dir should be created");
    let outside_tmp = outside_dir.path().join("outside.tmp");
    std::fs::write(&outside_tmp, "outside tmp").expect("outside tmp should be written");

    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, TmpSymlinkBeforeRenameMemoFs::new(outside_tmp.clone()));
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("tmp precheck should reject symlink replacement");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
    assert_eq!(
        std::fs::read_to_string(&sidecar_path).expect("existing sidecar should remain readable"),
        "old memo"
    );
    assert_eq!(
        std::fs::read_to_string(&outside_tmp).expect("outside tmp should remain readable"),
        "outside tmp"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_安全確認io失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    let Some(_guard) = make_dir_unsearchable(workspace.path(), &sidecar_path) else {
        return;
    };

    let result = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("inspection failure should be surfaced as 500");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモ保存先の安全確認に失敗しました");
}

#[tokio::test]
async fn test_save_route_memo_create_dir_all失敗で500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_parent = workspace.path().to_path_buf();

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::CreateDirAll,
        &sidecar_parent,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar parent creation failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[tokio::test]
#[allow(non_snake_case)]
async fn test_save_route_memo_disk_full系IO失敗で500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(Op::WriteAtomic, &sidecar_path, std::io::ErrorKind::Other);
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar io failure should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_保存成功後のcompat削除失敗は200を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("a\\b.md"), "# separator shaped")
        .expect("target markdown should be written");
    let compat_sidecar_path = workspace
        .write_file(Path::new(".a\\b.md.memo.md"), "compat memo")
        .expect("compat sidecar memo should be written");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar_path = workspace.path().join(new_sidecar_name.as_str());

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &compat_sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs.clone());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let saved = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("compat cleanup failure should be non-fatal");

    assert_eq!(saved.raw(), "new memo");
    assert_eq!(
        memo_fs.atomic_writes().await,
        vec![(new_sidecar_path.clone(), b"new memo".to_vec())]
    );
    assert_eq!(fs::read_to_string(&new_sidecar_path).unwrap(), "new memo");
    assert!(compat_sidecar_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_compatと新sidecarが同一パスならcleanupで削除しない() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_name = format!("{}\\tail-tail-tail.md", "a".repeat(229));
    let file_path = workspace
        .write_md(Path::new(&file_name), "# separator shaped")
        .expect("target markdown should be written");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let compat_sidecar_name =
        SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new(&file_name))
            .expect("backslash name should have compat sidecar");
    assert_eq!(new_sidecar_name.as_str(), compat_sidecar_name.as_str());
    let new_sidecar_path = workspace.path().join(new_sidecar_name.as_str());

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let saved = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("same compat sidecar path should not delete active sidecar");

    assert_eq!(saved.raw(), "new memo");
    assert_eq!(fs::read_to_string(&new_sidecar_path).unwrap(), "new memo");
}

#[tokio::test]
async fn test_save_route_memo_空保存_sidecarが既にない場合は冪等的に200を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        " \n\t ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("blank save without sidecar should be idempotent");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
    assert!(!sidecar_path.exists());
}

#[tokio::test]
async fn test_save_route_memo_空保存はprimary_sidecarを最後に削除する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "sidecar")
        .expect("sidecar memo should be written");
    let legacy_path = workspace
        .write_file(Path::new(".markdown-view/memos/note.md"), "legacy")
        .expect("legacy memo should be written");

    let memo_fs = MockMemoFs::new();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs.clone());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        " ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("blank save should delete all memo files");

    assert_eq!(memo.raw(), "");
    assert_eq!(
        memo_fs.operations().await,
        vec![
            OpEvent::RemoveFile(legacy_path),
            OpEvent::RemoveFile(sidecar_path),
        ]
    );
}

#[tokio::test]
async fn test_save_route_memo_空保存_sidecar削除失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "memo")
        .expect("sidecar memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    let result = save_route_memo(
        &state,
        &target,
        " \n\t ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar delete failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert!(
        sidecar_path.exists(),
        "primary sidecar should remain when its deletion fails"
    );
}
