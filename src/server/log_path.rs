//! 監査ログ用のパスサニタイザ。
//!
//! 絶対パスの直接出力（`path.display()`）はディレクトリ構造を漏らすため、
//! base_dir 相対化を経由する。base 外パスは file_name のみ残して
//! `<outside-base>/{file_name}` で出力する。
//!
//! `base_dir` 自体（`base_dir.display()`）の出力はサーバー所有者が指定した
//! 値であり保守性優先で保持する。本ヘルパーの対象は base 配下/外を含む
//! 「path 引数」側のみ。

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// 監査ログ用にパスを base 相対化する。
///
/// - `path` が `base` 配下: 相対パス文字列（例: `"subdir/file.md"`）
/// - `path == base`: `"."`
/// - `path` が `base` 外: `<outside-base>/{file_name}`
/// - `file_name` 取得不可（ルート等）: `<outside-base>`
///
/// 存在するパスでは canonicalize 後の実パスで base 配下判定を優先し、
/// symlink 経由で base 外へ出るパスの誤判定を防ぐ。
pub(crate) fn sanitize_path_for_logging<'a>(path: &'a Path, base: &Path) -> Cow<'a, str> {
    match canonical_path_status(path, base) {
        Some(relative) if relative.as_os_str().is_empty() => Cow::Borrowed("."),
        Some(relative) => Cow::Owned(relative.display().to_string()),
        None if matches!(
            canonicalize_status(path, base),
            CanonicalizeStatus::OutsideBase
        ) =>
        {
            sanitize_outside_path_for_logging(path)
        }
        None => sanitize_path_for_logging_lexical(path, base),
    }
}

/// 監査ログ用 path を相対化し、制御文字を可視化する。
pub(crate) fn sanitize_path_for_logging_escaped(path: &Path, base: &Path) -> String {
    sanitize_path_for_logging(path, base)
        .as_ref()
        .escape_debug()
        .to_string()
}

/// 監査ログ用 path を字句的に相対化し、制御文字を可視化する。
pub(crate) fn sanitize_path_for_logging_lexical_escaped(path: &Path, base: &Path) -> String {
    sanitize_path_for_logging_lexical(path, base)
        .as_ref()
        .escape_debug()
        .to_string()
}

enum CanonicalizeStatus {
    Relative(PathBuf),
    OutsideBase,
    Unavailable,
}

fn canonical_path_status(path: &Path, base: &Path) -> Option<PathBuf> {
    match canonicalize_status(path, base) {
        CanonicalizeStatus::Relative(relative) => Some(relative),
        CanonicalizeStatus::OutsideBase | CanonicalizeStatus::Unavailable => None,
    }
}

fn canonicalize_status(path: &Path, base: &Path) -> CanonicalizeStatus {
    let canonical_path = match path.canonicalize() {
        Ok(path) => path,
        Err(_) => return CanonicalizeStatus::Unavailable,
    };
    let canonical_base = match base.canonicalize() {
        Ok(base) => base,
        Err(_) => return CanonicalizeStatus::Unavailable,
    };
    match canonical_path.strip_prefix(&canonical_base) {
        Ok(relative) => CanonicalizeStatus::Relative(relative.to_path_buf()),
        Err(_) => CanonicalizeStatus::OutsideBase,
    }
}

fn sanitize_path_for_logging_lexical<'a>(path: &'a Path, base: &Path) -> Cow<'a, str> {
    let normalized_path = normalize_lexical_path(path);
    let normalized_base = normalize_lexical_path(base);

    match normalized_path.strip_prefix(&normalized_base) {
        Ok(relative) if relative.as_os_str().is_empty() => Cow::Borrowed("."),
        Ok(relative) => Cow::Owned(relative.display().to_string()),
        Err(_) => sanitize_outside_path_for_logging(path),
    }
}

