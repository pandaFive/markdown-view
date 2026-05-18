use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::server::log_path::{
    sanitize_path_for_logging_escaped, sanitize_path_for_logging_lexical_escaped,
};
use crate::server::{CanonicalPath, CanonicalPathError};
use crate::workspace_exclusion::{exclusion_reason_for_name, exclusion_reason_for_relative_path};

/// ファイル一覧の最大件数
pub(in crate::server) const MAX_FILE_LIST: usize = 1000;

/// ディレクトリ走査の最大深度（スタックオーバーフロー防止）
pub(super) const MAX_DIR_DEPTH: usize = 32;

/// ディレクトリ内の.mdファイルを再帰的に列挙する
pub fn list_markdown_files(base_dir: &Path) -> std::io::Result<Vec<String>> {
    let canonical = CanonicalPath::try_from_path(base_dir).map_err(|error| match error {
        CanonicalPathError::Canonicalize(error) => error,
    })?;
    list_markdown_files_from_canonical_base(&canonical, MAX_FILE_LIST)
}

pub(in crate::server) fn list_markdown_files_from_canonical_base(
    base_dir: &CanonicalPath,
    max_files: usize,
) -> std::io::Result<Vec<String>> {
    list_markdown_files_from_canonical_base_with_cancellation(base_dir, max_files, &|| false)
}

pub(in crate::server) fn list_markdown_files_from_canonical_base_with_cancellation(
    base_dir: &CanonicalPath,
    max_files: usize,
    should_cancel: &dyn Fn() -> bool,
) -> std::io::Result<Vec<String>> {
    let base_path = base_dir.as_path();
    let mut files = Vec::new();
    let mut visited_dirs = HashSet::new();
    visited_dirs.insert(base_path.to_path_buf());
    let traversal = CatalogTraversal {
        log_base_dir: base_path,
        canonical_base_dir: base_path,
        max_files,
        should_cancel,
    };

    list_markdown_files_recursive(
        &traversal,
        base_path,
        base_path,
        &mut files,
        &mut visited_dirs,
        0,
    )?;
    files.sort();
    files.truncate(max_files);
    Ok(files)
}

struct CatalogTraversal<'a> {
    log_base_dir: &'a Path,
    canonical_base_dir: &'a Path,
    max_files: usize,
    should_cancel: &'a dyn Fn() -> bool,
}

fn list_markdown_files_recursive(
    traversal: &CatalogTraversal<'_>,
    current_dir: &Path,
    display_dir: &Path,
    files: &mut Vec<String>,
    visited_dirs: &mut HashSet<PathBuf>,
    depth: usize,
) -> std::io::Result<()> {
    if (traversal.should_cancel)() {
        return Ok(());
    }

    if depth >= MAX_DIR_DEPTH {
        tracing::warn!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            sanitize_path_for_logging_lexical_escaped(current_dir, traversal.log_base_dir)
        );
        return Ok(());
    }

    ensure_current_dir_still_canonical(
        current_dir,
        traversal.canonical_base_dir,
        traversal.log_base_dir,
    )?;

    let entries = std::fs::read_dir(current_dir)?;
    for entry in entries {
        if (traversal.should_cancel)() {
            return Ok(());
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ディレクトリエントリ読み取りエラー（スキップ）: {} ({})",
                    sanitize_path_for_logging_escaped(current_dir, traversal.log_base_dir),
                    error
                );
                continue;
            }
        };

        let name = entry.file_name();
        if exclusion_reason_for_name(&name).is_some() {
            continue;
        }

        let path = entry.path();
        let display_path = display_dir.join(&name);
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ファイルタイプ取得エラー（スキップ）: {} ({})",
                    sanitize_path_for_logging_escaped(&path, traversal.log_base_dir),
                    error
                );
                continue;
            }
        };

        if file_type.is_dir() || file_type.is_symlink() {
            if files.len() >= traversal.max_files || (traversal.should_cancel)() {
                return Ok(());
            }

            let Some(traversal_path) = resolve_recursable_directory(
                &path,
                file_type.is_symlink(),
                traversal.canonical_base_dir,
                visited_dirs,
                traversal.log_base_dir,
            )
            .map_err(|error| {
                tracing::warn!(
                    "[markdown-view] ディレクトリ再帰判定エラー: {} ({})",
                    sanitize_path_for_logging_escaped(&path, traversal.log_base_dir),
                    error
                );
                error
            })?
            else {
                continue;
            };

            list_markdown_files_recursive(
                traversal,
                &traversal_path,
                &display_path,
                files,
                visited_dirs,
                depth + 1,
            )?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            match display_path.strip_prefix(traversal.log_base_dir) {
                Ok(relative) => {
                    let relative_str = relative
                        .components()
                        .map(|component| component.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    files.push(relative_str);
                    if files.len() >= traversal.max_files {
                        return Ok(());
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 相対パス算出不可（スキップ）: {} (ベース: {})",
                        sanitize_path_for_logging_escaped(&path, traversal.log_base_dir),
                        traversal.log_base_dir.display()
                    );
                }
            }
        }
    }

    Ok(())
}

