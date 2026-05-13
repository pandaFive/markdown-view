#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use super::support::{create_test_dir, make_dir_unsearchable};
use crate::server::files::catalog::{
    canonicalize_dir_for_cycle, ensure_current_dir_still_canonical,
    list_markdown_files_from_canonical_base, MAX_DIR_DEPTH, MAX_FILE_LIST,
};
use crate::server::files::*;
use crate::server::CanonicalPath;

#[test]
fn test_list_markdown_files_基本動作() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"guide.md".to_string()));
    assert!(files.contains(&"docs/api.md".to_string()));
}

#[test]
fn test_list_markdown_files_from_canonical_base_基本動作() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.md"), "# b").unwrap();
    std::fs::write(dir.path().join("a.md"), "# a").unwrap();
    std::fs::write(dir.path().join("skip.txt"), "skip").unwrap();

    let canonical = CanonicalPath::try_from_path(dir.path()).unwrap();
    let files = list_markdown_files_from_canonical_base(&canonical, MAX_FILE_LIST).unwrap();

    assert_eq!(files, vec!["a.md".to_string(), "b.md".to_string()]);
}

#[test]
#[cfg(unix)]
fn test_list_markdown_files_from_canonical_base_ベース外symlinkディレクトリは除外() {
    use std::os::unix::fs::symlink;

    let base = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "# secret").unwrap();
    symlink(outside.path(), base.path().join("linked")).unwrap();

    let canonical = CanonicalPath::try_from_path(base.path()).unwrap();
    let files = list_markdown_files_from_canonical_base(&canonical, MAX_FILE_LIST).unwrap();

    assert!(files.is_empty());
}

#[test]
#[cfg(unix)]
#[tracing_test::traced_test]
fn test_list_markdown_files_from_canonical_base_ベース内symlinkはディレクトリだけ辿る() {
    use std::os::unix::fs::symlink;

    let base = tempfile::tempdir().unwrap();
    std::fs::write(base.path().join("target.txt"), "target").unwrap();
    symlink(
        base.path().join("target.txt"),
        base.path().join("linked_file.md"),
    )
    .unwrap();

    let visible_dir = base.path().join("target-dir");
    std::fs::create_dir_all(&visible_dir).unwrap();
    std::fs::write(visible_dir.join("doc.md"), "# doc").unwrap();
    symlink(&visible_dir, base.path().join("linked_dir")).unwrap();

    let canonical = CanonicalPath::try_from_path(base.path()).unwrap();
    let files = list_markdown_files_from_canonical_base(&canonical, MAX_FILE_LIST).unwrap();

    // read_dirの順序は未保証のため、canonical重複排除で実体側かsymlink側のどちらが残るかは環境依存。
    assert_eq!(files.len(), 1);
    assert!(
        files == vec!["linked_dir/doc.md".to_string()]
            || files == vec!["target-dir/doc.md".to_string()],
        "base内の同一canonicalディレクトリからdoc.mdが1件だけ返る必要がある: {:?}",
        files
    );
    assert!(logs_contain(
        "シンボリックリンクが通常ファイルを指すためスキップ"
    ));
    assert!(logs_contain("linked_file.md"));
}

#[test]
#[cfg(unix)]
fn test_list_markdown_files_from_canonical_base_隠しsymlink先ディレクトリは除外() {
    use std::os::unix::fs::symlink;

    let base = tempfile::tempdir().unwrap();
    let hidden_dir = base.path().join(".target-dir");
    std::fs::create_dir_all(&hidden_dir).unwrap();
    std::fs::write(hidden_dir.join("secret.md"), "# secret").unwrap();
    symlink(&hidden_dir, base.path().join("linked_dir")).unwrap();

    let canonical = CanonicalPath::try_from_path(base.path()).unwrap();
    let files = list_markdown_files_from_canonical_base(&canonical, MAX_FILE_LIST).unwrap();

    assert!(files.is_empty());
}

#[test]
#[cfg(unix)]
#[tracing_test::traced_test]
fn test_list_markdown_files_from_canonical_base_制御文字入りsymlink名をescapeしてログ出力する() {
    let base = tempfile::tempdir().unwrap();
    let hidden_dir = base.path().join(".target-dir");
    std::fs::create_dir_all(&hidden_dir).unwrap();
    std::fs::write(hidden_dir.join("secret.md"), "# secret").unwrap();
    let link_name = std::ffi::OsString::from_vec(b"linked\n\x1b_dir".to_vec());
    symlink(&hidden_dir, base.path().join(&link_name)).unwrap();

    let canonical = CanonicalPath::try_from_path(base.path()).unwrap();
    let files = list_markdown_files_from_canonical_base(&canonical, MAX_FILE_LIST).unwrap();

    assert!(files.is_empty());
    assert!(logs_contain("linked\\n\\u{1b}_dir"));
    assert!(!logs_contain("linked\n\x1b_dir"));
}

