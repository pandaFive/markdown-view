#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{fs, os::unix::fs::symlink};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use tokio::sync::broadcast;

use super::catalog::{canonicalize_dir_for_cycle, MAX_DIR_DEPTH, MAX_FILE_LIST};
use super::content::{read_bytes_with_limit, ReadMarkdownError};
use super::memo_sidecar::SidecarMemoName;
use super::resolve::revalidate_single_file_target;
use super::*;
use crate::server::{AppMode, AppState, BroadcastMessage};

fn assert_plain_sidecar_filename(name: &str) {
    let path = Path::new(name);
    assert!(path.parent().is_none() || path.parent() == Some(Path::new("")));
    assert_eq!(
        path.file_name().and_then(|file_name| file_name.to_str()),
        Some(name)
    );
}

#[test]
fn test_close_code_ioエラーは1011を返す() {
    let err = ReadMarkdownError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, ""));
    assert_eq!(err.close_code(), 1011);
}

#[test]
fn test_close_code_too_largeは1009を返す() {
    let err = ReadMarkdownError::TooLarge;
    assert_eq!(err.close_code(), 1009);
}

#[test]
fn test_close_code_not_utf8は1003を返す() {
    let err = ReadMarkdownError::NotUtf8;
    assert_eq!(err.close_code(), 1003);
}

#[test]
fn test_sidecar_name_超長名は255バイト以内に短縮される() {
    let file_name = format!("{}.md", "a".repeat(251));
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.starts_with("."));
    assert!(name.ends_with(".memo.md"));
    assert!(name.len() <= 255, "sidecar名が長すぎる: {}", name.len());
}

#[test]
fn test_sidecar_name_ファイル名なしfallbackは従来名を保つ() {
    let sidecar = SidecarMemoName::fallback();
    assert_plain_sidecar_filename(sidecar.as_str());
    assert_eq!(sidecar.as_str(), ".memo.md");
}

#[test]
fn test_sidecar_name_空ファイル名はfallbackを返す() {
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(""));
    assert_plain_sidecar_filename(sidecar.as_str());
    assert_eq!(sidecar.as_str(), ".memo.md");
}

#[test]
fn test_sidecar_name_同一prefixの超長名はhashで衝突しない() {
    let common_prefix = "a".repeat(260);
    let first_name = format!("{common_prefix}-first.md");
    let second_name = format!("{common_prefix}-second.md");
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&first_name));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&second_name));
    assert_plain_sidecar_filename(first.as_str());
    assert_plain_sidecar_filename(second.as_str());
    assert_ne!(first.as_str(), second.as_str());
    assert!(first.as_str().len() <= 255);
    assert!(second.as_str().len() <= 255);
}

#[test]
fn test_sidecar_name_特殊文字はパス区切りとして扱われない() {
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new("../secret\\..\\memo.md"));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.ends_with(".memo.md"));
    assert!(!name.contains('/'), "slashが残ってはいけない: {name}");
    assert!(!name.contains('\\'), "backslashが残ってはいけない: {name}");
    assert!(
        name.contains(".."),
        "通常文字としてのdotは保持してよい: {name}"
    );
}

#[test]
fn test_sidecar_name_正規化された短い名前はhashで衝突しない() {
    let plain = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a_b.md"));
    let normalized = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    assert_plain_sidecar_filename(plain.as_str());
    assert_plain_sidecar_filename(normalized.as_str());
    assert_eq!(plain.as_str(), ".a_b.md.memo.md");
    assert!(normalized.as_str().starts_with(".a_b.md."));
    assert!(normalized.as_str().ends_with(".memo.md"));
    assert_ne!(plain.as_str(), normalized.as_str());
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_旧形式compat名は正規化前の名前を返す() {
    let compat = SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new("a\\b.md"))
        .expect("backslash name should have compat sidecar");
    assert_plain_sidecar_filename(compat.as_str());
    assert_eq!(compat.as_str(), ".a\\b.md.memo.md");

    assert!(SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new("a_b.md")).is_none());
}

