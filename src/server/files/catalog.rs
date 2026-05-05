use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::server::log_path::sanitize_path_for_logging;
use crate::server::{CanonicalPath, CanonicalPathError};

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
    let base_path = base_dir.as_path();
    let mut files = Vec::new();
    let mut visited_dirs = HashSet::new();
    visited_dirs.insert(base_path.to_path_buf());
    list_markdown_files_recursive(
        base_path,
        base_path,
        base_path,
        &mut files,
        &mut visited_dirs,
        0,
        max_files,
    )?;
    files.sort();
    files.truncate(max_files);
    Ok(files)
}

fn list_markdown_files_recursive(
    log_base_dir: &Path,
    canonical_base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
    visited_dirs: &mut HashSet<PathBuf>,
    depth: usize,
    max_files: usize,
) -> std::io::Result<()> {
    if depth >= MAX_DIR_DEPTH {
        tracing::warn!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            sanitize_path_for_logging(current_dir, log_base_dir)
        );
        return Ok(());
    }

    let entries = std::fs::read_dir(current_dir)?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ディレクトリエントリ読み取りエラー（スキップ）: {} ({})",
                    sanitize_path_for_logging(current_dir, log_base_dir),
                    error
                );
                continue;
            }
        };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ファイルタイプ取得エラー（スキップ）: {} ({})",
                    sanitize_path_for_logging(&path, log_base_dir),
                    error
                );
                continue;
            }
        };

        if file_type.is_dir() || file_type.is_symlink() {
            if files.len() >= max_files {
                return Ok(());
            }

            if file_type.is_symlink() {
                let Some(resolved) =
                    canonicalize_dir_for_cycle(&path, "シンボリックリンク", log_base_dir)
                else {
                    continue;
                };
                if !resolved.starts_with(canonical_base_dir) {
                    tracing::warn!(
                        "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                        sanitize_path_for_logging(&path, log_base_dir),
                        sanitize_path_for_logging(&resolved, log_base_dir)
                    );
                    continue;
                }
                if !resolved.is_dir() {
                    tracing::debug!(
                        "[markdown-view] シンボリックリンクが通常ファイルを指すためスキップ: {} -> {}",
                        name_str,
                        sanitize_path_for_logging(&resolved, log_base_dir)
                    );
                    continue;
                }
                if !visited_dirs.insert(resolved) {
                    tracing::warn!(
                        "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                        sanitize_path_for_logging(&path, log_base_dir)
                    );
                    continue;
                }
            } else {
                let Some(canonical) =
                    canonicalize_dir_for_cycle(&path, "通常ディレクトリ", log_base_dir)
                else {
                    continue;
                };
                if !visited_dirs.insert(canonical) {
                    continue;
                }
            }

            list_markdown_files_recursive(
                log_base_dir,
                canonical_base_dir,
                &path,
                files,
                visited_dirs,
                depth + 1,
                max_files,
            )?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            match path.strip_prefix(log_base_dir) {
                Ok(relative) => {
                    let relative_str = relative
                        .components()
                        .map(|component| component.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    files.push(relative_str);
                    if files.len() >= max_files {
                        return Ok(());
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 相対パス算出不可（スキップ）: {} (ベース: {})",
                        sanitize_path_for_logging(&path, log_base_dir),
                        log_base_dir.display()
                    );
                }
            }
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub(super) fn resolve_recursable_directory(
    path: &Path,
    is_symlink: bool,
    canonical_base_dir: &Path,
    visited_dirs: &mut HashSet<PathBuf>,
    log_base_dir: &Path,
) -> Option<PathBuf> {
    let label = if is_symlink {
        "シンボリックリンク"
    } else {
        "通常ディレクトリ"
    };
    let resolved = canonicalize_dir_for_cycle(path, label, log_base_dir)?;

    if is_symlink && !resolved.starts_with(canonical_base_dir) {
        tracing::warn!(
            "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
            sanitize_path_for_logging(path, log_base_dir),
            sanitize_path_for_logging(&resolved, log_base_dir)
        );
        return None;
    }

    match std::fs::metadata(&resolved) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) if is_symlink => {
            tracing::debug!(
                "[markdown-view] シンボリックリンクが通常ファイルを指すためスキップ: {} -> {}",
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| path.as_os_str().to_string_lossy()),
                sanitize_path_for_logging(&resolved, log_base_dir)
            );
            return None;
        }
        Ok(_) => {
            tracing::warn!(
                "[markdown-view] 通常ディレクトリの正規化先がディレクトリではないためスキップ: {} -> {}",
                sanitize_path_for_logging(path, log_base_dir),
                sanitize_path_for_logging(&resolved, log_base_dir)
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}のメタデータ取得に失敗（スキップ）: {} ({})",
                label,
                sanitize_path_for_logging(&resolved, log_base_dir),
                error
            );
            return None;
        }
    }

    if !visited_dirs.insert(resolved.clone()) {
        if is_symlink {
            tracing::warn!(
                "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                sanitize_path_for_logging(path, log_base_dir)
            );
        }
        return None;
    }

    Some(resolved)
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
                sanitize_path_for_logging(path, base_dir),
                error
            );
            None
        }
    }
}