#[test]
fn test_list_markdown_files_非md除外() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(!files.iter().any(|f| f.ends_with(".txt")));
}

#[test]
fn test_list_markdown_files_隠しファイル除外() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(!files.iter().any(|f| f.contains(".hidden")));
    assert!(!files.iter().any(|f| f.starts_with('.')));
}

#[test]
fn test_list_markdown_files_生成物ディレクトリ配下を除外する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("docs/guide.md"), "# guide").unwrap();
    std::fs::write(dir.path().join("node_modules/pkg/readme.md"), "# generated").unwrap();
    std::fs::write(dir.path().join("target/debug/build.md"), "# generated").unwrap();

    let files = list_markdown_files(dir.path()).unwrap();

    assert_eq!(files, vec!["docs/guide.md".to_string()]);
}

#[test]
fn test_list_markdown_files_空ディレクトリ() {
    let dir = tempfile::tempdir().unwrap();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.is_empty());
}

#[tokio::test]
async fn test_search_directory_canonical_base_再canonicalizeなしで検索する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("guide.md"), "hello search target").unwrap();
    let canonical = CanonicalPath::try_from_path(dir.path()).unwrap();

    let response = search_directory(&canonical, "target").await.unwrap();

    assert_eq!(response.query, "target");
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].file, "guide.md");
}

#[tokio::test]
async fn test_search_directory_生成物ディレクトリ配下を検索しない() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("docs/guide.md"), "needle visible").unwrap();
    std::fs::write(
        dir.path().join("node_modules/pkg/readme.md"),
        "needle generated",
    )
    .unwrap();
    std::fs::write(dir.path().join("target/debug/build.md"), "needle generated").unwrap();
    let canonical = CanonicalPath::try_from_path(dir.path()).unwrap();

    let response = search_directory(&canonical, "needle").await.unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].file, "docs/guide.md");
    assert_eq!(response.searched_files, 1);
}

#[test]
fn test_list_markdown_files_ソート済み() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);
}

#[test]
fn test_list_markdown_files_最大1000件で打ち切る() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..(MAX_FILE_LIST + 200) {
        let path = dir.path().join(format!("doc-{i:04}.md"));
        std::fs::write(path, "# x").unwrap();
    }

    let files = list_markdown_files(dir.path()).unwrap();
    assert_eq!(files.len(), MAX_FILE_LIST);
}

#[test]
fn test_list_markdown_files_ベースディレクトリ正規化失敗はエラーを返す() {
    let missing = PathBuf::from("/path/that/does/not/exist");
    let result = list_markdown_files(&missing);

    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
}

#[test]
#[cfg(unix)]
fn test_list_markdown_files_ベースディレクトリ正規化失敗のio_error_kindを保持する() {
    let parent = tempfile::tempdir().unwrap();
    let base = parent.path().join("blocked");
    std::fs::create_dir(&base).unwrap();
    let Some(_guard) = make_dir_unsearchable(parent.path(), &base) else {
        return;
    };

    let result = list_markdown_files(&base);

    assert_eq!(
        result.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
}

#[cfg(unix)]
#[test]
fn test_list_markdown_files_シンボリックリンクサイクルでハングしない() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# README").unwrap();
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/doc.md"), "# Doc").unwrap();

    std::os::unix::fs::symlink(dir.path(), dir.path().join("sub/loop")).unwrap();

    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"sub/doc.md".to_string()));
    assert!(
        !files.iter().any(|f| f.contains("loop/")),
        "サイクル経由のエントリが含まれてはいけない: {:?}",
        files
    );
}

#[cfg(unix)]
#[test]
fn test_list_markdown_files_自己参照シンボリックリンクでハングしない() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# README").unwrap();

    std::os::unix::fs::symlink(".", dir.path().join("loop")).unwrap();

    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(
        !files.iter().any(|f| f.contains("loop/")),
        "サイクル経由のエントリが含まれてはいけない: {:?}",
        files
    );
}

