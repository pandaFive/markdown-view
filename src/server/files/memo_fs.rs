//! メモ保存・読み込みで使用するファイルシステム抽象。
//!
//! 本番では [`TokioMemoFs`] が通常の非同期ファイル操作に加え、
//! OS 固有 API を含む atomic replace と親ディレクトリ sync を担当する。
//! テストでは `MockMemoFs`（`test_support` モジュール）を注入し、
//! 特定パスの I/O エラーを決定論的に再現する。

use std::future::Future;
use std::io::{self, Write};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use axum::http::StatusCode;
#[cfg(any(unix, windows))]
use cap_std::fs::OpenOptionsExt as CapOpenOptionsExt;
use sha2::{Digest, Sha256};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FileRenameInfo, SetFileInformationByHandle, DELETE, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_GENERIC_WRITE, FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::catalog::open_relative_file_nofollow;
use super::content::{read_bytes_with_limit, ReadMarkdownError};

#[derive(Debug)]
pub(crate) enum MemoReadError {
    Open(std::io::Error),
    Read(std::io::Error),
    BeforeAccess(MemoBeforeRenameError),
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MemoBeforeRenameError {
    Missing,
    Forbidden(String),
    Internal(String),
}

impl MemoBeforeRenameError {
    pub(crate) fn new(user_message: impl Into<String>) -> Self {
        Self::Forbidden(user_message.into())
    }

    pub(crate) fn internal(user_message: impl Into<String>) -> Self {
        Self::Internal(user_message.into())
    }

    pub(crate) fn missing() -> Self {
        Self::Missing
    }

    pub(crate) fn user_message(&self) -> &str {
        match self {
            Self::Missing => "メモファイルが存在しません",
            Self::Forbidden(message) | Self::Internal(message) => message,
        }
    }

    pub(crate) fn status_code(&self) -> StatusCode {
        match self {
            Self::Missing => StatusCode::NOT_FOUND,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug)]
pub(crate) enum MemoWriteError {
    Io(io::Error),
    BeforeRename(MemoBeforeRenameError),
}

impl From<MemoBeforeRenameError> for MemoWriteError {
    fn from(error: MemoBeforeRenameError) -> Self {
        Self::BeforeRename(error)
    }
}

#[cfg(any(test, windows))]
/// tmp を最終パスへ置換する直前の非同期検査フック。
pub(crate) type BeforeRenameFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), MemoBeforeRenameError>> + Send + 'a>>;
#[cfg(any(test, windows))]
pub(crate) type BeforeRenameCheck<'a> =
    dyn Fn(&Path, &Path) -> BeforeRenameFuture<'a> + Send + Sync + 'a;
pub(crate) type BeforeAccessFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CheckedMemoPath, MemoBeforeRenameError>> + Send + 'a>>;
pub(crate) type BeforeAccessCheck<'a> = dyn Fn(&Path) -> BeforeAccessFuture<'a> + Send + Sync + 'a;
pub(crate) type BeforeRemoveFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CheckedMemoPath, MemoBeforeRenameError>> + Send + 'a>>;
pub(crate) type BeforeRemoveCheck<'a> = dyn Fn(&Path) -> BeforeRemoveFuture<'a> + Send + Sync + 'a;

#[derive(Debug)]
pub(crate) struct CheckedMemoPath {
    parent_dir: cap_std::fs::Dir,
    file_name: PathBuf,
}

impl CheckedMemoPath {
    pub(crate) fn new(parent_dir: cap_std::fs::Dir, file_name: PathBuf) -> Self {
        Self {
            parent_dir,
            file_name,
        }
    }

    pub(crate) fn parent_dir(&self) -> &cap_std::fs::Dir {
        &self.parent_dir
    }

    pub(crate) fn file_name(&self) -> &Path {
        &self.file_name
    }
}

#[cfg(any(test, windows))]
pub(crate) fn before_rename_future<'a>(
    future: impl Future<Output = Result<(), MemoBeforeRenameError>> + Send + 'a,
) -> BeforeRenameFuture<'a> {
    Box::pin(future)
}

pub(crate) fn before_access_future<'a>(
    future: impl Future<Output = Result<CheckedMemoPath, MemoBeforeRenameError>> + Send + 'a,
) -> BeforeAccessFuture<'a> {
    Box::pin(future)
}

pub(crate) fn before_remove_future<'a>(
    future: impl Future<Output = Result<CheckedMemoPath, MemoBeforeRenameError>> + Send + 'a,
) -> BeforeRemoveFuture<'a> {
    Box::pin(future)
}

#[cfg(any(test, windows))]
pub(crate) fn always_ok_before_rename(_: &Path, _: &Path) -> BeforeRenameFuture<'static> {
    before_rename_future(async { Ok(()) })
}

#[cfg(test)]
pub(crate) fn always_ok_before_access(path: &Path) -> BeforeAccessFuture<'static> {
    let path = path.to_path_buf();
    before_access_future(async move { checked_path_from_absolute_for_test(&path) })
}

#[cfg(test)]
pub(crate) fn always_ok_before_remove(path: &Path) -> BeforeRemoveFuture<'static> {
    let path = path.to_path_buf();
    before_remove_future(async move { checked_path_from_absolute_for_test(&path) })
}