#[test]
fn test_sidecar_name_utf8境界で切り詰める() {
    let file_name = format!("{}終端.md", "あ".repeat(120));
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.ends_with(".memo.md"));
    assert!(name.len() <= 255);
    assert!(name.is_char_boundary(name.len()));
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_非utf8名はhashで衝突しない() {
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::from_bytes(b"guide-\xff.md"));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::from_bytes(b"guide-\xfe.md"));
    assert_plain_sidecar_filename(first.as_str());
    assert_plain_sidecar_filename(second.as_str());
    assert!(first.as_str().starts_with("._bin."));
    assert!(second.as_str().starts_with("._bin."));
    assert!(first.as_str().ends_with(".memo.md"));
    assert!(second.as_str().ends_with(".memo.md"));
    assert_ne!(first.as_str(), second.as_str());
}

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
        Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
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
fn test_resolve_file_nulバイト拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "README\0.md");
    assert_eq!(result, Err(ResolveFileError::InvalidPath));
}

#[test]
fn test_resolve_file_ディレクトリパス拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "docs");
    assert_eq!(result, Err(ResolveFileError::NotFound));
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

#[test]
fn test_list_markdown_files_基本動作() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"guide.md".to_string()));
    assert!(files.contains(&"docs/api.md".to_string()));
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
fn test_list_markdown_files_空ディレクトリ() {
    let dir = tempfile::tempdir().unwrap();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.is_empty());
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
    assert!(result.is_err());
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

#[tokio::test]
async fn test_read_bytes_with_limit_takeによる第2段階チェックで超過を検出する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();

    let file = tokio::fs::File::open(&file_path).await.unwrap();
    let result = read_bytes_with_limit(file).await;
    assert!(matches!(result, Err(ReadMarkdownError::TooLarge)));
}

#[tokio::test]
async fn test_read_markdown_error_into_response_too_largeのjson形式() {
    let response = ReadMarkdownError::TooLarge.into_response();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "error": "ファイルサイズが上限（10MB）を超えています"
        })
    );
}

#[tokio::test]
async fn test_read_markdown_error_into_response_ioのjson形式() {
    let io_error = std::io::Error::other("disk failure");
    let response = ReadMarkdownError::Io(io_error).into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "error": "ファイルの読み込みに失敗しました"
        })
    );
}

#[tokio::test]
async fn test_read_markdown_error_into_response_not_utf8のjson形式() {
    let response = ReadMarkdownError::NotUtf8.into_response();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "error": "このファイルはUTF-8テキストではありません"
        })
    );
}

#[test]
fn test_resolve_route_target_page_ディレクトリモードでrelative_pathとfile_listを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let target =
        resolve_route_target(&state, RouteTargetRequest::page(Some("docs/api.md"))).unwrap();

    assert_eq!(target.relative_path(), Some("docs/api.md"));
    assert!(target.file_list().is_some());
    assert!(target.file_path().ends_with("docs/api.md"));
}

#[test]
fn test_resolve_route_target_api_contentはfile_listを含まない() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let target =
        resolve_route_target(&state, RouteTargetRequest::api_content(Some("docs/api.md"))).unwrap();

    assert_eq!(target.relative_path(), Some("docs/api.md"));
    assert!(target.file_list().is_none());
}

#[test]
fn test_resolve_route_target_page_queryなしではreadmeを優先する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("z-last.md"), "# z").unwrap();
    std::fs::write(dir.path().join("README.md"), "# readme").unwrap();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    assert_eq!(target.relative_path(), Some("README.md"));
    assert!(target.file_path().ends_with("README.md"));
    assert_eq!(
        target.file_list().unwrap(),
        &["README.md".to_string(), "z-last.md".to_string()]
    );
}

