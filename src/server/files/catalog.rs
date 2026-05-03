use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::server::log_path::sanitize_path_for_logging;

/// ファイル一覧の最大件数
pub(super) const MAX_FILE_LIST: usize = 1000;

/// ディレクトリ走査の最大深度（スタックオーバーフロー防止）
pub(super) const MAX_DIR_DEPTH: usize = 32;

/// ディレクトリ内の.mdファイルを再帰的に列挙する
pub fn list_markdown_files(base_dir: &Path) -> std::io::Result<Vec<String>> {
    list_markdown_files_with_limit(base_dir, MAX_FILE_LIST)
}

pub(super) fn list_markdown_files_with_limit(
    base_dir: &Path,
    max_files: usize,
) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    let mut visited_dirs = HashSet::new();
    let canonical_base = base_dir.canonicalize()?;
    visited_dirs.insert(canonical_base);
    list_markdown_files_recursive(
        base_dir,
        base_dir,
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
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
    visited_dirs: &mut HashSet<PathBuf>,
    depth: usize,
    max_files: usize,
) -> std::io::Result<()> {
    if depth >= MAX_DIR_DEPTH {
        tracing::warn!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            sanitize_path_for_logging(current_dir, base_dir)
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
                    sanitize_path_for_logging(current_dir, base_dir),
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
                    sanitize_path_for_logging(&path, base_dir),
                    error
                );
                continue;
            }
        };

        if file_type.is_dir() || (file_type.is_symlink() && path.is_dir()) {
            if files.len() >= max_files {
                return Ok(());
            }

            if file_type.is_symlink() {
                let Some(resolved) =
                    canonicalize_dir_for_cycle(&path, "シンボリックリンク", base_dir)
                else {
                    continue;
                };
                let canonical_base = match base_dir.canonicalize() {
                    Ok(path) => path,
                    Err(error) => {
                        tracing::warn!(
                            "[markdown-view] ベースディレクトリの正規化に失敗（スキップ）: {} ({})",
                            base_dir.display(),
                            error
                        );
                        continue;
                    }
                };
                if !resolved.starts_with(&canonical_base) {
                    tracing::warn!(
                        "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                        sanitize_path_for_logging(&path, base_dir),
                        sanitize_path_for_logging(&resolved, base_dir)
                    );
                    continue;
                }
                if !visited_dirs.insert(resolved) {
                    tracing::warn!(
                        "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                        sanitize_path_for_logging(&path, base_dir)
                    );
                    continue;
                }
            } else {
                let Some(canonical) =
                    canonicalize_dir_for_cycle(&path, "通常ディレクトリ", base_dir)
                else {
                    continue;
                };
                if !visited_dirs.insert(canonical) {
                    continue;
                }
            }

            list_markdown_files_recursive(
                base_dir,
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
            match path.strip_prefix(base_dir) {
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
                        sanitize_path_for_logging(&path, base_dir),
                        base_dir.display()
                    );
                }
            }
        }
    }

    Ok(())
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
