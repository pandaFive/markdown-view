use super::support::{
    connect_ws, connect_ws_with_host, setup_dir_server, setup_single_file_server,
};

#[tokio::test]
async fn test_host_middlewareは全http_routeの不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Check").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    for path in [
        "/",
        "/api/content",
        "/api/memo",
        "/api/files",
        "/api/search?q=test",
    ] {
        let resp = client
            .get(format!("http://{}{}", addr, path))
            .header("Host", &attack_host)
            .send()
            .await
            .unwrap();

        assert_forbidden_with_security_headers(&resp);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert!(json["error"].as_str().is_some());
    }
}
#[tokio::test]
async fn test_host_middlewareはbody付きmemo_putもbody_limit前に不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Memo PUT").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());
    let oversized_raw = "x".repeat(21 * 1024 * 1024);
    let body = serde_json::json!({
        "raw": oversized_raw
    })
    .to_string();

    let resp = client
        .put(format!("http://{}/api/memo", addr))
        .header("Host", &attack_host)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .unwrap();

    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "許可されていないHostヘッダーです");
}
fn assert_forbidden_with_security_headers(resp: &reqwest::Response) {
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    assert_eq!(
        resp.headers()
            .get(reqwest::header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        resp.headers()
            .get(reqwest::header::X_FRAME_OPTIONS)
            .and_then(|value| value.to_str().ok()),
        Some("DENY")
    );
    assert!(resp
        .headers()
        .get(reqwest::header::CONTENT_SECURITY_POLICY)
        .and_then(|value| value.to_str().ok())
        .is_some());
}
#[tokio::test]
async fn test_apiメモ_getは不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Memo GET").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    let resp = client
        .get(format!("http://{}/api/memo", addr))
        .header("Host", &attack_host)
        .send()
        .await
        .unwrap();

    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().is_some());
}
#[tokio::test]
async fn test_apiメモ_putは不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Memo PUT").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    let resp = client
        .put(format!("http://{}/api/memo", addr))
        .header("Host", &attack_host)
        .json(&serde_json::json!({
            "raw": "blocked memo"
        }))
        .send()
        .await
        .unwrap();

    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().is_some());
}
#[tokio::test]
async fn test_websocketは異なるoriginを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let result = connect_ws(&url, "https://evil.example").await;
    assert!(result.is_err());
}
#[tokio::test]
async fn test_websocketはoriginポート不一致を拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let wrong_port_origin = format!("http://localhost:{}", addr.port() + 1);
    let result = connect_ws(&url, &wrong_port_origin).await;
    assert!(result.is_err());
}
#[tokio::test]
async fn test_websocketはhost_middlewareで不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Host Test").await;
    let client = reqwest::Client::new();

    let attack_host = format!("evil.example:{}", addr.port());
    let allowed_origin = format!("http://{}", addr);
    let resp = client
        .get(format!("http://{}/ws", addr))
        .header("Host", attack_host)
        .header("Origin", allowed_origin)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();

    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "許可されていないHostヘッダーです");
}
#[tokio::test]
async fn test_websocketはrebind相当のhost_origin一致を拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let rebinding_authority = format!("evil.example:{}", addr.port());
    let rebinding_origin = format!("http://{}", rebinding_authority);
    let result = connect_ws_with_host(&url, &rebinding_origin, Some(&rebinding_authority)).await;
    assert!(result.is_err());
}
#[tokio::test]
async fn test_セキュリティヘッダが設定されている() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test").await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();

    // X-Content-Type-Options
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );

    // X-Frame-Options
    assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");

    // Content-Security-Policy
    let csp = resp
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("script-src 'sha256-"));
    assert!(csp.contains("style-src 'sha256-"));
    assert!(csp.contains("img-src 'self'"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(csp.contains("object-src 'none'"));
    assert!(!csp.contains("script-src 'unsafe-inline'"));
    assert!(!csp.contains("style-src 'unsafe-inline'"));
    assert!(!csp.contains("data:"));
    assert!(
        resp.headers()
            .get("x-markdown-view-security-warning")
            .is_none(),
        "x-markdown-view-security-warning ヘッダーは廃止済みのため応答に含まれてはならない"
    );
}
#[tokio::test]
async fn test_ディレクトリモード_トラバーサル攻撃拒否() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=../../etc/passwd", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}
#[tokio::test]
async fn test_ディレクトリモード_メモapiのパストラバーサルを拒否する() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "file": "../../etc/passwd",
            "raw": "attack"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), reqwest::StatusCode::NOT_FOUND);

    let get = client
        .get(format!("http://{}/api/memo?file=../../etc/passwd", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), reqwest::StatusCode::NOT_FOUND);
}
#[tokio::test]
async fn test_ディレクトリモード_隠しファイルの直接アクセスが拒否される() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    // 隠しディレクトリ内のファイル
    let resp = reqwest::get(format!(
        "http://{}/api/content?file=.hidden/secret.md",
        addr
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    // index経由でも同様
    let resp = reqwest::get(format!("http://{}/?file=.hidden/secret.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}
#[tokio::test]
async fn test_ディレクトリモード_api_filesは不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    let resp = client
        .get(format!("http://{}/api/files", addr))
        .header("Host", &attack_host)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
