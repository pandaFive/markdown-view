//! サーバー状態とモード判定を管理する。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard,
};

use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

use super::files::{MemoFs, TokioMemoFs};
use super::messages::BroadcastMessage;
use crate::renderer::syntax_theme_css;

pub(crate) const MAX_SEARCH_GENERATION_CLIENTS: usize = 128;
pub(crate) const MAX_CONCURRENT_DIRECTORY_SEARCHES: usize = 4;

/// canonicalize済みの絶対パスと生成時のファイルシステム実体
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CanonicalPath {
    path: PathBuf,
    identity: PathIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PathIdentity {
    Known {
        device: u64,
        file: u64,
    },
    #[allow(dead_code)]
    Unknown,
}

impl PathIdentity {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        identity_from_metadata(metadata)
    }

    fn from_cap_metadata(metadata: &cap_primitives::fs::Metadata) -> Self {
        identity_from_cap_metadata(metadata)
    }

    fn matches_metadata(&self, metadata: &std::fs::Metadata) -> bool {
        match self {
            PathIdentity::Known { .. } => self == &Self::from_metadata(metadata),
            PathIdentity::Unknown => false,
        }
    }

    fn matches_cap_metadata(&self, metadata: &cap_primitives::fs::Metadata) -> bool {
        match self {
            PathIdentity::Known { .. } => self == &Self::from_cap_metadata(metadata),
            PathIdentity::Unknown => false,
        }
    }

    fn is_supported(&self) -> bool {
        matches!(self, PathIdentity::Known { .. })
    }
}

#[cfg(unix)]
fn identity_from_metadata(metadata: &std::fs::Metadata) -> PathIdentity {
    use std::os::unix::fs::MetadataExt;

    PathIdentity::Known {
        device: metadata.dev(),
        file: metadata.ino(),
    }
}

#[cfg(unix)]
fn identity_from_cap_metadata(metadata: &cap_primitives::fs::Metadata) -> PathIdentity {
    use cap_primitives::fs::MetadataExt;

    PathIdentity::Known {
        device: metadata.dev(),
        file: metadata.ino(),
    }
}

#[cfg(windows)]
fn identity_from_metadata(metadata: &std::fs::Metadata) -> PathIdentity {
    use std::os::windows::fs::MetadataExt;

    match (metadata.volume_serial_number(), metadata.file_index()) {
        (Some(volume), Some(index)) => PathIdentity::Known {
            device: u64::from(volume),
            file: index,
        },
        _ => PathIdentity::Unknown,
    }
}

#[cfg(windows)]
fn identity_from_cap_metadata(metadata: &cap_primitives::fs::Metadata) -> PathIdentity {
    use cap_primitives::fs::MetadataExt;

    match (metadata.volume_serial_number(), metadata.file_index()) {
        (Some(volume), Some(index)) => PathIdentity::Known {
            device: u64::from(volume),
            file: index,
        },
        _ => PathIdentity::Unknown,
    }
}

#[cfg(not(any(unix, windows)))]
fn identity_from_metadata(_: &std::fs::Metadata) -> PathIdentity {
    PathIdentity::Unknown
}

#[cfg(not(any(unix, windows)))]
fn identity_from_cap_metadata(_: &cap_primitives::fs::Metadata) -> PathIdentity {
    PathIdentity::Unknown
}

impl CanonicalPath {
    /// パスをcanonicalizeして`CanonicalPath`を生成する
    pub fn try_from_path(path: impl AsRef<Path>) -> Result<Self, CanonicalPathError> {
        let canonical = path
            .as_ref()
            .canonicalize()
            .map_err(CanonicalPathError::Canonicalize)?;
        let metadata = std::fs::metadata(&canonical).map_err(CanonicalPathError::Metadata)?;
        let identity = PathIdentity::from_metadata(&metadata);
        if !identity.is_supported() {
            return Err(CanonicalPathError::UnsupportedIdentity);
        }
        Ok(Self {
            path: canonical,
            identity,
        })
    }

