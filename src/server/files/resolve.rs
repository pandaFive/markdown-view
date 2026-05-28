use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::catalog::{
    list_markdown_files_from_canonical_base, open_relative_file_nofollow, open_verified_base_dir,
};
use super::run_blocking_file_task;
use crate::server::guards::json_error;
use crate::server::log_path::sanitize_path_for_logging;
use crate::server::messages::ApiError;
use crate::server::state::{AppState, CanonicalPath};
use crate::template::UpdateMessage;
use crate::workspace_exclusion::exclusion_reason_for_relative_path;

#[derive(Debug, Clone)]
/// ファイル解決結果。ターゲットファイルのパス、ファイル一覧、相対パス、表示用ラベルを保持する。
pub(in crate::server) struct ResolvedTarget {
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    relative_path: Option<String>,
    file_label: String,
    read_file: Option<std::sync::Arc<std::fs::File>>,
}

impl ResolvedTarget {
    fn new(
        file_path: PathBuf,
        file_list: Option<Vec<String>>,
        relative_path: Option<String>,
        read_file: Option<std::fs::File>,
    ) -> Self {
        let file_label = relative_path
            .clone()
            .unwrap_or_else(|| file_display_name(&file_path));

        Self {
            file_path,
            file_list,
            relative_path,
            file_label,
            read_file: read_file.map(std::sync::Arc::new),
        }
    }

    #[cfg(test)]
    pub(in crate::server) fn for_test(
        file_path: PathBuf,
        file_list: Option<Vec<String>>,
        relative_path: Option<String>,
    ) -> Self {
        Self::new(file_path, file_list, relative_path, None)
    }

    pub(in crate::server) fn file_path(&self) -> &Path {
        &self.file_path
    }

    pub(in crate::server) fn file_list(&self) -> Option<&[String]> {
        self.file_list.as_deref()
    }

    pub(in crate::server) fn relative_path(&self) -> Option<&str> {
        self.relative_path.as_deref()
    }

    pub(in crate::server) fn file_label(&self) -> &str {
        &self.file_label
    }

    pub(in crate::server) fn read_file(&self) -> std::io::Result<Option<std::fs::File>> {
        self.read_file
            .as_ref()
            .map(|file| file.try_clone())
            .transpose()
    }

    pub(super) fn attach_file_info(&self, update: UpdateMessage) -> UpdateMessage {
        update.with_file(self.relative_path.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::server) struct RouteTargetRequest<'a> {
    kind: RouteTargetKind,
    query_file: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::server) enum RouteTargetKind {
    Page,
    ApiContent,
    ApiMemo,
}

impl<'a> RouteTargetRequest<'a> {
    pub(in crate::server) fn page(query_file: Option<&'a str>) -> Self {
        Self {
            kind: RouteTargetKind::Page,
            query_file,
        }
    }

    pub(in crate::server) fn api_content(query_file: Option<&'a str>) -> Self {
        Self {
            kind: RouteTargetKind::ApiContent,
            query_file,
        }
    }

    pub(in crate::server) fn api_memo(query_file: Option<&'a str>) -> Self {
        Self {
            kind: RouteTargetKind::ApiMemo,
            query_file,
        }
    }

    pub(in crate::server) fn query_file(self) -> Option<&'a str> {
        self.query_file
    }

    #[cfg(test)]
    pub(in crate::server) fn kind(self) -> RouteTargetKind {
        self.kind
    }

    fn include_file_list(self) -> bool {
        match self.kind {
            RouteTargetKind::Page => true,
            RouteTargetKind::ApiContent | RouteTargetKind::ApiMemo => false,
        }
    }

    fn not_found_message(self) -> &'static str {
        match self.kind {
            RouteTargetKind::Page => "表示可能なMarkdownファイルが見つかりません",
            RouteTargetKind::ApiContent | RouteTargetKind::ApiMemo => {
                "指定したファイルが見つかりません"
            }
        }
    }

    pub(in crate::server) fn read_error_log_label(self) -> &'static str {
        match self.kind {
            RouteTargetKind::Page => "index",
            RouteTargetKind::ApiContent => "api/content",
            RouteTargetKind::ApiMemo => "api/memo",
        }
    }
}

/// 対象ファイル解決エラーをエンドポイント文脈に応じたAPIエラーへ変換する。
pub(in crate::server) async fn resolve_route_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<ResolvedTarget, ApiError> {
    let (file_path, file_list, read_file) =
        resolve_request_target(state, request)
            .await
            .map_err(|status| {
                let message = match status {
                    StatusCode::NOT_FOUND => request.not_found_message(),
                    StatusCode::INTERNAL_SERVER_ERROR => "ファイル一覧の取得に失敗しました",
                    other => {
                        tracing::warn!(
                            "[markdown-view] 予期しないファイル解決ステータスを検出: request={:?}, status={}",
                            request,
                            other
                        );
                        "ファイル解決に失敗しました"
                    }
                };
                json_error(status, message)
            })?;

    Ok(build_resolved_target(
        state,
        file_path,
        file_list,
        read_file,
        "ターゲットファイルの相対パス算出失敗",
    ))
}

