#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::support::{
    create_directory_state, create_markdown_fixture, create_single_file_state, create_test_dir,
    make_dir_unsearchable,
};
use crate::server::files::resolve::{resolve_change_target, revalidate_single_file_target};
use crate::server::files::*;
use crate::server::state::CanonicalPath;

#[test]
fn test_resolve_file_正常なパス() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "README.md");
    assert!(result.is_ok());
    assert!(result.unwrap().ends_with("README.md"));
}

#[test]
fn test_resolve_file_サブディレクトリのパス() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "docs/api.md");
    assert!(result.is_ok());
}

#[test]
fn test_resolve_file_トラバーサル拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "../../../etc/passwd");
    assert!(matches!(
        result,
        Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
    ));
}

#[test]
fn test_resolve_file_バックスラッシュ型トラバーサルを拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "..\\..\\..\\etc\\passwd");
    assert!(matches!(
        result,
        Err(ResolveFileError::Hidden)
            | Err(ResolveFileError::NotFound)
            | Err(ResolveFileError::Traversal)
    ));
}

#[test]
fn test_resolve_file_urlエンコード型トラバーサルを拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "docs/%2e%2e/%2e%2e/etc/passwd.md");
    assert!(matches!(
        result,
        Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
    ));
}

#[test]
fn test_resolve_file_絶対パス拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "/etc/passwd");
    assert_eq!(result, Err(ResolveFileError::InvalidPath));
}

#[test]
fn test_resolve_file_存在しないファイル() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "nonexistent.md");
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[test]
fn test_resolve_file_非md拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "notes.txt");
    assert_eq!(result, Err(ResolveFileError::NotMarkdown));
}

#[test]
fn test_resolve_file_隠しファイル拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), ".hidden/secret.md");
    assert_eq!(result, Err(ResolveFileError::Hidden));
}

#[test]
fn test_resolve_file_隠しドットファイル拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), ".dotfile.md");
    assert_eq!(result, Err(ResolveFileError::Hidden));
}

#[test]
fn test_resolve_file_生成物ディレクトリ配下は拒否する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("node_modules/pkg/readme.md"), "# generated").unwrap();
    std::fs::write(dir.path().join("target/debug/build.md"), "# generated").unwrap();

    assert_eq!(
        resolve_file(dir.path(), "node_modules/pkg/readme.md"),
        Err(ResolveFileError::Hidden)
    );
    assert_eq!(
        resolve_file(dir.path(), "target/debug/build.md"),
        Err(ResolveFileError::Hidden)
    );
}

#[cfg(unix)]
#[test]
fn test_resolve_file_除外componentのsymlink経由は拒否する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/readme.md"), "# docs").unwrap();
    std::os::unix::fs::symlink(dir.path().join("docs"), dir.path().join("node_modules")).unwrap();

    let result = resolve_file(dir.path(), "node_modules/readme.md");

    assert_eq!(result, Err(ResolveFileError::Hidden));
}

#[cfg(unix)]
#[test]
fn test_resolve_file_canonical先が除外componentならsymlink経由も拒否する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("target/debug/build.md"), "# build").unwrap();
    std::os::unix::fs::symlink(dir.path().join("target"), dir.path().join("linked")).unwrap();

    let result = resolve_file(dir.path(), "linked/debug/build.md");

    assert_eq!(result, Err(ResolveFileError::Hidden));
}

#[test]
fn test_resolve_file_nulバイト拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "README\0.md");
    assert_eq!(result, Err(ResolveFileError::InvalidPath));
}

#[test]
fn test_resolve_file_ディレクトリパスはnotfileを返す() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "docs");
    assert_eq!(result, Err(ResolveFileError::NotFile));
}

#[test]
fn test_resolve_file_空パス拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "");
    assert_eq!(result, Err(ResolveFileError::EmptyPath));
}

#[cfg(unix)]
#[test]
fn test_resolve_file_シンボリックリンクによるトラバーサル拒否() {
    let dir = create_test_dir();
    let outside_dir = tempfile::tempdir().unwrap();
    std::fs::write(outside_dir.path().join("secret.md"), "# Secret").unwrap();

    std::os::unix::fs::symlink(
        outside_dir.path().join("secret.md"),
        dir.path().join("link.md"),
    )
    .unwrap();

    let result = resolve_file(dir.path(), "link.md");
    assert_eq!(result, Err(ResolveFileError::Traversal));
}

