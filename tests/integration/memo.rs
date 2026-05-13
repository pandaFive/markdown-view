#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::time::Duration;

use tokio::sync::broadcast;

use markdown_view::server::BroadcastMessage;

use super::support::{setup_single_file_server, setup_single_file_server_from_path};

#[tokio::test]
async fn test_apiメモ_未作成時は空を返す() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;

    let resp = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["raw"], "");
    assert_eq!(json["html"], "");
    assert!(json.get("file").is_none());
}
#[tokio::test]
async fn test_apiメモ_保存と再取得ができる() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "> quote\n\n出典: [test.md](#memo) L1-L2"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    let saved: serde_json::Value = save.json().await.unwrap();
    assert_eq!(saved["raw"], "> quote\n\n出典: [test.md](#memo) L1-L2");
    assert!(saved["html"].as_str().unwrap().contains("<blockquote"));
    assert!(
        tokio::fs::try_exists(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap()
    );

    let get = client
        .get(format!("http://{}/api/memo", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let fetched: serde_json::Value = get.json().await.unwrap();
    assert_eq!(fetched["raw"], "> quote\n\n出典: [test.md](#memo) L1-L2");
}
#[tokio::test]
async fn test_apiメモ_put_raw欠落は422で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 422);
}
#[tokio::test]
async fn test_apiメモ_put_raw非文字列は422で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": 123
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 422);
}
#[tokio::test]
async fn test_apiメモ_保存成功後にtmpファイルが残らない() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "atomic memo"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    assert_eq!(
        tokio::fs::read_to_string(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap(),
        "atomic memo"
    );

    let mut entries = tokio::fs::read_dir(tmp_dir.path()).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.contains(".tmp."),
            "atomic temp file should be cleaned up: {name}"
        );
    }
}
#[tokio::test]
async fn test_apiメモ_保存成功時にmemo_updateをbroadcastする() {
    let (state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let mut rx = state.tx().subscribe();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "live memo"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);

    let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("memo update should be broadcast")
        .expect("broadcast receive should succeed");

    match received {
        BroadcastMessage::MemoUpdate(update) => {
            assert_eq!(update.file(), "test.md");
        }
        other => panic!("MemoUpdateメッセージを期待したが {:?} を受信", other),
    }
}
#[tokio::test]
async fn test_apiメモ_保存失敗時はmemo_updateをbroadcastしない() {
    let (state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let mut rx = state.tx().subscribe();
    let oversized = "a".repeat((markdown_view::server::MAX_FILE_SIZE as usize) + 1);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": oversized
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

    assert!(matches!(
        rx.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}
#[tokio::test]
async fn test_apiメモ_空白のみ保存で既存メモが削除される() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let memo_path = tmp_dir.path().join(".test.md.memo.md");

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "keep me"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    assert!(tokio::fs::try_exists(&memo_path).await.unwrap());

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "  \n  "
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);
    let deleted: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(deleted["raw"], "");
    assert_eq!(deleted["html"], "");
    assert!(!tokio::fs::try_exists(&memo_path).await.unwrap());

    let get = client
        .get(format!("http://{}/api/memo", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let fetched: serde_json::Value = get.json().await.unwrap();
    assert_eq!(fetched["raw"], "");
    assert_eq!(fetched["html"], "");
}
#[tokio::test]
async fn test_apiメモ_jsonエスケープで膨らんでも上限内rawなら保存できる() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let raw = "\\".repeat(markdown_view::server::MAX_FILE_SIZE as usize);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": raw
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    let saved: serde_json::Value = save.json().await.unwrap();
    assert_eq!(
        saved["raw"].as_str().unwrap().len(),
        markdown_view::server::MAX_FILE_SIZE as usize
    );
}
#[tokio::test]
async fn test_apiメモ_jsonボディ制限超過は413で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let escaped_raw = "\\\\".repeat(markdown_view::server::MAX_FILE_SIZE as usize);
    let padding = " ".repeat(4096 + 128);
    let body = format!("{{\"raw\":\"{}\"}}{}", escaped_raw, padding);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let body = save.text().await.unwrap();
    assert!(!body.contains("メモサイズが上限"));
}
#[tokio::test]
async fn test_apiメモ_10mb超過は413で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let raw = "a".repeat((markdown_view::server::MAX_FILE_SIZE as usize) + 1);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": raw
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["error"], "メモサイズが上限（10MB）を超えています");
}
#[tokio::test]
async fn test_apiメモ_getは旧保存先をそのまま読み込む() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy memo")
        .await
        .unwrap();

    let resp = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["raw"], "legacy memo");
    assert!(!tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(tokio::fs::try_exists(&legacy_memo_path).await.unwrap());
}
#[tokio::test]
async fn test_apiメモ_putは旧保存先から新sidecarへ自動移行する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy memo")
        .await
        .unwrap();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "updated memo"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);

    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "updated memo");
    assert!(tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(!tokio::fs::try_exists(&legacy_memo_path).await.unwrap());
}
#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_空白保存はunsafeなlegacyがあってもsidecar削除を優先する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::write(&sidecar_memo_path, "memo").await.unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    tokio::fs::create_dir_all(outside_dir.path().join("memos"))
        .await
        .unwrap();
    tokio::fs::write(outside_dir.path().join("memos/test.md"), "legacy")
        .await
        .unwrap();
    symlink(outside_dir.path(), tmp_dir.path().join(".markdown-view")).unwrap();

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "   \n"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(delete.status(), 200);
    let deleted: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(deleted["raw"], "");
    assert!(!tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(tokio::fs::try_exists(tmp_dir.path().join(".markdown-view"))
        .await
        .unwrap());

    let get = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let fetched: serde_json::Value = get.json().await.unwrap();
    assert_eq!(fetched["raw"], "");
}
#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_空白保存でsafe_legacyも通常削除する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::write(&sidecar_memo_path, "memo").await.unwrap();
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy").await.unwrap();

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "   \n"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(delete.status(), 200);
    let json: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(json["raw"], "");
    assert!(!tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(!tokio::fs::try_exists(&legacy_memo_path).await.unwrap());
}
#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_unsafeなlegacy_symlinkがあってもsidecar保存を継続できる() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let outside_dir = tempfile::tempdir().unwrap();
    symlink(outside_dir.path(), tmp_dir.path().join(".markdown-view")).unwrap();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "memo"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "memo");
    assert!(
        tokio::fs::try_exists(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap()
    );
}
#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_保存成功後にlegacyをsidecarへ移行して削除する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy memo")
        .await
        .unwrap();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "updated memo"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "updated memo");
    assert!(
        tokio::fs::try_exists(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap()
    );
    assert!(!tokio::fs::try_exists(&legacy_memo_path).await.unwrap());
}
#[tokio::test]
async fn test_apiメモ_長いファイル名でも短縮sidecarへ保存できる() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = tmp_dir.path().join(&file_name);
    tokio::fs::write(&file_path, "# Long").await.unwrap();
    let (_state, addr) = setup_single_file_server_from_path(&file_path).await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "memo"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "memo");
    let mut entries = tokio::fs::read_dir(tmp_dir.path()).await.unwrap();
    let mut memo_count = 0usize;
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".memo.md") {
            memo_count += 1;
            assert!(name.len() <= 255);
        }
    }
    assert_eq!(memo_count, 1);
}
#[tokio::test]
async fn test_apiメモ_長いファイル名で未作成時は空を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = tmp_dir.path().join(&file_name);
    tokio::fs::write(&file_path, "# Long").await.unwrap();
    let (_state, addr) = setup_single_file_server_from_path(&file_path).await;

    let resp = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["raw"], "");
    assert_eq!(json["html"], "");
}
#[tokio::test]
async fn test_apiメモ_長いファイル名のlegacyメモは空白保存で削除できる() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = tmp_dir.path().join(&file_name);
    tokio::fs::write(&file_path, "# Long").await.unwrap();
    let (_state, addr) = setup_single_file_server_from_path(&file_path).await;
    let client = reqwest::Client::new();
    let legacy_path = tmp_dir.path().join(".markdown-view/memos").join(&file_name);
    tokio::fs::create_dir_all(legacy_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_path, "memo").await.unwrap();

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": " \n "
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(delete.status(), 200);
    let json: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(json["raw"], "");
    assert!(!tokio::fs::try_exists(&legacy_path).await.unwrap());
}
#[tokio::test]
async fn test_indexページ取得_壊れたメモがあっても本文表示は継続する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::write(&memo_path, [0xff, 0xfe, 0xfd])
        .await
        .unwrap();

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("Body"));
    assert!(body.contains("id=\"memo-editor\""));
    assert!(body.contains("data-state=\"error\""));
    assert!(body.contains("内容を保護するため編集を無効化しています"));
    assert!(body.contains("本文の閲覧は継続できます"));
    assert!(body.contains("disabled"));
}