pub(super) fn ensure_current_dir_still_canonical(
    current_dir: &Path,
    canonical_base_dir: &Path,
    log_base_dir: &Path,
) -> std::io::Result<()> {
    let current_canonical = current_dir.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ディレクトリ再検証エラー: {} ({})",
            sanitize_path_for_logging_lexical_escaped(current_dir, log_base_dir),
            error
        );
        error
    })?;

    if current_canonical != current_dir || !current_canonical.starts_with(canonical_base_dir) {
        tracing::warn!(
            "[markdown-view] ディレクトリ再検証で正規化先の差し替えを検出: {} -> {}",
            sanitize_path_for_logging_lexical_escaped(current_dir, log_base_dir),
            sanitize_path_for_logging_escaped(&current_canonical, log_base_dir)
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "ディレクトリの正規化先が走査前検証時と一致しません",
        ));
    }

    Ok(())
}

pub(super) fn resolve_recursable_directory(
    path: &Path,
    is_symlink: bool,
    canonical_base_dir: &Path,
    visited_dirs: &mut HashSet<PathBuf>,
    log_base_dir: &Path,
) -> std::io::Result<Option<PathBuf>> {
    let label = if is_symlink {
        "シンボリックリンク"
    } else {
        "通常ディレクトリ"
    };
    let Some(resolved) = canonicalize_dir_for_cycle(path, label, log_base_dir) else {
        return Ok(None);
    };

    if !resolved.starts_with(canonical_base_dir) {
        if is_symlink {
            tracing::warn!(
                "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                sanitize_path_for_logging_escaped(&resolved, log_base_dir)
            );
            return Ok(None);
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "通常ディレクトリの正規化先がベースディレクトリ外です",
        ));
    }

    if is_symlink {
        if let Ok(relative) = resolved.strip_prefix(canonical_base_dir) {
            if exclusion_reason_for_relative_path(relative).is_some() {
                tracing::debug!(
                    "[markdown-view] シンボリックリンクが除外ディレクトリを指すためスキップ: {} -> {}",
                    sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                    sanitize_path_for_logging_escaped(&resolved, log_base_dir)
                );
                return Ok(None);
            }
        }
    }

    match std::fs::metadata(&resolved) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) if is_symlink => {
            tracing::debug!(
                "[markdown-view] シンボリックリンクが通常ファイルを指すためスキップ: {} -> {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                sanitize_path_for_logging_escaped(&resolved, log_base_dir)
            );
            return Ok(None);
        }
        Ok(_) => {
            tracing::warn!(
                "[markdown-view] 通常ディレクトリの正規化先がディレクトリではないためスキップ: {} -> {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                sanitize_path_for_logging_escaped(&resolved, log_base_dir)
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "通常ディレクトリの正規化先がディレクトリではありません",
            ));
        }
        Err(error) => {
            if is_symlink {
                tracing::warn!(
                    "[markdown-view] {}のメタデータ取得に失敗（スキップ）: {} ({})",
                    label,
                    sanitize_path_for_logging_escaped(&resolved, log_base_dir),
                    error
                );
                return Ok(None);
            }
            tracing::warn!(
                "[markdown-view] {}のメタデータ取得に失敗: {} ({})",
                label,
                sanitize_path_for_logging_escaped(&resolved, log_base_dir),
                error
            );
            return Err(error);
        }
    }

    if !visited_dirs.insert(resolved.clone()) {
        if is_symlink {
            tracing::warn!(
                "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir)
            );
        }
        return Ok(None);
    }

    Ok(Some(resolved))
}

pub(super) fn canonicalize_dir_for_cycle(
    path: &Path,
    label: &str,
    base_dir: &Path,
) -> Option<PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Some(canonical),
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}の正規化に失敗（スキップ）: {} ({})",
                label,
                sanitize_path_for_logging_lexical_escaped(path, base_dir),
                error
            );
            None
        }
    }
}