fn sanitize_outside_path_for_logging<'a>(path: &'a Path) -> Cow<'a, str> {
    match path.file_name() {
        Some(name) => Cow::Owned(format!("<outside-base>/{}", name.to_string_lossy())),
        None => Cow::Borrowed("<outside-base>"),
    }
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut prefix: Option<OsString> = None;
    let mut has_root = false;
    let mut leading_parents = 0usize;
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(value) => prefix = Some(value.as_os_str().to_os_string()),
            Component::RootDir => has_root = true,
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() && !has_root {
                    leading_parents += 1;
                }
            }
            Component::Normal(value) => parts.push(value.to_os_string()),
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if has_root {
        normalized.push(std::path::MAIN_SEPARATOR_STR);
    }
    for _ in 0..leading_parents {
        normalized.push("..");
    }
    for part in parts {
        normalized.push(part);
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    #[test]
    fn test_sanitize_base配下を相対パスにする() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base/subdir/file.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "subdir/file.md");
    }

    #[test]
    fn test_sanitize_base直下のファイル() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base/file.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "file.md");
    }

    #[test]
    fn test_sanitize_base外はfile_nameのみ() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/other/secret.md");
        assert_eq!(
            sanitize_path_for_logging(&path, &base),
            "<outside-base>/secret.md"
        );
    }

    #[test]
    fn test_sanitize_親ディレクトリでbase外に出る場合はoutside扱い() {
        let base = PathBuf::from("/base");
        let path = base.join("../../secret.md");
        assert_eq!(
            sanitize_path_for_logging(&path, &base),
            "<outside-base>/secret.md"
        );
    }

    #[test]
    fn test_sanitize_親ディレクトリを含んでもbase配下なら相対化する() {
        let base = PathBuf::from("/base");
        let path = base.join("docs/../file.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "file.md");
    }

    #[test]
    fn test_sanitize_subdirからbase外に出る場合はoutside扱い() {
        let base = PathBuf::from("/base");
        let path = base.join("sub/../../secret.md");
        assert_eq!(
            sanitize_path_for_logging(&path, &base),
            "<outside-base>/secret.md"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_symlink経由でbase外に出る場合はoutside扱い() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(outside.join("private")).unwrap();
        std::fs::write(outside.join("private/doc.md"), "# doc").unwrap();
        symlink(&outside, base.join("link")).unwrap();

        let path = base.join("link/private/doc.md");
        assert_eq!(
            sanitize_path_for_logging(&path, &base),
            "<outside-base>/doc.md"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_symlink経由でもbase配下なら相対化する() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("doc.md"), "# doc").unwrap();
        symlink(&nested, base.join("link")).unwrap();

        let path = base.join("link/doc.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "nested/doc.md");
    }

    #[test]
    fn test_sanitize_file_nameなしは完全マスク() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/");
        assert_eq!(sanitize_path_for_logging(&path, &base), "<outside-base>");
    }

    #[test]
    fn test_sanitize_path自体がbaseの場合() {
        // ベースディレクトリ自身は空文字ではなく "." で識別可能にする。
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base");
        assert_eq!(sanitize_path_for_logging(&path, &base), ".");
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_非utf8_file_name() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let base = PathBuf::from("/base");
        let mut path = PathBuf::from("/other");
        // 不正な UTF-8 シーケンスを含む file_name
        path.push(OsStr::from_bytes(b"bad\xff.md"));
        let sanitized = sanitize_path_for_logging(&path, &base);
        // to_string_lossy が U+FFFD を挿入しても panic しないこと
        assert!(sanitized.starts_with("<outside-base>/bad"));
        assert!(sanitized.ends_with(".md"));
    }

    #[test]
    fn test_sanitize_escapedは制御文字を可視化する() {
        let base = PathBuf::from("/base");
        let path = base.join("line\n\x1b.md");

        let sanitized = sanitize_path_for_logging_escaped(&path, &base);

        assert_eq!(sanitized, "line\\n\\u{1b}.md");
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\x1b'));
    }
}