#[test]
fn test_list_markdown_files_深度上限を超えるパスは除外される() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("root.md"), "# root").unwrap();

    let mut current = dir.path().to_path_buf();
    for i in 0..=MAX_DIR_DEPTH {
        current = current.join(format!("d{}", i));
        std::fs::create_dir_all(&current).unwrap();
    }
    std::fs::write(current.join("deep.md"), "# deep").unwrap();

    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"root.md".to_string()));
    assert!(!files.iter().any(|f| f.ends_with("deep.md")));
}

#[test]
fn test_list_markdown_files_recursive_通常ディレクトリcanonicalize失敗時はスキップ扱い() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-dir");
    assert!(canonicalize_dir_for_cycle(&missing, "通常ディレクトリ", dir.path()).is_none());
}

#[test]
fn test_resolve_recursable_directory_通常ディレクトリをvisitedに登録する() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let canonical_base = dir.path().canonicalize().unwrap();
    let mut visited_dirs = std::collections::HashSet::new();
    visited_dirs.insert(canonical_base.clone());

    let resolved = crate::server::files::catalog::resolve_recursable_directory(
        &child,
        false,
        &canonical_base,
        &mut visited_dirs,
        dir.path(),
    )
    .expect("通常ディレクトリの再帰判定は成功する")
    .expect("通常ディレクトリは再帰対象になる");

    assert_eq!(resolved, child.canonicalize().unwrap());
    assert!(visited_dirs.contains(&resolved));
}

#[test]
fn test_resolve_recursable_directory_通常ディレクトリ扱いの非ディレクトリ正規化先はエラー() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-dir.md");
    std::fs::write(&file, "# file").unwrap();
    let canonical_base = dir.path().canonicalize().unwrap();
    let mut visited_dirs = std::collections::HashSet::new();
    visited_dirs.insert(canonical_base.clone());

    let error = crate::server::files::catalog::resolve_recursable_directory(
        &file,
        false,
        &canonical_base,
        &mut visited_dirs,
        dir.path(),
    )
    .expect_err("通常ディレクトリ扱いなら非ディレクトリ正規化先はエラー");

    assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
}

#[cfg(unix)]
#[test]
fn test_resolve_recursable_directory_base内symlinkディレクトリをvisitedに登録する() {
    let base = tempfile::tempdir().unwrap();
    let target = base.path().join("target-dir");
    std::fs::create_dir(&target).unwrap();
    let link = base.path().join("linked_dir");
    symlink(&target, &link).unwrap();
    let canonical_base = base.path().canonicalize().unwrap();
    let mut visited_dirs = std::collections::HashSet::new();
    visited_dirs.insert(canonical_base.clone());

    let resolved = crate::server::files::catalog::resolve_recursable_directory(
        &link,
        true,
        &canonical_base,
        &mut visited_dirs,
        base.path(),
    )
    .expect("base内symlinkディレクトリの再帰判定は成功する")
    .expect("base内symlinkディレクトリは再帰対象になる");

    assert_eq!(resolved, target.canonicalize().unwrap());
    assert!(visited_dirs.contains(&resolved));
}

#[cfg(unix)]
#[test]
fn test_ensure_current_dir_still_canonical_差し替えでbase外になったらエラー() {
    let base = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "# secret").unwrap();
    let child = base.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let canonical_base = base.path().canonicalize().unwrap();
    let canonical_child = child.canonicalize().unwrap();
    std::fs::remove_dir(&child).unwrap();
    symlink(outside.path(), &child).unwrap();

    let error = ensure_current_dir_still_canonical(&canonical_child, &canonical_base, base.path())
        .expect_err("base外への差し替えは拒否する");

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[cfg(unix)]
#[test]
fn test_resolve_recursable_directory_base外symlinkはvisitedに登録しない() {
    let base = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let link = base.path().join("linked");
    symlink(outside.path(), &link).unwrap();
    let canonical_base = base.path().canonicalize().unwrap();
    let outside_canonical = outside.path().canonicalize().unwrap();
    let mut visited_dirs = std::collections::HashSet::new();
    visited_dirs.insert(canonical_base.clone());

    let resolved = crate::server::files::catalog::resolve_recursable_directory(
        &link,
        true,
        &canonical_base,
        &mut visited_dirs,
        base.path(),
    );

    assert!(resolved.expect("symlinkの再帰判定は成功する").is_none());
    assert!(!visited_dirs.contains(&outside_canonical));
}