#[cfg(test)]
fn checked_path_from_absolute_for_test(
    path: &Path,
) -> Result<CheckedMemoPath, MemoBeforeRenameError> {
    let parent = path
        .parent()
        .ok_or_else(|| MemoBeforeRenameError::internal("テスト用メモpathにparentがありません"))?;
    let file_name = path.file_name().ok_or_else(|| {
        MemoBeforeRenameError::internal("テスト用メモpathにfile nameがありません")
    })?;
    let root_dir = cap_std::fs::Dir::open_ambient_dir(parent, cap_std::ambient_authority())
        .map_err(|error| MemoBeforeRenameError::internal(error.to_string()))?;
    Ok(CheckedMemoPath::new(root_dir, PathBuf::from(file_name)))
}

#[derive(Debug)]
pub(crate) enum MemoRemoveError {
    Io(io::Error),
    BeforeRemove(MemoBeforeRenameError),
}

impl From<MemoBeforeRenameError> for MemoRemoveError {
    fn from(error: MemoBeforeRenameError) -> Self {
        Self::BeforeRemove(error)
    }
}

static ATOMIC_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const ATOMIC_TMP_ATTEMPTS: u8 = 8;
#[cfg(any(test, windows))]
const WINDOWS_ERROR_ACCESS_DENIED: i32 = 5;
#[cfg(any(test, windows))]
const WINDOWS_ERROR_SHARING_VIOLATION: i32 = 32;
#[cfg(any(test, windows))]
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;
#[cfg(any(test, windows))]
const WINDOWS_REPLACE_RETRY_DELAYS_MS: [u64; 3] = [10, 25, 50];

/// メモ保存先ファイルシステムの抽象。
///
/// サイズ上限の事前チェック等のドメイン責務は呼び出し側で行い、
/// 実読み取り量の上限は `read_with_limit` 側でも保証する。
/// `NotFound` 等の特殊エラー処理も呼び出し側で吸収する。
#[async_trait]
pub(crate) trait MemoFs: Send + Sync + std::fmt::Debug {
    /// パス存在確認。シンボリックリンク要素は呼び出し側で別途検査済み想定。
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool>;

    /// バイト列読み込み。
    ///
    /// 検証済み親ディレクトリhandle配下で開き、実読み取り量が `MAX_FILE_SIZE`
    /// を超えた場合は `MemoReadError::TooLarge` を返す。成功時に返す `Vec<u8>`
    /// は上限以下である。
    async fn read_with_limit(
        &self,
        path: &Path,
        before_read: &BeforeAccessCheck<'_>,
    ) -> Result<Vec<u8>, MemoReadError>;

    /// バイト列を同一ディレクトリ内 tmp へ書き込み、rename で最終パスへ差し替える。
    ///
    /// tmp 作成前に `before_write(path)` を呼び、検証済み親ディレクトリhandle配下で
    /// tmp作成・書き込み・rename・cleanupを完結する。tmp 作成から rename までの失敗、
    /// および rename 自体の失敗では tmp を best effort で削除し、最終保存先の既存内容を保持する。
    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_write: &BeforeAccessCheck<'_>,
    ) -> Result<(), MemoWriteError>;

    /// ファイル削除。削除直前に `before_remove(path)` を呼び出す。
    ///
    /// `NotFound` を含む I/O エラーは透過する（呼び出し側で吸収）。
    /// `before_remove` は symlink component や単一ファイル親ディレクトリの
    /// 同一性など、削除直前に再確認すべき不変条件を検査する。
    async fn remove_file(
        &self,
        path: &Path,
        before_remove: &BeforeRemoveCheck<'_>,
    ) -> Result<(), MemoRemoveError>;
}

/// 本番用 [`MemoFs`] 実装。`tokio::fs::*` を直接呼び出す。
#[derive(Debug, Default)]
pub(crate) struct TokioMemoFs;

fn atomic_tmp_file_name(file_name: &std::ffi::OsStr, counter: u64, attempt: u8) -> PathBuf {
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

    PathBuf::from(tmp_name)
}

#[cfg(test)]
fn atomic_tmp_path(path: &Path, counter: u64, attempt: u8) -> io::Result<PathBuf> {
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
    Ok(parent.join(atomic_tmp_file_name(file_name, counter, attempt)))
}

async fn write_atomic_with_counter(
    path: &Path,
    content: &[u8],
    before_write: &BeforeAccessCheck<'_>,
    counter: u64,
) -> Result<(), MemoWriteError> {
    let checked = before_write(path)
        .await
        .map_err(MemoWriteError::BeforeRename)?;
    let file_name = checked.file_name().to_path_buf();
    let mut last_already_exists = None;
    for attempt in 0..ATOMIC_TMP_ATTEMPTS {
        let tmp_path = atomic_tmp_file_name(file_name.as_os_str(), counter, attempt);
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        // tmp 名は推測可能なため、緩い umask の共有環境でも rename 前に他者読み取りさせない。
        options.mode(0o600);
        #[cfg(windows)]
        configure_windows_cap_atomic_tmp_options(&mut options);
        let mut tmp_file = match checked.parent_dir().open_with(&tmp_path, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_already_exists = Some(error);
                continue;
            }
            Err(error) => return Err(MemoWriteError::Io(error)),
        };

        if let Err(error) = tmp_file.write_all(content) {
            drop(tmp_file);
            cleanup_tmp_best_effort(checked.parent_dir(), &tmp_path);
            return Err(MemoWriteError::Io(error));
        }
        if let Err(error) = tmp_file.flush() {
            drop(tmp_file);
            cleanup_tmp_best_effort(checked.parent_dir(), &tmp_path);
            return Err(MemoWriteError::Io(error));
        }
        if let Err(error) = tmp_file.sync_data() {
            drop(tmp_file);
            cleanup_tmp_best_effort(checked.parent_dir(), &tmp_path);
            return Err(MemoWriteError::Io(error));
        }

        if let Err(error) =
            replace_checked_tmp(checked.parent_dir(), tmp_file, &tmp_path, &file_name).await
        {
            cleanup_tmp_best_effort(checked.parent_dir(), &tmp_path);
            return Err(MemoWriteError::Io(error));
        }

        sync_parent_dir_best_effort(checked.parent_dir());
        return Ok(());
    }

    let error = last_already_exists.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "memo temporary file already exists",
        )
    });
    let memo_name = file_name_for_logging(path);
    tracing::error!(
        "[markdown-view] メモ一時ファイル名が{}回連続で衝突したため保存を中止します ({}): {}",
        ATOMIC_TMP_ATTEMPTS,
        memo_name,
        error
    );
    Err(MemoWriteError::Io(error))
}

