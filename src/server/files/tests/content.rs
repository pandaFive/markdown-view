use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::support::{
    create_directory_state, create_markdown_fixture, create_single_file_state, create_test_dir,
};
use crate::server::files::content::{
    check_readable_before_render, map_socket_validation_error, read_bytes_with_limit,
    set_content_before_read_hook_for_test, ReadMarkdownError,
};
use crate::server::files::*;

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
fn test_map_socket_validation_error_internalstateは1011を返す() {
    let error = map_socket_validation_error(ResolveFileError::InternalState);

    assert_eq!(error.close_code(), 1011);
    assert!(error.reason().contains("内部エラー"));
}

#[test]
fn test_map_socket_validation_error_ioはkindを外部表示しない() {
    let error =
        map_socket_validation_error(ResolveFileError::Io(std::io::ErrorKind::PermissionDenied));

    assert_eq!(error.close_code(), 1008);
    assert_eq!(
        error.reason(),
        "ファイル検証に失敗しました: ファイルの検証に失敗しました"
    );
    assert!(!error.reason().contains("PermissionDenied"));
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

#[tokio::test]
async fn test_resolve_route_target_page_ディレクトリモードでrelative_pathとfile_listを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(Some("docs/api.md")))
        .await
        .unwrap();

    assert_eq!(target.relative_path(), Some("docs/api.md"));
    assert!(target.file_list().is_some());
    assert!(target.file_path().ends_with("docs/api.md"));
}

#[tokio::test]
async fn test_resolve_route_target_api_contentはfile_listを含まない() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::api_content(Some("docs/api.md")))
        .await
        .unwrap();

    assert_eq!(target.relative_path(), Some("docs/api.md"));
    assert!(target.file_list().is_none());
}

#[tokio::test]
async fn test_resolve_route_target_api_content_起動後base差し替えを拒否する() {
    let parent = tempfile::tempdir().unwrap();
    let base = parent.path().join("workspace");
    let replacement = parent.path().join("replacement");
    std::fs::create_dir(&base).unwrap();
    std::fs::write(base.join("old.md"), "# old").unwrap();
    let state = create_directory_state(&base);
    std::fs::create_dir(&replacement).unwrap();
    std::fs::write(replacement.join("new.md"), "# new").unwrap();
    std::fs::remove_dir_all(&base).unwrap();
    std::fs::rename(&replacement, &base).unwrap();

    let error = resolve_route_target(&state, RouteTargetRequest::api_content(Some("new.md")))
        .await
        .expect_err("起動時と異なるbase実体の明示ファイル解決は拒否する");

    assert_eq!(error.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[cfg(unix)]
async fn test_load_route_update_解決後base差し替えでも差し替え後本文を読まない() {
    let parent = tempfile::tempdir().unwrap();
    let base = parent.path().join("workspace");
    let replacement = parent.path().join("replacement");
    std::fs::create_dir(&base).unwrap();
    std::fs::write(base.join("note.md"), "# harmless").unwrap();
    let state = create_directory_state(&base);
    std::fs::create_dir(&replacement).unwrap();
    std::fs::write(replacement.join("note.md"), "# secret").unwrap();

    let target = resolve_route_target(&state, RouteTargetRequest::api_content(Some("note.md")))
        .await
        .unwrap();
    let base_for_hook = base.clone();
    let replacement_for_hook = replacement.clone();
    let _guard = set_content_before_read_hook_for_test(std::sync::Arc::new(move |file_path| {
        if file_path.ends_with("note.md") {
            std::fs::remove_dir_all(&base_for_hook).unwrap();
            std::fs::rename(&replacement_for_hook, &base_for_hook).unwrap();
        }
    }));

    let update = load_route_update(&target, RouteTargetRequest::api_content(Some("note.md")))
        .await
        .expect("解決済みtargetは差し替え後のbaseではなく解決時の実体から読む");

    assert!(update.content().as_str().contains("harmless"));
    assert!(!update.content().as_str().contains("secret"));
}

#[tokio::test]
#[cfg(unix)]
async fn test_load_route_update_単一ファイル解決後差し替えでも差し替え後本文を読まない() {
    let parent = tempfile::tempdir().unwrap();
    let note = parent.path().join("note.md");
    let secret = parent.path().join("secret.md");
    std::fs::write(&note, "# harmless").unwrap();
    std::fs::write(&secret, "# secret").unwrap();
    let state = create_single_file_state(&note);

    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();
    let note_for_hook = note.clone();
    let secret_for_hook = secret.clone();
    let _guard = set_content_before_read_hook_for_test(std::sync::Arc::new(move |file_path| {
        if file_path.ends_with("note.md") {
            std::fs::remove_file(&note_for_hook).unwrap();
            std::os::unix::fs::symlink(&secret_for_hook, &note_for_hook).unwrap();
        }
    }));

    let update = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect("単一ファイルtargetも解決時の実体から読む");

    assert!(update.content().as_str().contains("harmless"));
    assert!(!update.content().as_str().contains("secret"));
}

#[tokio::test]
#[cfg(unix)]
async fn test_check_readable_before_render_解決済みhandleを優先する() {
    let parent = tempfile::tempdir().unwrap();
    let note = parent.path().join("note.md");
    std::fs::write(&note, "# harmless").unwrap();
    let state = create_single_file_state(&note);

    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();
    std::fs::remove_file(&note).unwrap();
    std::os::unix::fs::symlink("/nonexistent/secret.md", &note).unwrap();

    check_readable_before_render(&target)
        .await
        .expect("解決済みhandleがある場合はpath再openに依存しない");
}

#[tokio::test]
async fn test_resolve_route_target_api_memoはfile_listを含まない() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let explicit_target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("docs/api.md")))
            .await
            .unwrap();
    let default_target = resolve_route_target(&state, RouteTargetRequest::api_memo(None))
        .await
        .unwrap();

    assert_eq!(explicit_target.relative_path(), Some("docs/api.md"));
    assert!(explicit_target.file_list().is_none());
    assert!(default_target.file_list().is_none());
}

#[tokio::test]
async fn test_resolve_route_target_ディレクトリ既定ファイルを返す() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("z-last.md"), "# z").unwrap();
    std::fs::write(dir.path().join("README.md"), "# readme").unwrap();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();

    assert_eq!(target.relative_path(), Some("README.md"));
    assert!(target.file_path().ends_with("README.md"));
    assert_eq!(
        target.file_list().unwrap(),
        &["README.md".to_string(), "z-last.md".to_string()]
    );
}

#[tokio::test]
async fn test_resolve_route_target_page_queryなしではreadme不在時に先頭ファイルを選ぶ() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("z-last.md"), "# z").unwrap();
    std::fs::write(dir.path().join("a-first.md"), "# a").unwrap();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();

    assert_eq!(target.relative_path(), Some("a-first.md"));
    assert!(target.file_path().ends_with("a-first.md"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_update_単一ファイル解決後削除でも解決時本文を読む() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();
    std::fs::remove_file(&file_path).unwrap();

    let update = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect("解決済みtargetはpath削除後も解決時のfile handleから読む");

    assert!(update.content().as_str().contains("title"));
}

#[tokio::test]
async fn test_load_route_update_サイズ超過を413へ変換する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();

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
    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();

    let (status, body) = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect_err("invalid utf8 should map to api error");

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "このファイルはUTF-8テキストではありません");
}
