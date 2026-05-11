# Search Query Length Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/api/search` の `q` を trim 後 256 文字で制限し、超過時は検索処理へ進まず `400 Bad Request` を返す。

**Architecture:** 検証責務は `src/server/files/search.rs` に集約し、`src/server/files/mod.rs` 経由で `service::search` から呼ぶ。`service::search` は単一ファイル/ディレクトリ分岐前に検証済み query を作り、`InvalidInput` だけを HTTP 400 に変換する。`routes.rs` は query 抽出と service 呼び出しだけを維持する。

**Tech Stack:** Rust, axum, tokio, reqwest integration tests, cargo test, `./verify.sh`

---

## File Structure

- Modify: `src/server/files/search.rs`
  - `MAX_SEARCH_QUERY_CHARS` と `normalize_search_query` を追加する。
  - `search_directory_with_limits_blocking` の入口で同じ検証を使う。
  - 検索実装を直接呼ぶ内部経路向けの単体テストを追加する。
- Modify: `src/server/files/mod.rs`
  - `normalize_search_query` と `MAX_SEARCH_QUERY_CHARS` を `pub(in crate::server)` で再公開する。
- Modify: `src/server/service.rs`
  - `search` で単一ファイル/ディレクトリ分岐前に query を検証する。
  - `InvalidInput` を `400 Bad Request` の JSON エラーへ変換する。
  - service 単体テストで単一ファイルモードの長大 query 拒否を固定する。
- Modify: `tests/integration_test.rs`
  - `/api/search` の HTTP 境界で、単一ファイルモードとディレクトリモードの長大 query が `400` になることを確認する。
- No change: `src/server/routes.rs`
  - handler は `SearchQuery` 抽出と `service::search` 呼び出しだけを続ける。

---

### Task 1: Search 実装側の query guard をTDDで追加する

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: 長大 query を拒否する failing unit test を追加する**

`src/server/files/search.rs` の既存 `#[cfg(test)] mod tests` 内、`test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない` の後に追加する。この時点では `MAX_SEARCH_QUERY_CHARS` がまだ未定義なので compile failure になる。

```rust
    #[test]
    fn test_search_directory_queryが上限を超えるとinvalid_inputを返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        let canonical = canonical_of(dir.path());
        let query = "あ".repeat(MAX_SEARCH_QUERY_CHARS + 1);

        let error = search_directory_with_limits_blocking(
            &canonical,
            &query,
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
```

- [ ] **Step 2: failing test を確認する**

Run:

```bash
cargo test --all-targets --all-features test_search_directory_queryが上限を超えるとinvalid_inputを返す
```

Expected: FAIL with `cannot find value MAX_SEARCH_QUERY_CHARS in this scope`.

- [ ] **Step 3: 256文字境界と空 query の unit test を追加する**

同じ `tests` module に続けて追加する。

```rust
    #[test]
    fn test_search_directory_queryはtrim後256文字まで許可する() {
        let dir = tempfile::tempdir().unwrap();
        let query = "あ".repeat(MAX_SEARCH_QUERY_CHARS);
        std::fs::write(dir.path().join("README.md"), format!("{query} found")).unwrap();
        let canonical = canonical_of(dir.path());

        let response = search_directory_with_limits_blocking(
            &canonical,
            &query,
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
        )
        .unwrap();

        assert_eq!(response.query, query);
        assert_eq!(response.results.len(), 1);
    }

    #[test]
    fn test_search_directory_queryはtrim後空なら空結果を返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        let canonical = canonical_of(dir.path());

        let response = search_directory_with_limits_blocking(
            &canonical,
            "   ",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
        )
        .unwrap();

        assert_eq!(response.query, "");
        assert_eq!(response.searched_files, 0);
        assert!(response.results.is_empty());
    }
```

- [ ] **Step 4: search query 検証関数を実装する**

`MAX_SEARCH_BYTES` の直後に追加する。

```rust
pub(in crate::server) const MAX_SEARCH_QUERY_CHARS: usize = 256;
const SEARCH_QUERY_TOO_LONG_MESSAGE: &str = "検索クエリが長すぎます";
```

`SearchContext` の直後、`search_directory` の前に追加する。

```rust
pub(in crate::server) fn normalize_search_query(raw_query: &str) -> std::io::Result<String> {
    let query = raw_query.trim().to_string();
    if query.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(search_query_too_long_error());
    }
    Ok(query)
}

fn search_query_too_long_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        SEARCH_QUERY_TOO_LONG_MESSAGE,
    )
}
```