async fn replace_checked_tmp(
    parent_dir: &cap_std::fs::Dir,
    tmp_file: cap_std::fs::File,
    tmp_path: &Path,
    file_name: &Path,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        drop(tmp_file);
        parent_dir.rename(tmp_path, parent_dir, file_name)
    }

    #[cfg(windows)]
    {
        move_open_cap_tmp_replace(parent_dir, tmp_file, tmp_path, file_name).await
    }
}

#[cfg(windows)]
fn configure_windows_atomic_tmp_options(options: &mut tokio::fs::OpenOptions) {
    // handle-based renameに必要なDELETE accessを持たせ、retry中は外部書き込みをshare modeで拒否する。
    options.access_mode(FILE_GENERIC_WRITE | DELETE);
    options.share_mode(FILE_SHARE_DELETE);
}

#[cfg(windows)]
fn configure_windows_cap_atomic_tmp_options(options: &mut cap_std::fs::OpenOptions) {
    // handle-based renameに必要なDELETE accessを持たせ、retry中は外部書き込みをshare modeで拒否する。
    options.access_mode(FILE_GENERIC_WRITE | DELETE);
    options.share_mode(FILE_SHARE_DELETE);
}

fn sync_parent_dir_best_effort(parent: &cap_std::fs::Dir) {
    let Ok(file) = parent.try_clone() else {
        tracing::error!("[markdown-view] メモ保存後の親ディレクトリcloneに失敗しました");
        return;
    };
    let file = file.into_std_file();
    if let Err(error) = file.sync_all() {
        // rename成功後は応答を巻き戻せないためbest-effortだが、クラッシュ耐性の劣化としてerrorで残す。
        tracing::error!(
            "[markdown-view] メモ保存後の親ディレクトリsyncに失敗しました: {}",
            error
        );
    }
}

#[cfg(windows)]
async fn sync_parent_dir_required(parent: &Path) -> io::Result<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true);
    options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    let file = options.open(parent).await?;
    file.sync_all().await
}

#[cfg(windows)]
fn sync_parent_dir_handle_required(parent: &cap_std::fs::Dir) -> io::Result<()> {
    parent.try_clone()?.into_std_file().sync_all()
}

fn file_name_for_logging(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().escape_debug().to_string())
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn cleanup_tmp_best_effort(parent_dir: &cap_std::fs::Dir, tmp_path: &Path) {
    match parent_dir.remove_file(tmp_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            let tmp_name = file_name_for_logging(tmp_path);
            tracing::warn!(
                "[markdown-view] メモ一時ファイルcleanup失敗を無視します ({}): {}",
                tmp_name,
                error
            );
        }
    }
}

#[cfg(any(test, windows))]
fn is_retryable_windows_replace_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(
            WINDOWS_ERROR_ACCESS_DENIED
                | WINDOWS_ERROR_SHARING_VIOLATION
                | WINDOWS_ERROR_LOCK_VIOLATION
        )
    )
}

#[cfg(any(test, windows))]
fn map_atomic_replace_join_error(error: tokio::task::JoinError) -> io::Error {
    let task_id = error.id();
    if error.is_panic() {
        tracing::error!(
            task_id = %task_id,
            "[markdown-view] メモatomic replace blocking taskがpanicしました"
        );
        io::Error::other("memo atomic replace task panicked")
    } else if error.is_cancelled() {
        tracing::warn!(
            task_id = %task_id,
            "[markdown-view] メモatomic replace blocking taskがcancelledされました"
        );
        io::Error::other("memo atomic replace task cancelled")
    } else {
        tracing::warn!(
            task_id = %task_id,
            "[markdown-view] メモatomic replace blocking taskのjoinに失敗しました"
        );
        io::Error::other("memo atomic replace task failed")
    }
}

#[cfg(windows)]
async fn checked_atomic_replace(
    tmp_file: &tokio::fs::File,
    tmp_path: &Path,
    path: &Path,
    before_rename: &BeforeRenameCheck<'_>,
) -> Result<(), MemoWriteError> {
    atomic_replace_checked(tmp_file, tmp_path, path, before_rename).await
}

#[cfg(windows)]
async fn move_open_tmp_replace_once(tmp_file: &tokio::fs::File, path: &Path) -> io::Result<()> {
    let tmp_file = tmp_file.try_clone().await?.into_std().await;
    let path_wide = path_to_wide(path)?;
    move_open_std_file_replace_once(tmp_file, std::ptr::null_mut(), path_wide).await
}

