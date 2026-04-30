//! メモ保存・読み込みで使用するファイルシステム抽象。
//!
//! 本番では [`TokioMemoFs`] が `tokio::fs::*` を呼び出す薄いラッパーとして動作する。
//! テストでは `MockMemoFs`（`test_support` モジュール）を注入し、
//! 特定パスの I/O エラーを決定論的に再現する。

use std::fs::Metadata;
use std::io;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use super::content::{read_bytes_with_limit, ReadMarkdownError};

#[derive(Debug)]
pub(crate) enum MemoReadError {
    Open(std::io::Error),
    Read(std::io::Error),
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoBeforeRenameError {
    user_message: String,
}

impl MemoBeforeRenameError {
    pub(crate) fn new(user_message: impl Into<String>) -> Self {
        Self {
            user_message: user_message.into(),
        }
    }

    pub(crate) fn user_message(&self) -> &str {
        &self.user_message
    }
}

#[derive(Debug)]
pub(crate) enum MemoWriteError {
    Io(io::Error),
    BeforeRename(MemoBeforeRenameError),
}

impl From<io::Error> for MemoWriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<MemoBeforeRenameError> for MemoWriteError {
    fn from(error: MemoBeforeRenameError) -> Self {
        Self::BeforeRename(error)
    }
}

pub(crate) type BeforeRenameCheck<'a> =
    dyn Fn(&Path, &Path) -> Result<(), MemoBeforeRenameError> + Send + Sync + 'a;

static ATOMIC_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const ATOMIC_TMP_ATTEMPTS: u8 = 8;

/// メモ保存先ファイルシステムの抽象。
///
/// サイズ上限の事前チェック等のドメイン責務は呼び出し側で行い、
/// 実読み取り量の上限は `read_with_limit` 側でも保証する。
/// `NotFound` 等の特殊エラー処理も呼び出し側で吸収する。
#[async_trait]
pub(crate) trait MemoFs: Send + Sync + std::fmt::Debug {
    /// パス存在確認。シンボリックリンク要素は呼び出し側で別途検査済み想定。
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool>;

    /// メタデータ取得（サイズ制限の一段目チェック用）
    async fn metadata(&self, path: &Path) -> std::io::Result<Metadata>;

    /// バイト列読み込み。TOCTOU 対策として実読み取り量を上限以下に制限する。
    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError>;

    /// 親ディレクトリを再帰的に作成（既存ならエラーを返さない）
    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;

    /// バイト列を同一ディレクトリ内 tmp へ書き込み、rename で最終パスへ差し替える。
    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck<'_>,
    ) -> Result<(), MemoWriteError>;

    /// ファイル削除。`NotFound` を含むエラーは透過する（呼び出し側で吸収）。
    async fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

/// 本番用 [`MemoFs`] 実装。`tokio::fs::*` を直接呼び出す。
#[derive(Debug, Default)]
pub(crate) struct TokioMemoFs;

fn atomic_tmp_path(path: &Path, attempt: u8) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "memo path must have a parent directory",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "memo path must have a file name",
        )
    })?;

    let counter = ATOMIC_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = format!(".tmp.{}.{}.{}", std::process::id(), counter, attempt);
    let base = file_name.to_string_lossy();
    let mut tmp_name = format!("{base}{suffix}");

    if tmp_name.len() > 255 {
        let mut hasher = Sha256::new();
        hasher.update(file_name.as_encoded_bytes());
        hasher.update(counter.to_le_bytes());
        hasher.update([attempt]);
        let hash = format!("{:x}", hasher.finalize());
        tmp_name = format!(".memo.{}.tmp", &hash[..32]);
    }

    Ok(parent.join(tmp_name))
}

async fn sync_parent_dir_best_effort(parent: &Path) {
    let file = match tokio::fs::OpenOptions::new().read(true).open(parent).await {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] メモ保存後の親ディレクトリopenに失敗しました: {}",
                error
            );
            return;
        }
    };

    if let Err(error) = file.sync_all().await {
        tracing::warn!(
            "[markdown-view] メモ保存後の親ディレクトリsyncに失敗しました: {}",
            error
        );
    }
}

async fn cleanup_tmp_best_effort(tmp_path: &Path) {
    match tokio::fs::remove_file(tmp_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            let tmp_name = tmp_path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| "<unknown>".into());
            tracing::warn!(
                "[markdown-view] メモ一時ファイルcleanup失敗を無視します ({}): {}",
                tmp_name,
                error
            );
        }
    }
}

#[cfg(not(windows))]
async fn atomic_replace(tmp_path: &Path, path: &Path) -> io::Result<()> {
    tokio::fs::rename(tmp_path, path).await
}

#[cfg(windows)]
async fn atomic_replace(tmp_path: &Path, path: &Path) -> io::Result<()> {
    let tmp_path = path_to_wide_null(tmp_path)?;
    let path = path_to_wide_null(path)?;

    tokio::task::spawn_blocking(move || {
        // SAFETY: 両パスはNUL終端済みで、interior NULを拒否したバッファとしてこの呼び出し中は生存する。
        let result = unsafe {
            MoveFileExW(
                tmp_path.as_ptr(),
                path.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
    .await
    .map_err(|error| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("memo atomic replace task failed: {error}"),
        )
    })?
}

#[cfg(windows)]
fn path_to_wide_null(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide = Vec::new();
    for unit in path.as_os_str().encode_wide() {
        if unit == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains an interior nul byte",
            ));
        }
        wide.push(unit);
    }
    wide.push(0);
    Ok(wide)
}