#[test]
fn test_resolve_route_target_page_queryなしではreadme不在時に先頭ファイルを選ぶ() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("z-last.md"), "# z").unwrap();
    std::fs::write(dir.path().join("a-first.md"), "# a").unwrap();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    assert_eq!(target.relative_path(), Some("a-first.md"));
    assert!(target.file_path().ends_with("a-first.md"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_旧メモルートがシンボリックリンクなら空メモとして扱う() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("README.md");
    fs::write(&file_path, "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside_dir.path().join("memos")).unwrap();
    fs::write(outside_dir.path().join("memos/README.md"), "legacy memo").unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("unsafe legacy should be ignored when no sidecar exists");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_新メモファイルがシンボリックリンクなら拒否する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::write(outside_dir.path().join("memo.md"), "outside").unwrap();
    symlink(
        outside_dir.path().join("memo.md"),
        dir.path().join(".README.md.memo.md"),
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("symlinked memo leaf should be rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
}

#[tokio::test]
async fn test_save_route_memo_単一ファイルモードで同階層sidecarへ保存する() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("memo should save");

    assert_eq!(memo.raw(), "memo");
    assert!(file_path
        .parent()
        .unwrap()
        .join(".test.md.memo.md")
        .exists());
}

#[tokio::test]
async fn test_load_route_memo_旧パスのみ存在する場合はそのまま読み込む() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("legacy memo should load");

    assert_eq!(memo.raw(), "legacy memo");
    assert!(!dir.path().join(".README.md.memo.md").exists());
    assert!(dir.path().join(".markdown-view/memos/README.md").exists());
}

#[tokio::test]
async fn test_load_route_memo_新旧両方ある場合は新sidecarを優先する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::write(dir.path().join(".README.md.memo.md"), "new memo").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("new memo should win");

    assert_eq!(memo.raw(), "new memo");
    assert!(dir.path().join(".markdown-view/memos/README.md").exists());
}

#[tokio::test]
async fn test_save_route_memo_旧パスのみ存在する場合は新sidecarへ移行して保存する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "updated memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy memo should migrate on save");

    assert_eq!(memo.raw(), "updated memo");
    assert!(dir.path().join(".README.md.memo.md").exists());
    assert!(!dir.path().join(".markdown-view/memos/README.md").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_旧symlinkが残っていてもsidecar保存を継続できる() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("unsafe legacy should not block sidecar save");

    assert_eq!(memo.raw(), "memo");
    assert!(dir.path().join(".README.md.memo.md").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存はunsafeなlegacyがあってもsidecar削除を優先する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::write(dir.path().join(".README.md.memo.md"), "memo").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside_dir.path().join("memos")).unwrap();
    fs::write(outside_dir.path().join("memos/README.md"), "legacy memo").unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("unsafe legacy should not block sidecar delete");

    assert_eq!(memo.raw(), "");
    assert!(!dir.path().join(".README.md.memo.md").exists());
    assert!(dir.path().join(".markdown-view").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存でsafe_legacy削除失敗ならエラーにする() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    let sidecar_path = dir.path().join(".README.md.memo.md");
    fs::write(&sidecar_path, "memo").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    let legacy_path = dir.path().join(".markdown-view/memos/README.md");
    fs::write(&legacy_path, "legacy memo").unwrap();

    let legacy_parent = legacy_path.parent().unwrap();
    let original_mode = fs::metadata(legacy_parent).unwrap().permissions().mode();
    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(0o555)).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(original_mode)).unwrap();

    let (status, body) = result.expect_err("delete should fail when safe legacy cleanup fails");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert!(sidecar_path.exists());
    assert!(legacy_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_保存成功後のlegacy削除失敗は成功扱いにする() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    let legacy_path = dir.path().join(".markdown-view/memos/README.md");
    fs::write(&legacy_path, "legacy memo").unwrap();
    let legacy_parent = legacy_path.parent().unwrap();
    let original_mode = fs::metadata(legacy_parent).unwrap().permissions().mode();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "updated memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("save should succeed before legacy cleanup");

    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(0o555)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "updated again".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;
    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(original_mode)).unwrap();

    assert_eq!(memo.raw(), "updated memo");
    let saved = result.expect("legacy cleanup failure should be non-fatal");
    assert_eq!(saved.raw(), "updated again");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_書込不可サブディレクトリではlegacyへfallbackする() {
    let dir = tempfile::tempdir().unwrap();
    let docs_dir = dir.path().join("docs");
    fs::create_dir_all(&docs_dir).unwrap();
    fs::write(docs_dir.join("guide.md"), "# Guide").unwrap();
    let original_mode = fs::metadata(&docs_dir).unwrap().permissions().mode();
    fs::set_permissions(&docs_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let state = create_directory_state(dir.path());
    let target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("docs/guide.md"))).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(Some("docs/guide.md")),
    )
    .await;

    fs::set_permissions(&docs_dir, fs::Permissions::from_mode(original_mode)).unwrap();

    let memo = result.expect("readonly subdir should fall back to legacy");
    assert_eq!(memo.raw(), "memo");
    assert!(!docs_dir.join(".guide.md.memo.md").exists());
    assert!(dir
        .path()
        .join(".markdown-view/memos/docs/guide.md")
        .exists());
}