    /// `Path`として参照する
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// 現在のパスが生成時と同じファイルシステム実体を指しているか確認する
    pub(crate) fn has_current_identity(&self) -> std::io::Result<bool> {
        if !self.identity.is_supported() {
            return Err(unsupported_identity_error());
        }
        let metadata = std::fs::metadata(&self.path)?;
        Ok(self.identity.matches_metadata(&metadata))
    }

    /// 指定されたcapability metadataが生成時と同じファイルシステム実体を指しているか確認する
    pub(crate) fn matches_cap_metadata_identity(
        &self,
        metadata: &cap_primitives::fs::Metadata,
    ) -> bool {
        self.identity.matches_cap_metadata(metadata)
    }

    #[cfg(test)]
    pub(crate) fn unknown_identity_for_test(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().canonicalize().unwrap(),
            identity: PathIdentity::Unknown,
        }
    }
}

impl AsRef<Path> for CanonicalPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

/// `CanonicalPath` 生成エラー
#[derive(Debug)]
pub enum CanonicalPathError {
    /// canonicalize失敗
    Canonicalize(std::io::Error),
    /// メタデータ取得失敗
    Metadata(std::io::Error),
    /// ファイルシステム実体IDを取得できない
    UnsupportedIdentity,
}

impl std::fmt::Display for CanonicalPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonicalPathError::Canonicalize(e) => {
                write!(f, "パスの正規化に失敗しました: {}", e)
            }
            CanonicalPathError::Metadata(e) => {
                write!(f, "パスのメタデータ取得に失敗しました: {}", e)
            }
            CanonicalPathError::UnsupportedIdentity => {
                write!(f, "このファイルシステムではパスの実体IDを取得できません")
            }
        }
    }
}

impl std::error::Error for CanonicalPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CanonicalPathError::Canonicalize(error) | CanonicalPathError::Metadata(error) => {
                Some(error)
            }
            CanonicalPathError::UnsupportedIdentity => None,
        }
    }
}

fn unsupported_identity_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "このファイルシステムではパスの実体IDを取得できません",
    )
}

/// `AppMode` 構築エラー
#[derive(Debug)]
pub enum AppModeBuildError {
    /// canonicalize済みパスの生成に失敗
    CanonicalPath(CanonicalPathError),
    /// 単一ファイルモードでファイル以外が指定された
    NotFile(PathBuf),
    /// ディレクトリモードでディレクトリ以外が指定された
    NotDirectory(PathBuf),
    /// 単一ファイルモードで.md以外が指定された
    NotMarkdown(PathBuf),
}

impl std::fmt::Display for AppModeBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppModeBuildError::CanonicalPath(e) => write!(f, "{}", e),
            AppModeBuildError::NotFile(path) => {
                write!(
                    f,
                    "単一ファイルモードにはファイルを指定してください: {}",
                    path.display()
                )
            }
            AppModeBuildError::NotDirectory(path) => {
                write!(
                    f,
                    "ディレクトリモードにはディレクトリを指定してください: {}",
                    path.display()
                )
            }
            AppModeBuildError::NotMarkdown(path) => {
                write!(f, ".mdファイルのみ指定可能です: {}", path.display())
            }
        }
    }
}

impl std::error::Error for AppModeBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppModeBuildError::CanonicalPath(error) => Some(error),
            AppModeBuildError::NotFile(_)
            | AppModeBuildError::NotDirectory(_)
            | AppModeBuildError::NotMarkdown(_) => None,
        }
    }
}

fn metadata_for_mode(canonical: &CanonicalPath) -> std::io::Result<std::fs::Metadata> {
    std::fs::metadata(canonical.as_path())
}

fn ensure_canonical_file(canonical: &CanonicalPath) -> Result<(), AppModeBuildError> {
    let metadata = metadata_for_mode(canonical)
        .map_err(|_| AppModeBuildError::NotFile(canonical.as_path().to_path_buf()))?;
    if !metadata.file_type().is_file() {
        return Err(AppModeBuildError::NotFile(
            canonical.as_path().to_path_buf(),
        ));
    }
    Ok(())
}