`search_directory_with_limits_blocking` の query 作成を置き換える。

```rust
    let query = normalize_search_query(raw_query)?;
    if query.is_empty() {
        return Ok(SearchResponse::empty(query));
    }
```

- [ ] **Step 5: search unit tests を通す**

Run:

```bash
cargo test --all-targets --all-features search_directory_query
```

Expected: 追加した3テストが PASS。

- [ ] **Step 6: Task 1 をコミットする**

```bash
git add src/server/files/search.rs
git commit -m "test: 検索query長guardを検索実装で固定"
```

---

### Task 2: Service 境界で 400 mapping を追加する

**Files:**
- Modify: `src/server/files/mod.rs`
- Modify: `src/server/service.rs`

- [ ] **Step 1: service の failing unit test を追加する**

`src/server/service.rs` の `test_search_単一ファイルモードでは空結果を返す` の後に追加する。

```rust
    #[tokio::test]
    async fn test_search_単一ファイルモードでも長すぎるqueryはbad_request() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("note.md");
        std::fs::write(&file_path, "# Note\n\nneedle").unwrap();
        let state = create_single_file_state(&file_path);
        let query = "あ".repeat(crate::server::files::MAX_SEARCH_QUERY_CHARS + 1);

        let error = search(&state, query)
            .await
            .expect_err("長すぎる検索queryは単一ファイルモードでも拒否する");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            error.1["error"].as_str(),
            Some("検索クエリが長すぎます")
        );
    }
```

- [ ] **Step 2: failing test を確認する**

Run:

```bash
cargo test --all-targets --all-features test_search_単一ファイルモードでも長すぎるqueryはbad_request
```

Expected: FAIL because single-file mode still returns `Ok(SearchResponse::empty(...))`.

- [ ] **Step 3: `files/mod.rs` で検証関数と定数を再公開する**

既存の search 再公開行を置き換える。

```rust
pub(in crate::server) use self::search::{
    normalize_search_query, search_directory, SearchResponse, MAX_SEARCH_QUERY_CHARS,
};
```

- [ ] **Step 4: `service.rs` の import を更新する**

`use super::files::{ ... }` に `normalize_search_query` を追加する。

```rust
use super::files::{
    list_markdown_files_from_canonical_base, load_route_memo, load_route_update,
    normalize_search_query, resolve_route_target, run_blocking_file_task, save_route_memo,
    search_directory, ResolvedTarget, RouteTargetRequest, SearchResponse, MAX_FILE_LIST,
};
```

- [ ] **Step 5: service 用の invalid input mapper を追加する**

`list_files` と `search` の間に追加する。

```rust
fn map_search_error(error: std::io::Error) -> ApiError {
    if error.kind() == std::io::ErrorKind::InvalidInput {
        return json_error(StatusCode::BAD_REQUEST, "検索クエリが長すぎます");
    }

    tracing::warn!("[markdown-view] ディレクトリ検索エラー: {}", error);
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "ディレクトリ検索に失敗しました",
    )
}
```

- [ ] **Step 6: `service::search` を検証済み query に変更する**

既存の `search` 関数全体を置き換える。

```rust
/// ディレクトリモードの全文検索を実行する。単一ファイルモードでは空結果を返す。
pub(super) async fn search(state: &AppState, query: String) -> Result<SearchResponse, ApiError> {
    let query = normalize_search_query(&query).map_err(map_search_error)?;
    let Some(base_dir) = state.mode().directory_canonical() else {
        return Ok(SearchResponse::empty(query));
    };

    search_directory(base_dir, &query).await.map_err(map_search_error)
}
```

- [ ] **Step 7: service unit tests を通す**

Run:

```bash
cargo test --all-targets --all-features test_search_
```

Expected: `test_search_ディレクトリモードでは検索結果を返す`、`test_search_単一ファイルモードでは空結果を返す`、追加テストが PASS。

- [ ] **Step 8: Task 2 をコミットする**

```bash
git add src/server/files/mod.rs src/server/service.rs
git commit -m "fix: 検索query長超過をservice境界で400にする"
```

---

### Task 3: HTTP integration tests を追加する

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: 単一ファイルモードの HTTP failing test を追加する**

`test_単一ファイルモードの後方互換_api_searchは空結果` の後に追加する。