#[async_trait]
impl MemoFs for TokioMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        tokio::fs::try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> std::io::Result<Metadata> {
        tokio::fs::metadata(path).await
    }

    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(MemoReadError::Open)?;
        read_bytes_with_limit(file)
            .await
            .map_err(|error| match error {
                ReadMarkdownError::Io(error) => MemoReadError::Read(error),
                ReadMarkdownError::TooLarge => MemoReadError::TooLarge,
                ReadMarkdownError::NotUtf8 => {
                    unreachable!("read_bytes_with_limit does not validate UTF-8")
                }
            })
    }

    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::create_dir_all(path).await
    }

    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck<'_>,
    ) -> Result<(), MemoWriteError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "memo path must have a parent directory",
            )
        })?;

        let mut last_already_exists = None;
        for attempt in 0..ATOMIC_TMP_ATTEMPTS {
            let tmp_path = atomic_tmp_path(path, attempt)?;
            let mut tmp_file = match tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)
                .await
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    last_already_exists = Some(error);
                    continue;
                }
                Err(error) => return Err(MemoWriteError::Io(error)),
            };

            if let Err(error) = tmp_file.write_all(content).await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }
            if let Err(error) = tmp_file.flush().await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }
            if let Err(error) = tmp_file.sync_data().await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }
            drop(tmp_file);

            if let Err(error) = before_rename(path, &tmp_path) {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::BeforeRename(error));
            }

            if let Err(error) = atomic_replace(&tmp_path, path).await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }

            sync_parent_dir_best_effort(parent).await;
            return Ok(());
        }

        Err(MemoWriteError::Io(last_already_exists.unwrap_or_else(
            || {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "memo temporary file already exists",
                )
            },
        )))
    }

    async fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::remove_file(path).await
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::ffi::OsString;
    #[cfg(windows)]
    use std::os::windows::ffi::OsStringExt;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn write_atomicはtmpを書いてからrenameする() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        tokio::fs::write(&path, b"old")
            .await
            .expect("initial memo should be written");
        let observed_tmp = Arc::new(Mutex::new(None));
        let observed_tmp_for_check = Arc::clone(&observed_tmp);
        let path_for_check = path.clone();

        TokioMemoFs
            .write_atomic(&path, b"new", &move |final_path, tmp_path| {
                assert_eq!(final_path, path_for_check.as_path());
                assert!(tmp_path.exists(), "tmp file should exist before rename");
                assert_eq!(
                    std::fs::read(final_path).expect("final path should still be readable"),
                    b"old"
                );
                *observed_tmp_for_check
                    .lock()
                    .expect("observed tmp mutex should not be poisoned") =
                    Some(tmp_path.to_path_buf());
                Ok(())
            })
            .await
            .expect("atomic write should succeed");

        let tmp_path = observed_tmp
            .lock()
            .expect("observed tmp mutex should not be poisoned")
            .clone()
            .expect("tmp path should be observed");
        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"new"
        );
        assert_eq!(tmp_path.parent(), path.parent());
        assert!(
            !tmp_path.exists(),
            "tmp path should disappear after successful rename"
        );
    }

    #[tokio::test]
    async fn write_atomicはbefore_rename失敗時にtmpを削除して元内容を残す() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        tokio::fs::write(&path, b"old")
            .await
            .expect("initial memo should be written");
        let observed_tmp = Arc::new(Mutex::new(None));
        let observed_tmp_for_check = Arc::clone(&observed_tmp);

        let err = TokioMemoFs
            .write_atomic(&path, b"new", &move |_, tmp_path| {
                assert!(tmp_path.exists(), "tmp file should exist before check");
                *observed_tmp_for_check
                    .lock()
                    .expect("observed tmp mutex should not be poisoned") =
                    Some(tmp_path.to_path_buf());
                Err(MemoBeforeRenameError::new("conflict"))
            })
            .await
            .expect_err("before_rename error should be returned");

        match err {
            MemoWriteError::BeforeRename(error) => {
                assert_eq!(error.user_message(), "conflict");
            }
            MemoWriteError::Io(error) => panic!("unexpected io error: {error}"),
        }

        let tmp_path = observed_tmp
            .lock()
            .expect("observed tmp mutex should not be poisoned")
            .clone()
            .expect("tmp path should be observed");
        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"old"
        );
        assert!(
            !tmp_path.exists(),
            "tmp path should be cleaned after before_rename failure"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn atomic_replaceはwindowsで既存ファイルを置換する() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        let tmp_path = workspace.path().join("memo.md.tmp");
        tokio::fs::write(&path, b"old")
            .await
            .expect("existing memo should be written");
        tokio::fs::write(&tmp_path, b"new")
            .await
            .expect("tmp memo should be written");

        atomic_replace(&tmp_path, &path)
            .await
            .expect("atomic replace should overwrite existing file on Windows");

        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"new"
        );
        assert!(
            !tmp_path.exists(),
            "tmp path should disappear after successful replace"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn atomic_replaceはwindowsで新規ファイルへ移動できる() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        let tmp_path = workspace.path().join("memo.md.tmp");
        tokio::fs::write(&tmp_path, b"new")
            .await
            .expect("tmp memo should be written");

        atomic_replace(&tmp_path, &path)
            .await
            .expect("atomic replace should create final file on Windows");

        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"new"
        );
        assert!(
            !tmp_path.exists(),
            "tmp path should disappear after successful replace"
        );
    }

    #[cfg(windows)]
    #[test]
    fn path_to_wide_nullはinterior_nulを拒否する() {
        let path = PathBuf::from(OsString::from_wide(&[b'a' as u16, 0, b'b' as u16]));
        let error = path_to_wide_null(&path).expect_err("interior nul should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