pub(super) fn resolve_single_file_target(
    state: &AppState,
    warn_label: &'static str,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    let Some(file_path) = state.mode().single_file() else {
        return Ok(None);
    };

    let validated_path = revalidate_single_file_target(file_path, state.mode().base_dir())?;
    let read_file = open_single_file_for_read(file_path)?;
    Ok(Some(build_resolved_target(
        state,
        validated_path,
        None,
        Some(read_file),
        warn_label,
    )))
}

pub(super) fn resolve_change_target(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    if let Some(expected) = state.mode().single_file() {
        let validated_path = revalidate_single_file_target(expected, state.mode().base_dir())?;
        let read_file = open_single_file_for_read(expected)?;
        return Ok(Some(build_resolved_target(
            state,
            validated_path,
            None,
            Some(read_file),
            "更新対象の相対パス算出失敗",
        )));
    }

    resolve_directory_change_target(state, changed_file)
}

async fn resolve_request_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<(PathBuf, Option<Vec<String>>, Option<std::fs::File>), StatusCode> {
    if let Some(path) = state.mode().single_file() {
        let canonical =
            revalidate_single_file_target(path, state.mode().base_dir()).map_err(|error| {
                tracing::warn!("[markdown-view] 単一ファイル解決エラー: {}", error);
                error.status_code()
            })?;
        let read_file = open_single_file_for_read(path).map_err(|error| {
            tracing::warn!("[markdown-view] 単一ファイルopen検証エラー: {}", error);
            error.status_code()
        })?;
        return Ok((canonical, None, Some(read_file)));
    }

    let Some(base_dir) = state.mode().directory_canonical() else {
        tracing::error!("[markdown-view] 未知のAppModeです");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let mut precomputed_files = None;
    let (file_path, read_file) = if let Some(relative) = request.query_file() {
        let resolved = resolve_file_blocking(base_dir, relative)
            .await?
            .map_err(|error| {
                tracing::warn!("[markdown-view] ファイル解決エラー: {}", error);
                error.status_code()
            })?;
        (resolved.path, Some(resolved.file))
    } else {
        let files = list_markdown_files_blocking(base_dir).await?;
        precomputed_files = Some(files.clone());

        let default_file = files
            .iter()
            .find(|file| file.eq_ignore_ascii_case("readme.md"))
            .or_else(|| files.first());

        match default_file {
            Some(relative) => {
                let resolved =
                    resolve_file_blocking(base_dir, relative)
                        .await?
                        .map_err(|error| {
                            tracing::warn!(
                                "[markdown-view] デフォルトファイル解決エラー: {}",
                                error
                            );
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                (resolved.path, Some(resolved.file))
            }
            None => return Err(StatusCode::NOT_FOUND),
        }
    };

    let file_list = if request.include_file_list() {
        match precomputed_files {
            Some(files) => Some(files),
            None => Some(list_markdown_files_blocking(base_dir).await?),
        }
    } else {
        None
    };

    Ok((file_path, file_list, read_file))
}

async fn list_markdown_files_blocking(base_dir: &CanonicalPath) -> Result<Vec<String>, StatusCode> {
    let base_dir = base_dir.clone();
    run_blocking_file_task("ファイル一覧取得", move || {
        list_markdown_files_from_canonical_base(&base_dir, super::catalog::MAX_FILE_LIST)
    })
    .await?
    .map_err(|error| {
        tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", error);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

async fn resolve_file_blocking(
    base_dir: &CanonicalPath,
    relative: &str,
) -> Result<Result<ResolvedFileHandle, ResolveFileError>, StatusCode> {
    let base_dir = base_dir.clone();
    let relative = relative.to_owned();
    run_blocking_file_task("ファイル解決", move || {
        resolve_file_from_canonical_base(&base_dir, &relative)
    })
    .await
}

#[derive(Debug)]
struct ResolvedFileHandle {
    path: PathBuf,
    file: std::fs::File,
}

fn resolve_file_from_canonical_base(
    base_dir: &CanonicalPath,
    relative: &str,
) -> Result<ResolvedFileHandle, ResolveFileError> {
    validate_directory_base_identity_for_resolve(base_dir)?;
    let path = resolve_file(base_dir.as_path(), relative)?;
    let relative_path = path
        .strip_prefix(base_dir.as_path())
        .map_err(|_| ResolveFileError::Traversal)?;
    let verified_base = open_verified_base_dir(base_dir, "ファイル解決base directory")
        .map_err(|error| ResolveFileError::Io(error.kind()))?;
    let file = match open_relative_file_nofollow(&verified_base, relative_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    if !metadata.is_file() {
        return Err(ResolveFileError::NotFile);
    }
    Ok(ResolvedFileHandle {
        path,
        file: file.into_std(),
    })
}

fn open_single_file_for_read(path: &Path) -> Result<std::fs::File, ResolveFileError> {
    let parent = path.parent().ok_or(ResolveFileError::InvalidPath)?;
    let file_name = path.file_name().ok_or(ResolveFileError::InvalidPath)?;
    let parent_dir = cap_std::fs::Dir::open_ambient_dir(parent, cap_std::ambient_authority())
        .map_err(|error| ResolveFileError::Io(error.kind()))?;
    let file = match open_relative_file_nofollow(&parent_dir, Path::new(file_name)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    if !metadata.is_file() {
        return Err(ResolveFileError::NotFile);
    }
    Ok(file.into_std())
}

fn validate_directory_base_identity_for_resolve(
    base_dir: &CanonicalPath,
) -> Result<(), ResolveFileError> {
    match base_dir.has_current_identity() {
        Ok(true) => Ok(()),
        Ok(false) => {
            tracing::warn!(
                "[markdown-view] ファイル解決base directory pathの実体差し替えを検出しました"
            );
            Err(ResolveFileError::Traversal)
        }
        Err(error) => {
            tracing::warn!(
                "[markdown-view] ファイル解決base directory pathの実体検証に失敗しました: {}",
                error
            );
            if error.kind() == std::io::ErrorKind::NotFound {
                Err(ResolveFileError::NotFound)
            } else {
                Err(ResolveFileError::Io(error.kind()))
            }
        }
    }
}

fn build_resolved_target(
    state: &AppState,
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    read_file: Option<std::fs::File>,
    warn_label: &'static str,
) -> ResolvedTarget {
    let relative_path = state.mode().relative_path_of(&file_path);
    if state.mode().is_directory() && relative_path.is_none() {
        tracing::warn!(
            "[markdown-view] {}: {} はベース {} の配下ではありません",
            warn_label,
            sanitize_path_for_logging(&file_path, state.mode().base_dir()),
            state.mode().base_dir().display()
        );
        // 相対パス算出失敗時の方針（呼び出し経路ごとに後段で扱いを変える）:
        // - 本関数は警告ログのみで描画継続を許容する（graceful degradation）
        // - HTTP経路: サイドバーのハイライトが落ちるだけで本文描画は継続
        // - WebSocket変更通知経路: 変更ターゲット解決時の再検証後にここへ到達したら内部不整合
    }

    ResolvedTarget::new(file_path, file_list, relative_path, read_file)
}

fn resolve_directory_change_target(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    let Some(base_dir) = state.mode().directory_canonical() else {
        tracing::error!("[markdown-view] 未知のAppModeです");
        return Err(ResolveFileError::InternalState);
    };

    let relative = relative_change_path(base_dir.as_path(), changed_file)?;
    let relative_string = relative_change_path_to_query(&relative)?;
    let resolved = resolve_file_from_canonical_base(base_dir, &relative_string)?;
    Ok(Some(build_resolved_target(
        state,
        resolved.path,
        None,
        Some(resolved.file),
        "更新対象の相対パス算出失敗",
    )))
}

fn relative_change_path_to_query(relative: &Path) -> Result<String, ResolveFileError> {
    relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or(ResolveFileError::InvalidPath)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

fn relative_change_path(base_dir: &Path, changed_file: &Path) -> Result<PathBuf, ResolveFileError> {
    if let Ok(relative) = changed_file.strip_prefix(base_dir) {
        return Ok(relative.to_path_buf());
    }

    let canonical_base = base_dir.canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] watcher変更ターゲット: ベース正規化失敗: {} ({})",
            base_dir.display(),
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    let canonical_changed = changed_file.canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] watcher変更ターゲット: パス正規化失敗: {} ({})",
            sanitize_path_for_logging(changed_file, base_dir),
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    canonical_changed
        .strip_prefix(&canonical_base)
        .map(Path::to_path_buf)
        .map_err(|_| ResolveFileError::Traversal)
}

fn resolve_canonicalize_error(error_kind: std::io::ErrorKind) -> ResolveFileError {
    if error_kind == std::io::ErrorKind::NotFound {
        ResolveFileError::NotFound
    } else {
        ResolveFileError::Io(error_kind)
    }
}

/// 相対パスを安全に解決する（ディレクトリトラバーサル防止）
pub fn resolve_file(base_dir: &Path, relative: &str) -> Result<PathBuf, ResolveFileError> {
    resolve_file_with_canonicalize_error(base_dir, relative, resolve_canonicalize_error)
}

fn resolve_file_with_canonicalize_error(
    base_dir: &Path,
    relative: &str,
    map_canonicalize_error: fn(std::io::ErrorKind) -> ResolveFileError,
) -> Result<PathBuf, ResolveFileError> {
    if relative.is_empty() {
        return Err(ResolveFileError::EmptyPath);
    }
    if relative.contains('\0') {
        return Err(ResolveFileError::InvalidPath);
    }

    let relative_path = Path::new(relative);
    if relative_path.is_absolute() {
        return Err(ResolveFileError::InvalidPath);
    }
    if exclusion_reason_for_relative_path(relative_path).is_some() {
        return Err(ResolveFileError::Hidden);
    }

    let candidate = base_dir.join(relative_path);
    let canonical = candidate.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ファイルパス正規化失敗: {} ({})",
            sanitize_path_for_logging(&candidate, base_dir),
            error
        );
        map_canonicalize_error(error.kind())
    })?;

    let canonical_base = base_dir.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ベースディレクトリ正規化失敗: {} ({})",
            base_dir.display(),
            error
        );
        map_canonicalize_error(error.kind())
    })?;
    if !canonical.starts_with(&canonical_base) {
        return Err(ResolveFileError::Traversal);
    }

    if let Ok(resolved_relative) = canonical.strip_prefix(&canonical_base) {
        if exclusion_reason_for_relative_path(resolved_relative).is_some() {
            return Err(ResolveFileError::Hidden);
        }
    }

    if !canonical.is_file() {
        return Err(ResolveFileError::NotFile);
    }

    match canonical.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => Ok(canonical),
        _ => Err(ResolveFileError::NotMarkdown),
    }
}