#[tokio::test]
async fn test_save_route_memo_拡張子の大文字小文字が異なるファイルでもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("guide.md"), "# lower").unwrap();
    fs::write(dir.path().join("guide.MD"), "# upper").unwrap();
    let state = create_directory_state(dir.path());

    let lower_target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("guide.md"))).unwrap();
    let upper_target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("guide.MD"))).unwrap();

    let lower = save_route_memo(
        &state,
        &lower_target,
        "lower memo".to_string(),
        RouteTargetRequest::api_memo(Some("guide.md")),
    )
    .await
    .expect("lower memo should save");
    let upper = save_route_memo(
        &state,
        &upper_target,
        "upper memo".to_string(),
        RouteTargetRequest::api_memo(Some("guide.MD")),
    )
    .await
    .expect("upper memo should save");

    assert_eq!(lower.raw(), "lower memo");
    assert_eq!(upper.raw(), "upper memo");
    assert!(dir.path().join(".guide.md.memo.md").exists());
    assert!(dir.path().join(".guide.MD.memo.md").exists());
}

#[tokio::test]
async fn test_save_route_memo_長いファイル名でもlegacyへfallbackして保存できる() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("long filename should still save");

    assert_eq!(memo.raw(), "memo");
    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 1);
    assert!(memo_entries[0].len() <= 255);
}

