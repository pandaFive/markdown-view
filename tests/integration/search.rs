use super::support::{build_dir_state, setup_dir_server, spawn_test_server};

#[tokio::test]
async fn test_ディレクトリモード_検索apiは複数ファイルから結果を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();
    tokio::fs::create_dir_all(tmp_dir.path().join("docs"))
        .await
        .unwrap();
    tokio::fs::write(
        tmp_dir.path().join("docs/guide.md"),
        "# Guide\n\nAnother alpha note appears there.",
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let (addr, _server) = spawn_test_server(state).await;

    let resp = reqwest::get(format!("http://{}/api/search?q=alpha%20note", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["query"].as_str().unwrap(), "alpha note");
    assert_eq!(json["searched_files"].as_u64().unwrap(), 2);
    assert_eq!(json["skipped_files"].as_u64().unwrap(), 0);
    assert!(!json["truncated"].as_bool().unwrap());
    assert!(json["truncated_reasons"].as_array().unwrap().is_empty());
    assert_eq!(json["limits"]["max_results"].as_u64().unwrap(), 100);
    assert_eq!(json["limits"]["max_files"].as_u64().unwrap(), 1000);
    assert_eq!(
        json["limits"]["max_bytes"].as_u64().unwrap(),
        64 * 1024 * 1024
    );
    assert!(json["searched_bytes"].as_u64().unwrap() > 0);

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["file"].as_str().unwrap(), "README.md");
    assert_eq!(results[1]["file"].as_str().unwrap(), "docs/guide.md");
    assert!(results[0]["current"]
        .as_str()
        .unwrap()
        .contains("Alpha note appears here."));
}
#[tokio::test]
async fn test_ディレクトリモード_api_searchは長すぎるqueryを400で拒否する() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let (addr, _server) = spawn_test_server(state).await;
    let client = reqwest::Client::new();
    let query = "あ".repeat(257);

    let resp = client
        .get(format!("http://{}/api/search", addr))
        .query(&[("q", &query)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = resp.text().await.unwrap();
    assert!(!body.contains(&query));
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"].as_str(), Some("検索クエリが長すぎます"));
}
#[tokio::test]
async fn test_ディレクトリモード_api_searchは削除済みbaseでも長すぎるqueryを400で拒否する() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let (addr, _server) = spawn_test_server(state).await;
    tokio::fs::remove_dir_all(tmp_dir.path()).await.unwrap();
    let client = reqwest::Client::new();
    let query = "あ".repeat(257);

    let resp = client
        .get(format!("http://{}/api/search", addr))
        .query(&[("q", &query)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = resp.text().await.unwrap();
    assert!(!body.contains(&query));
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"].as_str(), Some("検索クエリが長すぎます"));
}
#[tokio::test]
async fn test_ディレクトリモード_api_searchはraw_query上限超過を400で拒否する() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let (addr, _server) = spawn_test_server(state).await;
    let query = "a".repeat(4097);

    let resp = reqwest::get(format!("http://{}/api/search?q={}", addr, query))
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = resp.text().await.unwrap();
    assert!(!body.contains(&query));
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"].as_str(), Some("検索クエリが長すぎます"));
}
#[tokio::test]
async fn test_ディレクトリモード_api_searchは不正percent_encodingを400で拒否する() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let (addr, _server) = spawn_test_server(state).await;
    let invalid_query = "%E0%A4%A";

    let resp = reqwest::get(format!("http://{}/api/search?q={}", addr, invalid_query))
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = resp.text().await.unwrap();
    assert!(!body.contains(invalid_query));
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"].as_str(), Some("検索クエリが不正です"));
}
#[tokio::test]
async fn test_ディレクトリモード_api_searchは結果数打ち切りをjsonで返す() {
    let dir = tempfile::tempdir().unwrap();
    let markdown = (0..120)
        .map(|index| format!("alpha note {index}."))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(dir.path().join("many.md"), markdown).unwrap();

    let state = build_dir_state(dir.path());
    let (addr, _server) = spawn_test_server(state).await;
    let resp = reqwest::get(format!("http://{}/api/search?q=alpha%20note", addr))
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["truncated"].as_bool().unwrap());
    assert_eq!(json["truncated_reasons"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["truncated_reasons"][0].as_str().unwrap(),
        "result_limit"
    );
    assert_eq!(json["results"].as_array().unwrap().len(), 100);
}
#[tokio::test]
async fn test_ディレクトリモード_検索apiは巨大ファイルをスキップする() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();
    tokio::fs::write(
        tmp_dir.path().join("large.md"),
        "a".repeat((10 * 1024 * 1024) + 1),
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let (addr, _server) = spawn_test_server(state).await;

    let resp = reqwest::get(format!("http://{}/api/search?q=alpha%20note", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["searched_files"].as_u64().unwrap(), 1);
    assert_eq!(json["skipped_files"].as_u64().unwrap(), 1);
    assert_eq!(json["results"].as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn test_ディレクトリモード_api_searchは不正hostを拒否する() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    let resp = client
        .get(format!("http://{}/api/search?q=readme", addr))
        .header("Host", &attack_host)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