/// 単一ファイルモードの対象ファイルを安全に再検証する
///
/// 起動時に正規化したパスと現在のパスを比較し、シンボリックリンク差し替え等の
/// 攻撃を検出する。正規化後のパスが起動時と異なる場合はトラバーサルとして拒否する。
pub(super) fn revalidate_single_file_target(
    expected_path: &Path,
    base_dir: &Path,
) -> Result<PathBuf, ResolveFileError> {
    let canonical = expected_path.canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] 単一ファイルパス正規化失敗: {} ({:?}: {})",
            sanitize_path_for_logging(expected_path, base_dir),
            error_kind,
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    if canonical != expected_path {
        return Err(ResolveFileError::Traversal);
    }
    if !canonical.is_file() {
        return Err(ResolveFileError::NotFile);
    }

    match canonical.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => Ok(canonical),
        _ => Err(ResolveFileError::NotMarkdown),
    }
}

#[derive(Debug, PartialEq)]
pub enum ResolveFileError {
    /// 空パス
    EmptyPath,
    /// 無効なパス（絶対パス、NULバイト等）
    InvalidPath,
    /// ファイルが見つからない
    NotFound,
    /// 通常ファイルではない
    NotFile,
    /// ディレクトリトラバーサル検出
    Traversal,
    /// Markdownファイルではない
    NotMarkdown,
    /// 隠しファイルまたは除外対象へのアクセス
    Hidden,
    /// watcher再検証中の一時不在ではないI/O失敗
    Io(std::io::ErrorKind),
    /// AppModeの内部不整合
    InternalState,
}