#[cfg(windows)]
async fn move_open_cap_tmp_replace(
    parent_dir: &cap_std::fs::Dir,
    tmp_file: cap_std::fs::File,
    tmp_path: &Path,
    file_name: &Path,
) -> io::Result<()> {
    let file_name_wide = path_component_to_wide(file_name)?;
    let no_check =
        |_: &Path, _: &Path| before_rename_future(async { Ok::<(), MemoBeforeRenameError>(()) });

    replace_with_retry_and_revalidation(
        tmp_path,
        file_name,
        &no_check,
        || move_open_cap_tmp_replace_once(parent_dir, &tmp_file, &file_name_wide),
        |delay_ms| tokio::time::sleep(std::time::Duration::from_millis(delay_ms)),
    )
    .await?;
    sync_parent_dir_handle_required(parent_dir)?;
    Ok(())
}

#[cfg(windows)]
async fn move_open_cap_tmp_replace_once(
    parent_dir: &cap_std::fs::Dir,
    tmp_file: &cap_std::fs::File,
    file_name_wide: &[u16],
) -> io::Result<()> {
    let tmp_file = tmp_file.try_clone()?.into_std();
    let root_dir = parent_dir.try_clone()?.into_std_file();
    move_open_std_file_replace_once(
        tmp_file,
        root_dir.as_raw_handle().cast(),
        file_name_wide.to_vec(),
    )
    .await
}

#[cfg(windows)]
async fn move_open_std_file_replace_once(
    tmp_file: std::fs::File,
    root_directory: windows_sys::Win32::Foundation::HANDLE,
    path_wide: Vec<u16>,
) -> io::Result<()> {
    tokio::task::spawn_blocking(move || {
        let rename_info = build_file_rename_info(root_directory, &path_wide)?;
        // SAFETY: handleは生存中のtmp file objectを指し、rename_infoはFILE_RENAME_INFO layoutの
        // 可変長bufferとしてこの呼び出し中は生存する。
        let result = unsafe {
            SetFileInformationByHandle(
                tmp_file.as_raw_handle().cast(),
                FileRenameInfo,
                rename_info.as_ptr().cast(),
                u32::try_from(rename_info.len()).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "rename info buffer is too large",
                    )
                })?,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
    .await
    .map_err(map_atomic_replace_join_error)?
}

#[cfg(windows)]
fn build_file_rename_info(
    root_directory: windows_sys::Win32::Foundation::HANDLE,
    path_wide: &[u16],
) -> io::Result<Vec<u8>> {
    let file_name_length = path_wide
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is too long"))?;
    let file_name_offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_len = file_name_offset
        .checked_add(file_name_length as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is too long"))?;
    let mut buffer = vec![0_u8; buffer_len];

    // SAFETY: bufferはFILE_RENAME_INFO header以上の長さを持ち、FileNameの可変長領域へ
    // UTF-16 bytesをコピーする。path_wideはinterior NUL拒否済みでNUL終端しない。
    unsafe {
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        (*info).Anonymous.ReplaceIfExists = true;
        (*info).RootDirectory = root_directory;
        (*info).FileNameLength = file_name_length;
        std::ptr::copy_nonoverlapping(
            path_wide.as_ptr().cast::<u8>(),
            buffer.as_mut_ptr().add(file_name_offset),
            file_name_length as usize,
        );
    }

    Ok(buffer)
}

#[cfg(any(test, windows))]
async fn replace_with_retry_and_revalidation<Replace, ReplaceFuture, Sleep, SleepFuture>(
    tmp_path: &Path,
    path: &Path,
    before_rename: &BeforeRenameCheck<'_>,
    mut replace_once: Replace,
    mut sleep: Sleep,
) -> Result<(), MemoWriteError>
where
    Replace: FnMut() -> ReplaceFuture,
    ReplaceFuture: Future<Output = io::Result<()>>,
    Sleep: FnMut(u64) -> SleepFuture,
    SleepFuture: Future<Output = ()>,
{
    let memo_name = file_name_for_logging(path);

    let mut attempt = 0;
    loop {
        before_rename(path, tmp_path)
            .await
            .map_err(MemoWriteError::BeforeRename)?;

        match replace_once().await {
            Ok(()) => return Ok(()),
            Err(error) if is_retryable_windows_replace_error(&error) => {
                let Some(&delay_ms) = WINDOWS_REPLACE_RETRY_DELAYS_MS.get(attempt) else {
                    return Err(MemoWriteError::Io(error));
                };
                attempt += 1;
                tracing::warn!(
                    "[markdown-view] メモatomic replaceをretryします (file: {}, attempt: {}, delay_ms: {}, error: {})",
                    memo_name,
                    attempt,
                    delay_ms,
                    error
                );
                sleep(delay_ms).await;
            }
            Err(error) => return Err(MemoWriteError::Io(error)),
        }
    }
}

#[cfg(windows)]
async fn atomic_replace(tmp_path: &Path, path: &Path) -> io::Result<()> {
    let tmp_file = open_windows_atomic_tmp_for_replace(tmp_path).await?;
    let no_check =
        |_: &Path, _: &Path| before_rename_future(async { Ok::<(), MemoBeforeRenameError>(()) });

    replace_with_retry_and_revalidation(
        tmp_path,
        path,
        &no_check,
        || move_open_tmp_replace_once(&tmp_file, path),
        |delay_ms| tokio::time::sleep(std::time::Duration::from_millis(delay_ms)),
    )
    .await
    .map_err(|error| match error {
        MemoWriteError::Io(error) => error,
        MemoWriteError::BeforeRename(error) => io::Error::other(error.user_message().to_owned()),
    })
}

#[cfg(windows)]
async fn atomic_replace_checked(
    tmp_file: &tokio::fs::File,
    tmp_path: &Path,
    path: &Path,
    before_rename: &BeforeRenameCheck<'_>,
) -> Result<(), MemoWriteError> {
    path_to_wide(path).map_err(MemoWriteError::Io)?;

    replace_with_retry_and_revalidation(
        tmp_path,
        path,
        before_rename,
        || move_open_tmp_replace_once(tmp_file, path),
        |delay_ms| tokio::time::sleep(std::time::Duration::from_millis(delay_ms)),
    )
    .await
}

#[cfg(windows)]
async fn open_windows_atomic_tmp_for_replace(tmp_path: &Path) -> io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true);
    configure_windows_atomic_tmp_options(&mut options);
    options.open(tmp_path).await
}