#[tokio::test]
async fn test_load_route_memo_長いファイル名でlegacy未作成なら空メモを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("overlong sidecar path should not break empty memo read");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[tokio::test]
async fn test_save_route_memo_長いファイル名のlegacyメモは空白保存で削除できる() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    let legacy_path = dir.path().join(".markdown-view/memos").join(&file_name);
    fs::write(&legacy_path, "memo").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        " \n ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy fallback memo should be deletable");

    assert_eq!(memo.raw(), "");
    assert!(!legacy_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_非utf8ファイル名でもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    let lower_name = std::ffi::OsStr::from_bytes(b"guide-\xff.md");
    let upper_name = std::ffi::OsStr::from_bytes(b"guide-\xfe.md");
    let lower_path = dir.path().join(lower_name);
    let upper_path = dir.path().join(upper_name);
    fs::write(&lower_path, "# lower").unwrap();
    fs::write(&upper_path, "# upper").unwrap();

    let lower_state = create_single_file_state(&lower_path);
    let upper_state = create_single_file_state(&upper_path);
    let lower_target =
        resolve_route_target(&lower_state, RouteTargetRequest::api_memo(None)).unwrap();
    let upper_target =
        resolve_route_target(&upper_state, RouteTargetRequest::api_memo(None)).unwrap();

    let lower = save_route_memo(
        &lower_state,
        &lower_target,
        "lower memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("lower memo should save");
    let upper = save_route_memo(
        &upper_state,
        &upper_target,
        "upper memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("upper memo should save");

    assert_eq!(lower.raw(), "lower memo");
    assert_eq!(upper.raw(), "upper memo");

    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 2);
    assert_ne!(memo_entries[0], memo_entries[1]);
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_正規化される短いファイル名でもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    let plain_path = dir.path().join("a_b.md");
    let separator_shaped_path = dir.path().join("a\\b.md");
    fs::write(&plain_path, "# plain").unwrap();
    fs::write(&separator_shaped_path, "# separator shaped").unwrap();

    let plain_state = create_single_file_state(&plain_path);
    let separator_shaped_state = create_single_file_state(&separator_shaped_path);
    let plain_target =
        resolve_route_target(&plain_state, RouteTargetRequest::api_memo(None)).unwrap();
    let separator_shaped_target =
        resolve_route_target(&separator_shaped_state, RouteTargetRequest::api_memo(None)).unwrap();

    save_route_memo(
        &plain_state,
        &plain_target,
        "plain memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("plain memo should save");
    save_route_memo(
        &separator_shaped_state,
        &separator_shaped_target,
        "separator shaped memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("separator shaped memo should save");

    let plain = load_route_memo(
        &plain_state,
        &plain_target,
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("plain memo should load");
    let separator_shaped = load_route_memo(
        &separator_shaped_state,
        &separator_shaped_target,
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("separator shaped memo should load");

    assert_eq!(plain.raw(), "plain memo");
    assert_eq!(separator_shaped.raw(), "separator shaped memo");

    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 2);
    assert!(memo_entries.contains(&".a_b.md.memo.md".to_string()));
    assert_ne!(memo_entries[0], memo_entries[1]);
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_旧形式backslash_sidecarを読み込む() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(dir.path().join(".a\\b.md.memo.md"), "compat memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("compat sidecar memo should load");

    assert_eq!(memo.raw(), "compat memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_旧形式backslash_sidecarを新形式へ移行する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let old_sidecar = dir.path().join(".a\\b.md.memo.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&old_sidecar, "compat memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("compat sidecar should migrate on save");

    assert_eq!(memo.raw(), "new memo");
    assert_eq!(fs::read_to_string(&new_sidecar).unwrap(), "new memo");
    assert!(!old_sidecar.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_新旧backslash_sidecar両方ある場合は新形式を優先する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let old_sidecar = dir.path().join(".a\\b.md.memo.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&old_sidecar, "compat memo").unwrap();
    fs::write(&new_sidecar, "new memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("new sidecar memo should win");

    assert_eq!(memo.raw(), "new memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_単一ファイルモードではpermission_deniedでもlegacyへfallbackしない() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    fs::write(&file_path, "# test").unwrap();
    let original_mode = fs::metadata(dir.path()).unwrap().permissions().mode();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    fs::set_permissions(dir.path(), fs::Permissions::from_mode(original_mode)).unwrap();

    let (status, body) = result.expect_err("single file mode should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_単一ファイルモードでも既存legacyがあればpermission_denied時にfallbackする(
) {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    fs::write(&file_path, "# test").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/test.md"),
        "legacy memo",
    )
    .unwrap();

    let original_mode = fs::metadata(dir.path()).unwrap().permissions().mode();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "updated memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    fs::set_permissions(dir.path(), fs::Permissions::from_mode(original_mode)).unwrap();

    let memo = result.expect("existing legacy should remain writable fallback");
    assert_eq!(memo.raw(), "updated memo");
    assert_eq!(
        fs::read_to_string(dir.path().join(".markdown-view/memos/test.md")).unwrap(),
        "updated memo"
    );
    assert!(!dir.path().join(".test.md.memo.md").exists());
}

#[tokio::test]
async fn test_load_initial_socket_update_単一ファイルモードでupdateを返す() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);

    let update = load_initial_socket_update(&state).await.unwrap().unwrap();
    assert!(update.content().as_str().contains("title"));
}

#[tokio::test]
async fn test_load_initial_socket_update_ディレクトリモードではnoneを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let result = load_initial_socket_update(&state).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_load_initial_socket_update_単一ファイル削除時は1008エラーを返す() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    std::fs::remove_file(&file_path).unwrap();

    let err = load_initial_socket_update(&state)
        .await
        .expect_err("削除済みファイルはSocketInitErrorを返すべき");

    assert_eq!(err.close_code(), 1008);
}

#[tokio::test]
async fn test_load_initial_socket_update_単一ファイルサイズ超過時は1009エラーを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);

    let err = load_initial_socket_update(&state)
        .await
        .expect_err("サイズ超過ファイルはSocketInitErrorを返すべき");

    assert_eq!(err.close_code(), 1009);
}

#[tokio::test]
async fn test_load_route_update_ioエラーを500へ変換する() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();
    std::fs::remove_file(&file_path).unwrap();

    let (status, body) = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect_err("missing file should map to api error");

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "ファイルの読み込みに失敗しました");
}

#[tokio::test]
async fn test_load_route_update_サイズ超過を413へ変換する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    let (status, body) = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect_err("oversized file should map to api error");

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "ファイルサイズが上限（10MB）を超えています");
}

#[tokio::test]
async fn test_load_route_update_非utf8を422へ変換する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("binary.md");
    tokio::fs::write(&file_path, vec![0xff, 0xfe, 0xfd])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    let (status, body) = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect_err("invalid utf8 should map to api error");

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "このファイルはUTF-8テキストではありません");
}