impl std::fmt::Display for ResolveFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveFileError::EmptyPath => write!(f, "ファイルパスが空です"),
            ResolveFileError::InvalidPath => write!(f, "無効なパスです"),
            ResolveFileError::NotFound => write!(f, "ファイルが見つかりません"),
            ResolveFileError::NotFile => write!(f, "通常ファイルではありません"),
            ResolveFileError::Traversal => {
                write!(f, "ディレクトリ外へのアクセスは禁止されています")
            }
            ResolveFileError::NotMarkdown => write!(f, ".mdファイルのみアクセス可能です"),
            ResolveFileError::Hidden => write!(
                f,
                "隠しファイルまたは除外対象へのアクセスは禁止されています"
            ),
            ResolveFileError::Io(kind) => {
                write!(f, "ファイル解決中にI/Oエラーが発生しました ({kind:?})")
            }
            ResolveFileError::InternalState => write!(f, "内部状態が不整合です"),
        }
    }
}

impl std::error::Error for ResolveFileError {}

impl ResolveFileError {
    /// エラー種別に関わらず404を返す
    ///
    /// エラー種別で応答を分けるとファイル存在有無の推測材料になるため、
    /// すべて404に統一してセキュリティを確保する。
    pub fn status_code(&self) -> StatusCode {
        StatusCode::NOT_FOUND
    }
}

/// パスから表示用ラベルを生成する（`file_name` があればそれ、なければ `display()` フォールバック）
pub(super) fn file_display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