#[cfg(unix)]
#[test]
fn test_resolve_file_正規化io失敗はio_kindを返す() {
    let dir = tempfile::tempdir().unwrap();
    let locked_dir = dir.path().join("locked");
    std::fs::create_dir(&locked_dir).unwrap();
    let target = locked_dir.join("secret.md");
    std::fs::write(&target, "# secret").unwrap();
    let Some(_guard) = make_dir_unsearchable(&locked_dir, &target) else {
        return;
    };

    let result = resolve_file(dir.path(), "locked/secret.md");

    assert!(matches!(
        result,
        Err(ResolveFileError::Io(std::io::ErrorKind::PermissionDenied))
    ));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更はcanonical_pathへ再解決する() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let changed = dir.path().join("docs/../docs/api.md");
    let expected = dir.path().join("docs/api.md").canonicalize().unwrap();

    let target = resolve_change_target(&state, &changed)
        .expect("watcher change should resolve")
        .expect("directory watcher change should produce a target");

    assert_eq!(target.file_path(), expected.as_path());
    assert_eq!(target.relative_path(), Some("docs/api.md"));
}

#[cfg(unix)]
#[test]
fn test_resolve_change_target_unixのbackslashファイル名を通常文字として扱う() {
    let dir = tempfile::tempdir().unwrap();
    let changed = dir.path().join("back\\slash.md");
    std::fs::write(&changed, "# backslash").unwrap();
    let state = create_directory_state(dir.path());
    let expected = changed.canonicalize().unwrap();

    let target = resolve_change_target(&state, &changed)
        .expect("backslash file name should resolve")
        .expect("directory watcher change should produce a target");

    assert_eq!(target.file_path(), expected.as_path());
    assert_eq!(target.relative_path(), Some("back\\slash.md"));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更の隠しパスは拒否する() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let hidden = dir.path().join(".hidden/secret.md");

    let result = resolve_change_target(&state, &hidden);

    assert!(matches!(result, Err(ResolveFileError::Hidden)));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更のbase外パスは拒否する() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("outside.md");
    std::fs::write(&outside, "# outside").unwrap();
    let state = create_directory_state(base_dir.path());

    let result = resolve_change_target(&state, &outside);

    assert!(matches!(result, Err(ResolveFileError::Traversal)));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更の正規化不能なpathはnotfoundを返す() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("missing.md");
    let state = create_directory_state(base_dir.path());

    let result = resolve_change_target(&state, &outside);

    assert!(matches!(result, Err(ResolveFileError::NotFound)));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更のmissing_pathはnotfoundを返す() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.md");
    let state = create_directory_state(dir.path());

    let result = resolve_change_target(&state, &missing);

    assert!(matches!(result, Err(ResolveFileError::NotFound)));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更のmissing_baseはnotfoundを返す() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.md");
    std::fs::write(&missing, "# missing").unwrap();
    let state = create_directory_state(dir.path());
    drop(dir);

    let result = resolve_change_target(&state, &missing);

    assert!(matches!(result, Err(ResolveFileError::NotFound)));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更のbase正規化notfoundを返す() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("outside.md");
    let state = create_directory_state(base_dir.path());
    drop(base_dir);

    let result = resolve_change_target(&state, &outside);

    assert!(matches!(result, Err(ResolveFileError::NotFound)));
}

#[cfg(unix)]
#[test]
fn test_resolve_change_target_ディレクトリ変更の正規化io失敗はio_kindを返す() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let locked_dir = outside_dir.path().join("locked");
    std::fs::create_dir(&locked_dir).unwrap();
    let target = locked_dir.join("secret.md");
    std::fs::write(&target, "# secret").unwrap();
    let state = create_directory_state(base_dir.path());
    let Some(_guard) = make_dir_unsearchable(&locked_dir, &target) else {
        return;
    };

    let result = resolve_change_target(&state, &target);

    assert!(matches!(
        result,
        Err(ResolveFileError::Io(std::io::ErrorKind::PermissionDenied))
    ));
}

#[cfg(unix)]
#[test]
fn test_resolve_change_target_base配下の正規化io失敗はio_kindを返す() {
    let dir = tempfile::tempdir().unwrap();
    let locked_dir = dir.path().join("locked");
    std::fs::create_dir(&locked_dir).unwrap();
    let target = locked_dir.join("secret.md");
    std::fs::write(&target, "# secret").unwrap();
    let state = create_directory_state(dir.path());
    let Some(_guard) = make_dir_unsearchable(&locked_dir, &target) else {
        return;
    };

    let result = resolve_change_target(&state, &target);

    assert!(matches!(
        result,
        Err(ResolveFileError::Io(std::io::ErrorKind::PermissionDenied))
    ));
}

#[cfg(unix)]
#[test]
fn test_resolve_change_target_ディレクトリ変更の非utf8相対パスは拒否する() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = std::ffi::OsString::from_vec(b"invalid-\xff.md".to_vec());
    let target = dir.path().join(file_name);
    std::fs::write(&target, "# invalid").unwrap();
    let state = create_directory_state(dir.path());

    let result = resolve_change_target(&state, &target);

    assert!(matches!(result, Err(ResolveFileError::InvalidPath)));
}