#[tokio::test]
async fn test_build_lagged_recovery_message_ディレクトリモードではrefreshを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let message = build_lagged_recovery_message(&state).await;

    assert!(matches!(message, BroadcastMessage::Refresh));
}

#[tokio::test]
async fn test_build_lagged_recovery_message_単一ファイルモードではmemo_refresh付きupdateを返す() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::LaggedRecovery(message) => {
            let json = serde_json::to_value(message).unwrap();
            assert!(json["content"].as_str().unwrap().contains("title"));
            assert_eq!(json["memo_refresh"], true);
            assert!(json.get("memo_file").is_none());
        }
        other => panic!("LaggedRecoveryを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_lagged_recovery_message_単一ファイル読み込み失敗時はerrorを返す() {
    let (_dir, file_path) = create_markdown_fixture("missing.md", "# title");
    let state = create_single_file_state(&file_path);
    std::fs::remove_file(&file_path).unwrap();

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(msg.contains("ファイル検証エラー"));
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_lagged_recovery_message_サイズ超過時は読み込みエラーを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル読み込みエラー"),
                "ReadFailed分岐のプレフィックスを期待: {}",
                msg
            );
            assert!(
                msg.contains("ファイルサイズが上限（10MB）を超えています"),
                "TooLargeのuser_messageを期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_lagged_recovery_message_非utf8時は読み込みエラーを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("binary.md");
    tokio::fs::write(&file_path, vec![0xff, 0xfe, 0xfd])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル読み込みエラー"),
                "ReadFailed分岐のプレフィックスを期待: {}",
                msg
            );
            assert!(
                msg.contains("このファイルはUTF-8テキストではありません"),
                "NotUtf8のuser_messageを期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_change_broadcast_message_ディレクトリモードでfileを含むupdateを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let target = dir.path().join("docs/api.md");

    let message = build_change_broadcast_message(&state, &target)
        .await
        .unwrap();
    match message {
        BroadcastMessage::Update(update) => {
            assert_eq!(update.file(), Some("docs/api.md"));
        }
        other => panic!("Updateを期待したが {:?} を受信", other),
    }
}

#[test]
fn test_revalidate_single_file_target_正常なファイルを許可する() {
    let (dir, file_path) = create_markdown_fixture("test.md", "# test");
    let canonical = file_path.canonicalize().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), canonical);
}

#[test]
fn test_revalidate_single_file_target_存在しないファイルはnotfoundを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("nonexistent.md");
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&file_path, &base_dir);
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[test]
fn test_revalidate_single_file_target_ディレクトリはnotfoundを返す() {
    let dir = tempfile::tempdir().unwrap();
    let canonical = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &canonical);
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[test]
fn test_revalidate_single_file_target_非mdファイルはnotmarkdownを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello").unwrap();
    let canonical = file_path.canonicalize().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert_eq!(result, Err(ResolveFileError::NotMarkdown));
}

#[cfg(unix)]
#[test]
fn test_revalidate_single_file_target_シンボリックリンクはtraversalを返す() {
    let dir = tempfile::tempdir().unwrap();
    let real_file = dir.path().join("real.md");
    std::fs::write(&real_file, "# real").unwrap();
    let link_path = dir.path().join("link.md");
    std::os::unix::fs::symlink(&real_file, &link_path).unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&link_path, &base_dir);
    assert_eq!(result, Err(ResolveFileError::Traversal));
}

fn create_test_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# README").unwrap();
    std::fs::write(dir.path().join("guide.md"), "# Guide").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "text file").unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/api.md"), "# API").unwrap();
    std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
    std::fs::write(dir.path().join(".hidden/secret.md"), "# Secret").unwrap();
    std::fs::write(dir.path().join(".dotfile.md"), "# Dot").unwrap();
    dir
}

fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join(name);
    std::fs::write(&file_path, content).unwrap();
    (dir, file_path)
}

fn create_single_file_state(file_path: &std::path::Path) -> AppState {
    let mode = AppMode::new_single_file(file_path).unwrap();
    let (tx, _rx) = broadcast::channel(4);
    AppState::new(mode, false, None, tx)
}

fn create_directory_state(dir_path: &std::path::Path) -> AppState {
    let mode = AppMode::new_directory(dir_path).unwrap();
    let (tx, _rx) = broadcast::channel(4);
    AppState::new(mode, false, None, tx)
}