```rust
#[tokio::test]
async fn test_単一ファイルモード_api_searchは長すぎるqueryを400で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test").await;
    let client = reqwest::Client::new();
    let query = "あ".repeat(257);

    let resp = client
        .get(format!("http://{}/api/search", addr))
        .query(&[("q", &query)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"].as_str(), Some("検索クエリが長すぎます"));
}
```

- [ ] **Step 2: ディレクトリモードの HTTP failing test を追加する**

`test_ディレクトリモード_検索apiは複数ファイルから結果を返す` の後に追加する。

```rust
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
    let addr = spawn_test_server(state).await;
    let client = reqwest::Client::new();
    let query = "あ".repeat(257);

    let resp = client
        .get(format!("http://{}/api/search", addr))
        .query(&[("q", &query)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"].as_str(), Some("検索クエリが長すぎます"));
}
```

- [ ] **Step 3: integration tests を実行する**

Run:

```bash
cargo test --all-targets --all-features api_searchは長すぎるquery
```

Expected: 追加した2テストが PASS。

- [ ] **Step 4: query 本文が error に含まれないことを明示する**

2つの追加テストの JSON assertion の直後に次を追加する。

```rust
    assert!(!json["error"].as_str().unwrap().contains(&query));
```

- [ ] **Step 5: integration tests を再実行する**

Run:

```bash
cargo test --all-targets --all-features api_searchは長すぎるquery
```

Expected: 追加した2テストが PASS。

- [ ] **Step 6: Task 3 をコミットする**

```bash
git add tests/integration_test.rs
git commit -m "test: api searchの長大query拒否を統合テストで固定"
```

---

### Task 4: Full verification と追跡更新

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: 追跡ファイルに issue 142 があるか確認する**

Run:

```bash
rg -n "#142|api/search|query 長|検索クエリ" docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: `docs/todo/TODO.md:19:- [ ] \`/api/search\` のクエリ長ガードを routes.rs 側に追加する` が表示される。

- [ ] **Step 2: TODO の issue 142 ブロックを削除する**

`docs/todo/TODO.md` の High Priority から、次のブロック全体だけを削除する。他の TODO/BACKLOG 項目は並べ替えない。

```markdown
- [ ] `/api/search` のクエリ長ガードを routes.rs 側に追加する
  - ファイル: `src/server/routes.rs` L203-209, `src/server/service.rs` L195-208, `src/server/files/search.rs`
  - 現状: `api_search_handler` はクエリ `q` を長さチェックせず `service::search` に渡し、`service::search` は `search_directory` へ委譲する。検索実装は `raw_query.trim().to_string()` で照合・レスポンス用 query を作るが、route/service 境界では極端に長い `q`（例: 1MB）を拒否しない
  - 対応: 1KB 程度の長さガードを `routes.rs` または `service::search` の入口に追加し、超過時は 400 を返す。`search.rs` 内部にも防御を残す（defense in depth）
  - 昇格理由: 極端に長い未信頼入力が検索処理とレスポンス生成に流れるため、resource exhaustion と API 入力境界に関わる
  - 由来: アーキテクチャレビュー (2026-04-30)
```

- [ ] **Step 3: フォーマットを確認する**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS. On formatting failure, run the exact command below, inspect `git diff`, and rerun `cargo fmt --all -- --check`.

```bash
cargo fmt --all
```

- [ ] **Step 4: lint を確認する**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: 全Rustテストを確認する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 6: required verification を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 7: 最終差分を確認する**

Run:

```bash
git status --short
git diff --stat HEAD
```

Expected: 意図したファイルだけが変更されている。query 本文をログやエラーへ含める変更がない。

- [ ] **Step 8: Task 4 をコミットする**

Commit the TODO update:

```bash
git add docs/todo/TODO.md
git commit -m "docs: issue 142の完了状態を更新"
```

---

## Security Review Checklist

- [ ] 長大 query は単一ファイルモードでもディレクトリモードでも `400` になる。
- [ ] 長大 query 拒否時、ディレクトリ列挙や Markdown 読み込みへ進まない。
- [ ] エラー JSON と warn log に query 本文を含めない。
- [ ] Host/Origin validation、CSP、HTML sanitization、path validation、file size limit は変更しない。
- [ ] 成功時の `SearchResponse` JSON shape は変更しない。

## Rollback

実装コミットを逆順に revert する。最小 rollback が必要な場合は、`service::search` の `normalize_search_query` 呼び出しと `map_search_error` の `InvalidInput` mapping を戻し、`search.rs` の `normalize_search_query` guard 呼び出しを外す。