fn ensure_canonical_directory(canonical: &CanonicalPath) -> Result<(), AppModeBuildError> {
    let metadata = metadata_for_mode(canonical)
        .map_err(|_| AppModeBuildError::NotDirectory(canonical.as_path().to_path_buf()))?;
    if !metadata.file_type().is_dir() {
        return Err(AppModeBuildError::NotDirectory(
            canonical.as_path().to_path_buf(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum AppModeKind {
    SingleFile(CanonicalPath),
    Directory(CanonicalPath),
}

/// アプリケーション動作モード
#[derive(Debug, Clone)]
pub struct AppMode(AppModeKind);

impl AppMode {
    /// 単一ファイルモードを生成する（canonicalize済み・.mdファイルのみ許可）
    pub fn new_single_file(path: impl AsRef<Path>) -> Result<Self, AppModeBuildError> {
        let canonical =
            CanonicalPath::try_from_path(path).map_err(AppModeBuildError::CanonicalPath)?;
        ensure_canonical_file(&canonical)?;
        match canonical.as_path().extension() {
            Some(ext) if ext.eq_ignore_ascii_case("md") => {}
            _ => {
                return Err(AppModeBuildError::NotMarkdown(
                    canonical.as_path().to_path_buf(),
                ));
            }
        }
        Ok(Self(AppModeKind::SingleFile(canonical)))
    }

    /// ディレクトリモードを生成する（canonicalize済みディレクトリのみ許可）
    pub fn new_directory(path: impl AsRef<Path>) -> Result<Self, AppModeBuildError> {
        let canonical =
            CanonicalPath::try_from_path(path).map_err(AppModeBuildError::CanonicalPath)?;
        ensure_canonical_directory(&canonical)?;
        Ok(Self(AppModeKind::Directory(canonical)))
    }

    /// ベースディレクトリを返す（ファイルモードは親、ディレクトリモードはそのまま）
    pub fn base_dir(&self) -> &Path {
        match &self.0 {
            AppModeKind::SingleFile(p) => p.as_path().parent().unwrap_or(p.as_path()),
            AppModeKind::Directory(p) => p.as_path(),
        }
    }

    /// 単一ファイルモードのパスを返す（ディレクトリモードはNone）
    pub fn single_file(&self) -> Option<&Path> {
        match &self.0 {
            AppModeKind::SingleFile(p) => Some(p.as_path()),
            AppModeKind::Directory(_) => None,
        }
    }

    /// ディレクトリモードのパスを返す（単一ファイルモードはNone）
    pub fn directory(&self) -> Option<&Path> {
        match &self.0 {
            AppModeKind::SingleFile(_) => None,
            AppModeKind::Directory(p) => Some(p.as_path()),
        }
    }

    /// 単一ファイルモードの正規化パスを返す。ディレクトリモードの場合はNone
    pub(crate) fn single_file_canonical(&self) -> Option<&CanonicalPath> {
        match &self.0 {
            AppModeKind::SingleFile(path) => Some(path),
            AppModeKind::Directory(_) => None,
        }
    }

    /// ディレクトリモードの正規化パスを返す。単一ファイルモードの場合はNone
    pub(crate) fn directory_canonical(&self) -> Option<&CanonicalPath> {
        match &self.0 {
            AppModeKind::SingleFile(_) => None,
            AppModeKind::Directory(path) => Some(path),
        }
    }

    /// ディレクトリモードかどうか
    pub fn is_directory(&self) -> bool {
        matches!(self.0, AppModeKind::Directory(_))
    }

    /// ディレクトリモード時にファイルの相対パスを計算する
    ///
    /// `file_path` はcanonicalize済みの絶対パスであること。
    /// strip_prefix失敗時はNoneを返す。
    /// 単一ファイルモードでは常にNoneを返す。
    pub fn relative_path_of(&self, file_path: &Path) -> Option<String> {
        match &self.0 {
            AppModeKind::Directory(base) => file_path
                .strip_prefix(base.as_path())
                .ok()
                .map(relative_path_to_display_string),
            AppModeKind::SingleFile(_) => None,
        }
    }
}

fn relative_path_to_display_string(relative: &Path) -> String {
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// ディレクトリ検索の開始世代と現在世代を比較するためのhandle。
#[derive(Debug, Clone)]
pub(crate) struct SearchGeneration {
    started_at: u64,
    current: Arc<AtomicU64>,
    force_stale: bool,
}

/// 検索client世代storeがactive entryだけで上限に到達している。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchGenerationLimitError;

/// ディレクトリ検索の同時実行数が上限に到達している。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchConcurrencyLimitError;

impl SearchGeneration {
    pub(crate) fn new(started_at: u64, current: Arc<AtomicU64>) -> Self {
        Self {
            started_at,
            current,
            force_stale: false,
        }
    }

    fn stale(current: Arc<AtomicU64>) -> Self {
        let started_at = current.load(Ordering::Acquire);
        Self {
            started_at,
            current,
            force_stale: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn started_at(&self) -> u64 {
        self.started_at
    }

    pub(crate) fn is_stale(&self) -> bool {
        self.force_stale || self.current.load(Ordering::Acquire) != self.started_at
    }
}

#[derive(Debug)]
struct SearchGenerationEntry {
    current: Arc<AtomicU64>,
    last_used: u64,
    latest_sequence: Option<u64>,
}

#[derive(Debug)]
struct SearchGenerationStore {
    entries: HashMap<String, SearchGenerationEntry>,
    next_access_order: u64,
}

impl SearchGenerationStore {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            next_access_order: 0,
        }
    }

    fn next_order(&mut self) -> u64 {
        self.next_access_order = self.next_access_order.wrapping_add(1);
        self.next_access_order
    }

    fn evict_for_new_client(&mut self) -> bool {
        let idle_key = self
            .entries
            .iter()
            .filter(|(_, entry)| Arc::strong_count(&entry.current) == 1)
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(client_id, _)| client_id.clone());
        if let Some(client_id) = idle_key {
            self.entries.remove(&client_id);
            return true;
        }

        tracing::warn!(
            "[markdown-view] 検索client世代の上限到達により新規clientの世代発行を拒否します"
        );
        false
    }
}

/// サーバー共有状態
#[derive(Debug)]
pub struct AppState {
    mode: AppMode,
    dark_mode: bool,
    syntax_css: String,
    tx: broadcast::Sender<BroadcastMessage>,
    memo_fs: Arc<dyn MemoFs>,
    search_generations: Arc<Mutex<SearchGenerationStore>>,
    directory_search_permits: Arc<Semaphore>,
}

impl AppState {
    /// 本番用のメモファイルシステムで`AppState`を生成する
    pub fn new_with_tokio_memo_fs(
        mode: AppMode,
        dark_mode: bool,
        theme: Option<String>,
        tx: broadcast::Sender<BroadcastMessage>,
    ) -> Self {
        Self::new(mode, dark_mode, theme, tx, Arc::new(TokioMemoFs))
    }

    /// メモファイルシステムを注入して`AppState`を生成する
    pub(crate) fn new(
        mode: AppMode,
        dark_mode: bool,
        theme: Option<String>,
        tx: broadcast::Sender<BroadcastMessage>,
        memo_fs: Arc<dyn MemoFs>,
    ) -> Self {
        Self {
            syntax_css: syntax_theme_css(theme.as_deref()),
            mode,
            dark_mode,
            tx,
            memo_fs,
            search_generations: Arc::new(Mutex::new(SearchGenerationStore::new())),
            directory_search_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_DIRECTORY_SEARCHES)),
        }
    }

    /// 動作モードを返す
    pub fn mode(&self) -> &AppMode {
        &self.mode
    }

    /// ダークモード設定を返す
    pub fn dark_mode(&self) -> bool {
        self.dark_mode
    }

    /// 構文ハイライト用CSSを返す
    pub fn syntax_css(&self) -> &str {
        &self.syntax_css
    }

    /// broadcast送信チャネルを返す
    pub fn tx(&self) -> &broadcast::Sender<BroadcastMessage> {
        &self.tx
    }

    /// メモ保存・読み込みで使用するファイルシステム抽象を返す
    pub(crate) fn memo_fs(&self) -> &Arc<dyn MemoFs> {
        &self.memo_fs
    }

    fn lock_search_generations(&self) -> MutexGuard<'_, SearchGenerationStore> {
        self.search_generations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// ディレクトリ検索の同時実行数を制限するpermitを取得する。
    pub(crate) fn try_acquire_directory_search_permit(
        &self,
    ) -> Result<OwnedSemaphorePermit, SearchConcurrencyLimitError> {
        Arc::clone(&self.directory_search_permits)
            .try_acquire_owned()
            .map_err(|_| SearchConcurrencyLimitError)
    }

    /// 指定クライアントのディレクトリ検索用に新しい世代を発行する。
    pub(crate) fn begin_search_generation(
        &self,
        client_id: &str,
        sequence: Option<u64>,
    ) -> Result<SearchGeneration, SearchGenerationLimitError> {
        let mut store = self.lock_search_generations();
        if !store.entries.contains_key(client_id)
            && store.entries.len() >= MAX_SEARCH_GENERATION_CLIENTS
            && !store.evict_for_new_client()
        {
            return Err(SearchGenerationLimitError);
        }

        let last_used = store.next_order();
        let current = {
            let entry = store
                .entries
                .entry(client_id.to_string())
                .or_insert_with(|| SearchGenerationEntry {
                    current: Arc::new(AtomicU64::new(0)),
                    last_used,
                    latest_sequence: None,
                });
            match (entry.latest_sequence, sequence) {
                (Some(latest), Some(sequence)) if sequence <= latest => {
                    return Ok(SearchGeneration::stale(Arc::clone(&entry.current)));
                }
                (Some(_), None) => {
                    return Ok(SearchGeneration::stale(Arc::clone(&entry.current)));
                }
                (_, Some(sequence)) => entry.latest_sequence = Some(sequence),
                (None, None) => {}
            }
            entry.last_used = last_used;
            Arc::clone(&entry.current)
        };
        let generation = current.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        Ok(SearchGeneration::new(generation, current))
    }

    /// 既存クライアントの検索世代だけを進める。
    pub(crate) fn advance_existing_search_generation(&self, client_id: Option<&str>) -> bool {
        let Some(client_id) = client_id else {
            return false;
        };

        let mut store = self.lock_search_generations();
        let last_used = store.next_order();
        let Some(entry) = store.entries.get_mut(client_id) else {
            return false;
        };

        entry.last_used = last_used;
        entry.current.fetch_add(1, Ordering::AcqRel);
        true
    }

    /// 指定クライアントの現在のディレクトリ検索世代を返す。
    #[cfg(test)]
    pub(crate) fn current_search_generation(&self, client_id: &str) -> u64 {
        self.lock_search_generations()
            .entries
            .get(client_id)
            .map(|entry| entry.current.load(Ordering::Acquire))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};

    use super::*;

    #[test]
    fn test_app_mode_new_single_file() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");

        let mode = AppMode::new_single_file(&file_path).unwrap();
        let canonical = file_path.canonicalize().unwrap();

        assert_eq!(mode.base_dir(), canonical.parent().unwrap());
        assert_eq!(mode.single_file(), Some(canonical.as_path()));
        assert!(mode.single_file_canonical().is_some());
        assert!(mode.directory().is_none());
        assert!(mode.directory_canonical().is_none());
    }

    #[test]
    fn test_app_mode_new_directory() {
        let dir = tempfile::tempdir().unwrap();

        let mode = AppMode::new_directory(dir.path()).unwrap();
        let canonical = dir.path().canonicalize().unwrap();

        assert_eq!(mode.base_dir(), canonical.as_path());
        assert!(mode.single_file().is_none());
        assert!(mode.single_file_canonical().is_none());
        assert_eq!(mode.directory(), Some(canonical.as_path()));
        assert!(mode.directory_canonical().is_some());
    }

    #[test]
    fn test_app_mode_relative_path_of_ディレクトリモード() {
        let dir = create_test_dir();
        let canonical = dir.path().canonicalize().unwrap();
        let mode = AppMode::new_directory(dir.path()).unwrap();
        let file_path = canonical.join("docs/api.md");
        assert_eq!(
            mode.relative_path_of(&file_path),
            Some("docs/api.md".to_string())
        );
    }

    #[test]
    fn test_app_mode_relative_path_of_単一ファイルモードはnone() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");

        let mode = AppMode::new_single_file(&file_path).unwrap();
        let canonical = file_path.canonicalize().unwrap();
        assert_eq!(mode.relative_path_of(&canonical), None);
    }

    #[test]
    fn test_app_mode_new_directory_存在しないパスは拒否() {
        let result = AppMode::new_directory("/nonexistent/path/that/does/not/exist");
        assert!(matches!(result, Err(AppModeBuildError::CanonicalPath(_))));
    }

    #[test]
    fn test_app_mode_new_single_file_非mdは拒否() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "test").unwrap();

        let result = AppMode::new_single_file(&file_path);
        assert!(matches!(result, Err(AppModeBuildError::NotMarkdown(_))));
    }

    #[test]
    fn test_app_mode_new_single_file_ディレクトリ指定は拒否() {
        let dir = tempfile::tempdir().unwrap();
        let result = AppMode::new_single_file(dir.path());
        assert!(matches!(result, Err(AppModeBuildError::NotFile(_))));
    }

    #[test]
    fn test_app_mode_new_directory_ファイル指定は拒否() {
        let (_dir, file_path) = create_markdown_fixture("note.md", "# note");
        let result = AppMode::new_directory(&file_path);
        assert!(matches!(result, Err(AppModeBuildError::NotDirectory(_))));
    }

    #[test]
    fn test_ensure_canonical_file_metadata失敗はnotfileへ集約する() {
        let (_dir, file_path) = create_markdown_fixture("vanish.md", "# vanish");
        let canonical = file_path.canonicalize().unwrap();
        let canonical_path = CanonicalPath::try_from_path(&canonical).unwrap();
        std::fs::remove_file(&file_path).unwrap();

        let result = ensure_canonical_file(&canonical_path);

        assert!(matches!(
            result,
            Err(AppModeBuildError::NotFile(path)) if path == canonical
        ));
    }

    #[test]
    fn test_ensure_canonical_directory_metadata失敗はnotdirectoryへ集約する() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let canonical_path = CanonicalPath::try_from_path(&canonical).unwrap();
        std::fs::remove_dir(dir.path()).unwrap();

        let result = ensure_canonical_directory(&canonical_path);

        assert!(matches!(
            result,
            Err(AppModeBuildError::NotDirectory(path)) if path == canonical
        ));
    }

    #[test]
    fn test_canonical_path_unknown_identityは明示的にunsupportedを返す() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_path = CanonicalPath::unknown_identity_for_test(dir.path());

        let error = canonical_path
            .has_current_identity()
            .expect_err("identity未取得状態は明示的なunsupportedとして扱う");

        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    }

    #[test]
    fn test_app_state_newはmemo_fsを生成時注入する() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let memo_fs: Arc<dyn MemoFs> = Arc::new(TokioMemoFs);

        let state = AppState::new(mode, false, None, tx, Arc::clone(&memo_fs));

        assert!(Arc::ptr_eq(&memo_fs, state.memo_fs()));
    }

    #[test]
    fn test_app_state_new_with_tokio_memo_fsは本番用memo_fsを組み込む() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);

        let state =
            AppState::new_with_tokio_memo_fs(mode, true, Some("base16-ocean.dark".to_string()), tx);

        assert!(state.dark_mode());
        assert!(!state.syntax_css().is_empty());
        assert_eq!(state.tx().receiver_count(), 1);
    }

    #[test]
    fn test_app_state_search_generationは初期値0() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);

        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        assert_eq!(state.current_search_generation("client-a"), 0);
    }

    #[test]
    fn test_begin_search_generationは世代を進めてhandleを返す() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        let first = state.begin_search_generation("client-a", None).unwrap();
        let second = state.begin_search_generation("client-a", None).unwrap();

        assert_eq!(first.started_at(), 1);
        assert_eq!(second.started_at(), 2);
        assert!(first.is_stale());
        assert!(!second.is_stale());
        assert_eq!(state.current_search_generation("client-a"), 2);
    }

    #[test]
    fn test_begin_search_generationはclientごとに独立する() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        let first_a = state.begin_search_generation("client-a", None).unwrap();
        let first_b = state.begin_search_generation("client-b", None).unwrap();
        let second_a = state.begin_search_generation("client-a", None).unwrap();

        assert!(first_a.is_stale());
        assert!(!first_b.is_stale());
        assert!(!second_a.is_stale());
        assert_eq!(state.current_search_generation("client-a"), 2);
        assert_eq!(state.current_search_generation("client-b"), 1);
    }

    #[test]
    fn test_begin_search_generationは古いsequenceでは世代を進めない() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        let current = state.begin_search_generation("client-a", Some(2)).unwrap();
        let stale = state.begin_search_generation("client-a", Some(1)).unwrap();

        assert!(!current.is_stale());
        assert!(stale.is_stale());
        assert_eq!(state.current_search_generation("client-a"), 1);
    }

    #[test]
    fn test_begin_search_generationのstale_handleは後続世代と衝突してもstaleのまま() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        let current = state.begin_search_generation("client-a", Some(2)).unwrap();
        let stale = state.begin_search_generation("client-a", Some(1)).unwrap();
        let next = state.begin_search_generation("client-a", Some(3)).unwrap();

        assert!(current.is_stale());
        assert!(stale.is_stale());
        assert!(!next.is_stale());
        assert_eq!(state.current_search_generation("client-a"), 2);
    }

    #[test]
    fn test_search_generation_force_staleはcurrent値と一致してもstaleのまま() {
        let current = Arc::new(AtomicU64::new(1));
        let stale = SearchGeneration {
            started_at: 2,
            current: Arc::clone(&current),
            force_stale: true,
        };

        current.store(2, Ordering::Release);

        assert!(stale.is_stale());
    }

    #[test]
    fn test_begin_search_generationはsequenced_clientのsequenceなしrequestで世代を進めない() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        let current = state.begin_search_generation("client-a", Some(1)).unwrap();
        let stale = state.begin_search_generation("client-a", None).unwrap();

        assert!(!current.is_stale());
        assert!(stale.is_stale());
        assert_eq!(state.current_search_generation("client-a"), 1);
    }

    #[test]
    fn test_begin_search_generationは上限到達時に最古idle_clientを入れ替える() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        let active = state.begin_search_generation("client-0", None).unwrap();
        for index in 1..MAX_SEARCH_GENERATION_CLIENTS {
            state
                .begin_search_generation(&format!("client-{index}"), None)
                .unwrap();
        }

        let overflow = state
            .begin_search_generation("overflow-client", None)
            .unwrap();

        assert_eq!(overflow.started_at(), 1);
        assert!(!overflow.is_stale());
        assert_eq!(state.current_search_generation("overflow-client"), 1);
        assert_eq!(state.current_search_generation("client-1"), 0);
        assert!(!active.is_stale());
    }

    #[test]
    fn test_begin_search_generationは全client_activeなら新規clientを拒否する() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);
        let active_handles = (0..MAX_SEARCH_GENERATION_CLIENTS)
            .map(|index| {
                state
                    .begin_search_generation(&format!("client-{index}"), None)
                    .unwrap()
            })
            .collect::<Vec<_>>();

        let overflow = state.begin_search_generation("overflow-client", None);

        assert!(matches!(overflow, Err(SearchGenerationLimitError)));
        assert_eq!(state.current_search_generation("overflow-client"), 0);
        assert_eq!(state.current_search_generation("client-0"), 1);
        assert!(!active_handles[0].is_stale());
    }

    #[test]
    fn test_begin_search_generationは同一clientの並行発行でも世代が重複しない() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = Arc::new(AppState::new_with_tokio_memo_fs(mode, false, None, tx));
        let worker_count = 16;
        let barrier = Arc::new(Barrier::new(worker_count));

        let handles = (0..worker_count)
            .map(|_| {
                let state = Arc::clone(&state);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    state.begin_search_generation("client-a", None).unwrap()
                })
            })
            .collect::<Vec<_>>();
        let mut generations = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        generations.sort_by_key(SearchGeneration::started_at);

        let started_at = generations
            .iter()
            .map(SearchGeneration::started_at)
            .collect::<Vec<_>>();
        assert_eq!(started_at, (1..=worker_count as u64).collect::<Vec<_>>());
        assert!(generations[..worker_count - 1]
            .iter()
            .all(SearchGeneration::is_stale));
        assert!(!generations[worker_count - 1].is_stale());
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn create_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/api.md"), "# API").unwrap();
        dir
    }
}
