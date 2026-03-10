use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::super::guards::json_error;
use super::super::messages::ApiError;
use super::super::state::AppState;
use super::catalog::list_markdown_files;
use crate::template::UpdateMessage;

#[derive(Debug, Clone)]
/// ファイル解決結果。ターゲットファイルのパス、ファイル一覧、相対パス、表示用ラベルを保持する。
pub(in crate::server) struct ResolvedTarget {
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    relative_path: Option<String>,
    file_label: String,
}

impl ResolvedTarget {
    fn new(
        file_path: PathBuf,
        file_list: Option<Vec<String>>,
        relative_path: Option<String>,
    ) -> Self {
        let file_label = relative_path
            .clone()
            .or_else(|| {
                file_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| file_path.display().to_string());

        Self {
            file_path,
            file_list,
            relative_path,
            file_label,
        }
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

    pub(super) fn file_label(&self) -> &str {
        &self.file_label
    }

    pub(super) fn update(&self, update: UpdateMessage) -> UpdateMessage {
        update.with_file(self.relative_path.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::server) enum RouteTargetRequest<'a> {
    Page { query_file: Option<&'a str> },
    ApiContent { query_file: Option<&'a str> },
}

impl<'a> RouteTargetRequest<'a> {
    pub(in crate::server) fn page(query_file: Option<&'a str>) -> Self {
        Self::Page { query_file }
    }

    pub(in crate::server) fn api_content(query_file: Option<&'a str>) -> Self {
        Self::ApiContent { query_file }
    }

    fn query_file(self) -> Option<&'a str> {
        match self {
            Self::Page { query_file } | Self::ApiContent { query_file } => query_file,
        }
    }

    fn include_file_list(self) -> bool {
        matches!(self, Self::Page { .. })
    }

    fn not_found_message(self) -> &'static str {
        match self {
            Self::Page { .. } => "表示可能なMarkdownファイルが見つかりません",
            Self::ApiContent { .. } => "指定したファイルが見つかりません",
        }
    }

    pub(in crate::server) fn read_error_log_label(self) -> &'static str {
        match self {
            Self::Page { .. } => "index",
            Self::ApiContent { .. } => "api/content",
        }
    }
}

/// 対象ファイル解決エラーをエンドポイント文脈に応じたAPIエラーへ変換する。
pub(in crate::server) fn resolve_route_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<ResolvedTarget, ApiError> {
    let (file_path, file_list) = resolve_request_target(state, request).map_err(|status| {
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

    let validated_path = revalidate_single_file_target(file_path)?;
    Ok(Some(build_resolved_target(
        state,
        validated_path,
        None,
        warn_label,
    )))
}

pub(super) fn resolve_change_target(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    if let Some(expected) = state.mode().single_file() {
        revalidate_single_file_target(expected)?;
        Ok(Some(build_resolved_target(
            state,
            changed_file.to_path_buf(),
            None,
            "更新対象の相対パス算出失敗",
        )))
    } else {
        Ok(build_update_target(state, changed_file))
    }
}

fn resolve_request_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<(PathBuf, Option<Vec<String>>), StatusCode> {
    if let Some(path) = state.mode().single_file() {
        let canonical = revalidate_single_file_target(path).map_err(|error| {
            tracing::warn!("[markdown-view] 単一ファイル解決エラー: {}", error);
            error.status_code()
        })?;
        return Ok((canonical, None));
    }

    let Some(base_dir) = state.mode().directory() else {
        tracing::error!("[markdown-view] 未知のAppModeです");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let mut precomputed_files = None;
    let file_path = if let Some(relative) = request.query_file() {
        resolve_file(base_dir, relative).map_err(|error| {
            tracing::warn!("[markdown-view] ファイル解決エラー: {}", error);
            error.status_code()
        })?
    } else {
        let files = list_markdown_files(base_dir).map_err(|error| {
            tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", error);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        precomputed_files = Some(files.clone());

        let default_file = files
            .iter()
            .find(|file| file.eq_ignore_ascii_case("readme.md"))
            .or_else(|| files.first());

        match default_file {
            Some(relative) => resolve_file(base_dir, relative).map_err(|error| {
                tracing::warn!("[markdown-view] デフォルトファイル解決エラー: {}", error);
                StatusCode::INTERNAL_SERVER_ERROR
            })?,
            None => return Err(StatusCode::NOT_FOUND),
        }
    };

    let file_list = if request.include_file_list() {
        match precomputed_files {
            Some(files) => Some(files),
            None => Some(list_markdown_files(base_dir).map_err(|error| {
                tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", error);
                StatusCode::INTERNAL_SERVER_ERROR
            })?),
        }
    } else {
        None
    };

    Ok((file_path, file_list))
}

fn build_resolved_target(
    state: &AppState,
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    warn_label: &'static str,
) -> ResolvedTarget {
    let relative_path = state.mode().relative_path_of(&file_path);
    if state.mode().is_directory() && relative_path.is_none() {
        tracing::warn!(
            "[markdown-view] {}: {} はベース {} の配下ではありません",
            warn_label,
            file_path.display(),
            state.mode().base_dir().display()
        );
        // HTTP経路ではサイドバーのハイライト低下に留め、描画継続を優先する。
    }

    ResolvedTarget::new(file_path, file_list, relative_path)
}

fn build_update_target(state: &AppState, changed_file: &Path) -> Option<ResolvedTarget> {
    let target = build_resolved_target(
        state,
        changed_file.to_path_buf(),
        None,
        "相対パス算出失敗のためブロードキャストをスキップ",
    );
    if state.mode().is_directory() && target.relative_path.is_none() {
        return None;
    }
    Some(target)
}

/// 相対パスを安全に解決する（ディレクトリトラバーサル防止）
pub fn resolve_file(base_dir: &Path, relative: &str) -> Result<PathBuf, ResolveFileError> {
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

    let candidate = base_dir.join(relative_path);
    let canonical = candidate.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ファイルパス正規化失敗: {} ({})",
            candidate.display(),
            error
        );
        ResolveFileError::NotFound
    })?;

    let canonical_base = base_dir.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ベースディレクトリ正規化失敗: {} ({})",
            base_dir.display(),
            error
        );
        ResolveFileError::NotFound
    })?;
    if !canonical.starts_with(&canonical_base) {
        return Err(ResolveFileError::Traversal);
    }

    if let Ok(resolved_relative) = canonical.strip_prefix(&canonical_base) {
        if resolved_relative
            .components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
        {
            return Err(ResolveFileError::Hidden);
        }
    }

    if !canonical.is_file() {
        return Err(ResolveFileError::NotFound);
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
) -> Result<PathBuf, ResolveFileError> {
    let canonical = expected_path.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] 単一ファイルパス正規化失敗: {} ({})",
            expected_path.display(),
            error
        );
        ResolveFileError::NotFound
    })?;

    if canonical != expected_path {
        return Err(ResolveFileError::Traversal);
    }
    if !canonical.is_file() {
        return Err(ResolveFileError::NotFound);
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
    /// ディレクトリトラバーサル検出
    Traversal,
    /// Markdownファイルではない
    NotMarkdown,
    /// 隠しファイルへのアクセス
    Hidden,
}

impl std::fmt::Display for ResolveFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveFileError::EmptyPath => write!(f, "ファイルパスが空です"),
            ResolveFileError::InvalidPath => write!(f, "無効なパスです"),
            ResolveFileError::NotFound => write!(f, "ファイルが見つかりません"),
            ResolveFileError::Traversal => {
                write!(f, "ディレクトリ外へのアクセスは禁止されています")
            }
            ResolveFileError::NotMarkdown => write!(f, ".mdファイルのみアクセス可能です"),
            ResolveFileError::Hidden => {
                write!(f, "隠しファイルへのアクセスは禁止されています")
            }
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