#[cfg(unix)]
#[test]
fn test_resolve_change_target_ディレクトリ変更のbase外symlinkは拒否する() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("secret.md");
    std::fs::write(&outside, "# secret").unwrap();
    let link = base_dir.path().join("link.md");
    symlink(&outside, &link).unwrap();
    let state = create_directory_state(base_dir.path());

    let result = resolve_change_target(&state, &link);

    assert!(matches!(result, Err(ResolveFileError::Traversal)));
}

#[test]
fn test_resolve_change_target_単一ファイル変更は再検証済みpathを返す() {
    let (_dir, file_path) = create_markdown_fixture("target.md", "# target");
    let canonical = file_path.canonicalize().unwrap();
    let state = create_single_file_state(&file_path);

    let target = resolve_change_target(&state, &file_path)
        .expect("single file change should resolve")
        .expect("single file watcher change should produce a target");

    assert_eq!(target.file_path(), canonical.as_path());
}

#[test]
fn test_resolve_change_target_単一ファイル通常ファイル差し替えは拒否する() {
    let parent = tempfile::tempdir().unwrap();
    let target = parent.path().join("target.md");
    let replacement = parent.path().join("replacement.md");
    std::fs::write(&target, "# target").unwrap();
    std::fs::write(&replacement, "# replacement").unwrap();
    let state = create_single_file_state(&target);
    std::fs::remove_file(&target).unwrap();
    std::fs::rename(&replacement, &target).unwrap();

    let result = resolve_change_target(&state, &target);

    assert!(matches!(result, Err(ResolveFileError::Traversal)));
}

#[test]
fn test_resolve_file_mdディレクトリはnotfileを返す() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("folder.md")).unwrap();

    let result = resolve_file(dir.path(), "folder.md");

    assert_eq!(result, Err(ResolveFileError::NotFile));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更のmdディレクトリはnotfileを返す() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("folder.md");
    std::fs::create_dir(&target).unwrap();
    let state = create_directory_state(dir.path());

    let result = resolve_change_target(&state, &target);

    assert!(matches!(result, Err(ResolveFileError::NotFile)));
}

#[test]
fn test_resolve_file_error_io_displayはerror_kindを含む() {
    let error = ResolveFileError::Io(std::io::ErrorKind::PermissionDenied);

    assert!(error.to_string().contains("PermissionDenied"));
}

#[test]
fn test_resolve_file_error_hidden_displayは除外対象を含む() {
    assert!(ResolveFileError::Hidden.to_string().contains("除外対象"));
}

#[test]
fn test_revalidate_single_file_target_正常なファイルを許可する() {
    let (dir, file_path) = create_markdown_fixture("test.md", "# test");
    let canonical = CanonicalPath::try_from_path(&file_path).unwrap();
    let expected_path = canonical.as_path().to_path_buf();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), expected_path);
}

#[test]
fn test_revalidate_single_file_target_存在しないファイルはnotfoundを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("nonexistent.md");
    std::fs::write(&file_path, "# temporary").unwrap();
    let canonical = CanonicalPath::try_from_path(&file_path).unwrap();
    std::fs::remove_file(&file_path).unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[cfg(unix)]
#[test]
fn test_revalidate_single_file_target_正規化io失敗はio_kindを返す() {
    let dir = tempfile::tempdir().unwrap();
    let locked_dir = dir.path().join("locked");
    std::fs::create_dir(&locked_dir).unwrap();
    let target = locked_dir.join("secret.md");
    std::fs::write(&target, "# secret").unwrap();
    let canonical = CanonicalPath::try_from_path(&target).unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let Some(_guard) = make_dir_unsearchable(&locked_dir, &target) else {
        return;
    };

    let result = revalidate_single_file_target(&canonical, &base_dir);

    assert!(matches!(
        result,
        Err(ResolveFileError::Io(std::io::ErrorKind::PermissionDenied))
    ));
}

#[test]
fn test_revalidate_single_file_target_ディレクトリはnotfileを返す() {
    let dir = tempfile::tempdir().unwrap();
    let canonical = CanonicalPath::try_from_path(dir.path()).unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert_eq!(result, Err(ResolveFileError::NotFile));
}

#[test]
fn test_revalidate_single_file_target_非mdファイルはnotmarkdownを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello").unwrap();
    let canonical = CanonicalPath::try_from_path(&file_path).unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert_eq!(result, Err(ResolveFileError::NotMarkdown));
}

#[test]
fn test_revalidate_single_file_target_実体差し替えはtraversalを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("real.md");
    let replacement = dir.path().join("replacement.md");
    std::fs::write(&file_path, "# real").unwrap();
    std::fs::write(&replacement, "# replacement").unwrap();
    let canonical = CanonicalPath::try_from_path(&file_path).unwrap();
    std::fs::remove_file(&file_path).unwrap();
    std::fs::rename(&replacement, &file_path).unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert_eq!(result, Err(ResolveFileError::Traversal));
}