#[cfg(windows)]
fn path_to_wide(path: &Path) -> io::Result<Vec<u16>> {
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
    Ok(wide)
}

#[cfg(windows)]
fn path_component_to_wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => path_to_wide(path),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must be a single file name",
        )),
    }
}

#[cfg(windows)]
fn path_to_wide_null(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide = path_to_wide(path)?;
    wide.push(0);
    Ok(wide)
}

#[async_trait]
impl MemoFs for TokioMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        tokio::fs::try_exists(path).await
    }

    async fn read_with_limit(
        &self,
        path: &Path,
        before_read: &BeforeAccessCheck<'_>,
    ) -> Result<Vec<u8>, MemoReadError> {
        let checked = before_read(path)
            .await
            .map_err(MemoReadError::BeforeAccess)?;
        let file = open_relative_file_nofollow(checked.parent_dir(), checked.file_name())
            .map_err(MemoReadError::Open)?
            .into_std();
        let file = tokio::fs::File::from_std(file);
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

    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_write: &BeforeAccessCheck<'_>,
    ) -> Result<(), MemoWriteError> {
        let counter = ATOMIC_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        write_atomic_with_counter(path, content, before_write, counter).await
    }

    async fn remove_file(
        &self,
        path: &Path,
        before_remove: &BeforeRemoveCheck<'_>,
    ) -> Result<(), MemoRemoveError> {
        let checked = before_remove(path).await?;
        checked
            .parent_dir()
            .remove_file(checked.file_name())
            .map_err(MemoRemoveError::Io)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(windows)]
    use std::os::windows::ffi::OsStringExt;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::server::files::MAX_FILE_SIZE;

    #[test]
    fn is_retryable_windows_replace_errorは一時lock系windowsエラーだけtrueにする() {
        for code in [
            WINDOWS_ERROR_ACCESS_DENIED,
            WINDOWS_ERROR_SHARING_VIOLATION,
            WINDOWS_ERROR_LOCK_VIOLATION,
        ] {
            let error = io::Error::from_raw_os_error(code);
            assert!(
                is_retryable_windows_replace_error(&error),
                "Windows error {code} should be retryable"
            );
        }
    }

    #[test]
    fn is_retryable_windows_replace_errorは対象外エラーをfalseにする() {
        for error in [
            io::Error::from(io::ErrorKind::AlreadyExists),
            io::Error::from(io::ErrorKind::Other),
            io::Error::from_raw_os_error(12345),
        ] {
            assert!(
                !is_retryable_windows_replace_error(&error),
                "unexpected retryable error: {error}"
            );
        }
    }

    #[tokio::test]
    async fn map_atomic_replace_join_errorはpanicを分類する() {
        let handle = tokio::spawn(async {
            panic!("atomic replace panic classification test");
        });
        let join_error = handle.await.expect_err("task should panic");

        let error = map_atomic_replace_join_error(join_error);

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(
            error.to_string().contains("panicked"),
            "panic classification should be visible in io error: {error}"
        );
    }

    #[tokio::test]
    async fn map_atomic_replace_join_errorはcancelledを分類する() {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        handle.abort();
        let join_error = handle.await.expect_err("task should be cancelled");

        let error = map_atomic_replace_join_error(join_error);

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(
            error.to_string().contains("cancelled"),
            "cancel classification should be visible in io error: {error}"
        );
    }

    #[tokio::test]
    async fn windows_atomic_replace_retryはretryごとにbefore_renameを再実行する() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        let tmp_path = workspace.path().join("memo.md.tmp");
        let replace_attempts = Arc::new(Mutex::new(0));
        let validation_count = Arc::new(Mutex::new(0));
        let slept_delays = Arc::new(Mutex::new(Vec::new()));

        replace_with_retry_and_revalidation(
            &tmp_path,
            &path,
            &{
                let validation_count = Arc::clone(&validation_count);
                move |_, _| {
                    let validation_count = Arc::clone(&validation_count);
                    before_rename_future(async move {
                        *validation_count
                            .lock()
                            .expect("validation count mutex should not be poisoned") += 1;
                        Ok(())
                    })
                }
            },
            {
                let replace_attempts = Arc::clone(&replace_attempts);
                move || {
                    let replace_attempts = Arc::clone(&replace_attempts);
                    async move {
                        let mut attempts = replace_attempts
                            .lock()
                            .expect("replace attempts mutex should not be poisoned");
                        *attempts += 1;
                        if *attempts == 1 {
                            Err(io::Error::from_raw_os_error(
                                WINDOWS_ERROR_SHARING_VIOLATION,
                            ))
                        } else {
                            Ok(())
                        }
                    }
                }
            },
            {
                let slept_delays = Arc::clone(&slept_delays);
                move |delay_ms| {
                    let slept_delays = Arc::clone(&slept_delays);
                    async move {
                        slept_delays
                            .lock()
                            .expect("slept delays mutex should not be poisoned")
                            .push(delay_ms);
                    }
                }
            },
        )
        .await
        .expect("retry should eventually succeed");

        assert_eq!(
            *validation_count
                .lock()
                .expect("validation count mutex should not be poisoned"),
            2
        );
        assert_eq!(
            *replace_attempts
                .lock()
                .expect("replace attempts mutex should not be poisoned"),
            2
        );
        assert_eq!(
            *slept_delays
                .lock()
                .expect("slept delays mutex should not be poisoned"),
            vec![10]
        );
    }

    #[tokio::test]
    async fn windows_atomic_replace_retryは再検証失敗時にreplaceを呼ばない() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        let tmp_path = workspace.path().join("memo.md.tmp");
        let replace_attempts = Arc::new(Mutex::new(0));
        let validation_count = Arc::new(Mutex::new(0));

        let error = replace_with_retry_and_revalidation(
            &tmp_path,
            &path,
            &{
                let validation_count = Arc::clone(&validation_count);
                move |_, _| {
                    let validation_count = Arc::clone(&validation_count);
                    before_rename_future(async move {
                        let mut count = validation_count
                            .lock()
                            .expect("validation count mutex should not be poisoned");
                        *count += 1;
                        if *count == 2 {
                            Err(MemoBeforeRenameError::new("unsafe retry path"))
                        } else {
                            Ok(())
                        }
                    })
                }
            },
            {
                let replace_attempts = Arc::clone(&replace_attempts);
                move || {
                    let replace_attempts = Arc::clone(&replace_attempts);
                    async move {
                        *replace_attempts
                            .lock()
                            .expect("replace attempts mutex should not be poisoned") += 1;
                        Err(io::Error::from_raw_os_error(
                            WINDOWS_ERROR_SHARING_VIOLATION,
                        ))
                    }
                }
            },
            |_| async {},
        )
        .await
        .expect_err("second validation should fail");

        match error {
            MemoWriteError::BeforeRename(error) => {
                assert_eq!(error.user_message(), "unsafe retry path");
            }
            MemoWriteError::Io(error) => panic!("unexpected io error: {error}"),
        }
        assert_eq!(
            *validation_count
                .lock()
                .expect("validation count mutex should not be poisoned"),
            2
        );
        assert_eq!(
            *replace_attempts
                .lock()
                .expect("replace attempts mutex should not be poisoned"),
            1
        );
    }

    #[tokio::test]
    async fn windows_atomic_replace_retryは非retryエラーを即時返す() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        let tmp_path = workspace.path().join("memo.md.tmp");
        let replace_attempts = Arc::new(Mutex::new(0));
        let slept_delays = Arc::new(Mutex::new(Vec::new()));

        let error = replace_with_retry_and_revalidation(
            &tmp_path,
            &path,
            &always_ok_before_rename,
            {
                let replace_attempts = Arc::clone(&replace_attempts);
                move || {
                    let replace_attempts = Arc::clone(&replace_attempts);
                    async move {
                        *replace_attempts
                            .lock()
                            .expect("replace attempts mutex should not be poisoned") += 1;
                        Err(io::Error::from_raw_os_error(12345))
                    }
                }
            },
            {
                let slept_delays = Arc::clone(&slept_delays);
                move |delay_ms| {
                    let slept_delays = Arc::clone(&slept_delays);
                    async move {
                        slept_delays
                            .lock()
                            .expect("slept delays mutex should not be poisoned")
                            .push(delay_ms);
                    }
                }
            },
        )
        .await
        .expect_err("non-retryable error should fail immediately");

        match error {
            MemoWriteError::Io(error) => {
                assert_eq!(error.raw_os_error(), Some(12345));
            }
            MemoWriteError::BeforeRename(error) => {
                panic!("unexpected before_rename error: {}", error.user_message());
            }
        }
        assert_eq!(
            *replace_attempts
                .lock()
                .expect("replace attempts mutex should not be poisoned"),
            1
        );
        assert!(
            slept_delays
                .lock()
                .expect("slept delays mutex should not be poisoned")
                .is_empty(),
            "non-retryable error should not sleep"
        );
    }

    #[tokio::test]
    async fn windows_atomic_replace_retryは上限までretryして最後のエラーを返す() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        let tmp_path = workspace.path().join("memo.md.tmp");
        let replace_attempts = Arc::new(Mutex::new(0));
        let slept_delays = Arc::new(Mutex::new(Vec::new()));

        let error = replace_with_retry_and_revalidation(
            &tmp_path,
            &path,
            &always_ok_before_rename,
            {
                let replace_attempts = Arc::clone(&replace_attempts);
                move || {
                    let replace_attempts = Arc::clone(&replace_attempts);
                    async move {
                        *replace_attempts
                            .lock()
                            .expect("replace attempts mutex should not be poisoned") += 1;
                        Err(io::Error::from_raw_os_error(WINDOWS_ERROR_LOCK_VIOLATION))
                    }
                }
            },
            {
                let slept_delays = Arc::clone(&slept_delays);
                move |delay_ms| {
                    let slept_delays = Arc::clone(&slept_delays);
                    async move {
                        slept_delays
                            .lock()
                            .expect("slept delays mutex should not be poisoned")
                            .push(delay_ms);
                    }
                }
            },
        )
        .await
        .expect_err("retry exhaustion should return the final error");

        match error {
            MemoWriteError::Io(error) => {
                assert_eq!(error.raw_os_error(), Some(WINDOWS_ERROR_LOCK_VIOLATION));
            }
            MemoWriteError::BeforeRename(error) => {
                panic!("unexpected before_rename error: {}", error.user_message());
            }
        }
        assert_eq!(
            *replace_attempts
                .lock()
                .expect("replace attempts mutex should not be poisoned"),
            WINDOWS_REPLACE_RETRY_DELAYS_MS.len() + 1
        );
        assert_eq!(
            *slept_delays
                .lock()
                .expect("slept delays mutex should not be poisoned"),
            WINDOWS_REPLACE_RETRY_DELAYS_MS
        );
    }

    #[tokio::test]
    async fn test_tokio_memo_fs_read_with_limitは実読み取り上限超過をtoo_largeにする() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".README.md.memo.md");
        tokio::fs::write(&path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
            .await
            .unwrap();

        let result = TokioMemoFs
            .read_with_limit(&path, &always_ok_before_access)
            .await;

        assert!(matches!(result, Err(MemoReadError::TooLarge)));
    }

    #[test]
    fn file_name_for_loggingは制御文字をescapeする() {
        let path = PathBuf::from("memo\nname.md");

        let name = file_name_for_logging(&path);

        assert_eq!(name, "memo\\nname.md");
    }

    #[tokio::test]
    async fn write_atomicはtmpを書いてからrenameする() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        tokio::fs::write(&path, b"old")
            .await
            .expect("initial memo should be written");
        let path_for_check = path.clone();

        TokioMemoFs
            .write_atomic(&path, b"new", &move |final_path| {
                let final_path = final_path.to_path_buf();
                let path_for_check = path_for_check.clone();
                before_access_future(async move {
                    assert_eq!(final_path.as_path(), path_for_check.as_path());
                    assert_eq!(
                        std::fs::read(&final_path).expect("final path should still be readable"),
                        b"old"
                    );
                    checked_path_from_absolute_for_test(&final_path)
                })
            })
            .await
            .expect("atomic write should succeed");

        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"new"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn write_atomicは同一パス並行保存でもtmpを残さず完全な最終内容にする() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        tokio::fs::write(&path, b"old")
            .await
            .expect("initial memo should be written");
        let fs = Arc::new(TokioMemoFs);
        let mut handles = Vec::new();

        for index in 0..16 {
            let fs = Arc::clone(&fs);
            let path = path.clone();
            let content = format!("parallel-memo-{index:02}-{}", "x".repeat(index + 1));
            handles.push(tokio::spawn(async move {
                fs.write_atomic(&path, content.as_bytes(), &always_ok_before_access)
                    .await
                    .expect("parallel atomic write should succeed");
                content.into_bytes()
            }));
        }

        let mut expected_contents = Vec::new();
        for handle in handles {
            expected_contents.push(handle.await.expect("parallel task should finish"));
        }
        let final_content = tokio::fs::read(&path)
            .await
            .expect("final memo should be readable");
        assert!(
            expected_contents
                .iter()
                .any(|content| content == &final_content),
            "final memo should be one complete concurrent write"
        );

        let mut entries = tokio::fs::read_dir(workspace.path())
            .await
            .expect("workspace entries should be readable");
        while let Some(entry) = entries
            .next_entry()
            .await
            .expect("workspace entry should be readable")
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(
                !name.contains(".tmp."),
                "atomic tmp file should not remain: {name}"
            );
        }
    }

    #[tokio::test]
    async fn write_atomicはbefore_write失敗時にtmpを作らず元内容を残す() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        tokio::fs::write(&path, b"old")
            .await
            .expect("initial memo should be written");

        let err = TokioMemoFs
            .write_atomic(&path, b"new", &move |_| {
                before_access_future(async move { Err(MemoBeforeRenameError::new("conflict")) })
            })
            .await
            .expect_err("before_write error should be returned");

        match err {
            MemoWriteError::BeforeRename(error) => {
                assert_eq!(error.user_message(), "conflict");
            }
            MemoWriteError::Io(error) => panic!("unexpected io error: {error}"),
        }

        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"old"
        );
        let entries = std::fs::read_dir(workspace.path()).expect("workspace entries should list");
        assert_eq!(
            entries.count(),
            1,
            "tmp file should not be created before validation"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_atomicはtmpを所有者のみ読み書き可能で作成する() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        TokioMemoFs
            .write_atomic(&path, b"secret", &always_ok_before_access)
            .await
            .expect("atomic write should succeed");

        assert_eq!(
            std::fs::metadata(&path)
                .expect("final memo metadata should be readable")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_atomic_tmp_optionsは外部書き込みopenを拒否する() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let parent_dir =
            cap_std::fs::Dir::open_ambient_dir(workspace.path(), cap_std::ambient_authority())
                .expect("workspace should open as cap dir");
        let tmp_name = Path::new("memo.md.tmp");
        let tmp_path = workspace.path().join(tmp_name);
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        configure_windows_cap_atomic_tmp_options(&mut options);
        let _tmp_file = parent_dir
            .open_with(tmp_name, &options)
            .expect("protected tmp should be created");

        let error = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&tmp_path)
            .await
            .expect_err("tmp should reject external write open while protected");

        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Other
            ),
            "Windows should reject concurrent tmp write open while memo save owns the handle"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn sync_parent_dir_requiredはwindowsディレクトリをsyncできる() {
        let workspace = tempfile::tempdir().expect("workspace should be created");

        sync_parent_dir_required(workspace.path())
            .await
            .expect("Windows parent directory sync should succeed for a normal directory");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn write_atomicはwindowsで成功後のtmp_path差し替えファイルを削除しない() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let parent_dir =
            cap_std::fs::Dir::open_ambient_dir(workspace.path(), cap_std::ambient_authority())
                .expect("workspace should open as cap dir");
        let path = workspace.path().join("memo.md");
        let tmp_name = Path::new("memo.md.tmp");
        let tmp_path = workspace.path().join(tmp_name);
        let swapped_path = workspace.path().join("memo.md.tmp.swapped");
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        configure_windows_cap_atomic_tmp_options(&mut options);
        let mut tmp_file = parent_dir
            .open_with(tmp_name, &options)
            .expect("protected tmp should be created");
        tmp_file
            .write_all(b"trusted")
            .expect("trusted tmp content should be written");
        tmp_file.flush().expect("tmp content should be flushed");
        tmp_file.sync_data().expect("tmp content should be synced");

        std::fs::rename(&tmp_path, &swapped_path)
            .expect("delete-shared tmp path should be swappable before replace");
        std::fs::write(&tmp_path, b"attacker")
            .expect("attacker replacement tmp should be writable");

        move_open_cap_tmp_replace(&parent_dir, tmp_file, tmp_name, Path::new("memo.md"))
            .await
            .expect("atomic write should succeed with the held tmp handle");

        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"trusted"
        );
        assert_eq!(
            tokio::fs::read(&tmp_path)
                .await
                .expect("attacker tmp should not be deleted after successful replace"),
            b"attacker"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_atomic_replace_retryはtmp_path_swap後も保持handleの内容を置換する() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        let tmp_path = workspace.path().join("memo.md.tmp");
        let swapped_path = workspace.path().join("memo.md.swapped");
        tokio::fs::write(&path, b"old")
            .await
            .expect("existing memo should be written");

        let tmp_file = Arc::new({
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            configure_windows_atomic_tmp_options(&mut options);
            let mut file = options
                .open(&tmp_path)
                .await
                .expect("protected tmp should be created");
            file.write_all(b"trusted")
                .await
                .expect("trusted tmp content should be written");
            file.flush().await.expect("tmp content should be flushed");
            file.sync_data()
                .await
                .expect("tmp content should be synced");
            file
        });
        let replace_attempts = Arc::new(Mutex::new(0));
        let validation_count = Arc::new(Mutex::new(0));

        replace_with_retry_and_revalidation(
            &tmp_path,
            &path,
            &{
                let validation_count = Arc::clone(&validation_count);
                let tmp_path = tmp_path.clone();
                let swapped_path = swapped_path.clone();
                move |_, _| {
                    let validation_count = Arc::clone(&validation_count);
                    let tmp_path = tmp_path.clone();
                    let swapped_path = swapped_path.clone();
                    before_rename_future(async move {
                        let mut count = validation_count
                            .lock()
                            .expect("validation count mutex should not be poisoned");
                        *count += 1;
                        if *count == 2 {
                            std::fs::rename(&tmp_path, &swapped_path)
                                .expect("delete-shared tmp path should be swappable");
                            std::fs::write(&tmp_path, b"attacker")
                                .expect("attacker replacement tmp should be writable");
                        }
                        Ok(())
                    })
                }
            },
            {
                let replace_attempts = Arc::clone(&replace_attempts);
                let tmp_file = Arc::clone(&tmp_file);
                let path = path.clone();
                move || {
                    let replace_attempts = Arc::clone(&replace_attempts);
                    let tmp_file = Arc::clone(&tmp_file);
                    let path = path.clone();
                    async move {
                        let attempt = {
                            let mut attempts = replace_attempts
                                .lock()
                                .expect("replace attempts mutex should not be poisoned");
                            *attempts += 1;
                            *attempts
                        };
                        if attempt == 1 {
                            Err(io::Error::from_raw_os_error(
                                WINDOWS_ERROR_SHARING_VIOLATION,
                            ))
                        } else {
                            move_open_tmp_replace_once(tmp_file.as_ref(), &path).await
                        }
                    }
                }
            },
            |_| async {},
        )
        .await
        .expect("retry should replace using the held tmp handle");

        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"trusted"
        );
        assert_eq!(
            tokio::fs::read(&tmp_path)
                .await
                .expect("attacker tmp should remain at original tmp path"),
            b"attacker"
        );
    }

    #[tokio::test]
    async fn write_atomicはtmp衝突がattempt上限まで続くと失敗する() {
        let workspace = tempfile::tempdir().expect("workspace should be created");
        let path = workspace.path().join("memo.md");
        tokio::fs::write(&path, b"old")
            .await
            .expect("initial memo should be written");

        let counter = 0;
        for attempt in 0..ATOMIC_TMP_ATTEMPTS {
            let tmp_path = atomic_tmp_path(&path, counter, attempt).expect("tmp path should build");
            tokio::fs::write(tmp_path, b"occupied")
                .await
                .expect("occupied tmp should be written");
        }

        // counter 0 の全 attempt を占有し、retry 枯渇時も最終ファイルを壊さないことを固定する。
        let err = write_atomic_with_counter(&path, b"new", &always_ok_before_access, counter)
            .await
            .expect_err("occupied tmp attempts should exhaust");

        match err {
            MemoWriteError::Io(error) => {
                assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
            }
            MemoWriteError::BeforeRename(error) => {
                panic!("unexpected before_rename error: {}", error.user_message());
            }
        }
        assert_eq!(
            tokio::fs::read(&path).await.expect("memo should be read"),
            b"old"
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
