# IO エラー経路の統合テスト追加 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ReadMarkdownError::Io(_)` が HTTP `/api/content` と WebSocket 変更 broadcast の 2 経路で透過することを契約として固定する統合テスト 2 件を追加する。

**Architecture:** `chmod 0o000` で resolve をパスさせつつ `open(2)` を EACCES に落とすことで、(T1) `/api/content` への HTTP GET で 500 + JSON、(T2) watcher 経由ファイル変更で `BroadcastMessage::Error` を WebSocket で受信、をそれぞれ実サーバー経路で透過確認する。新規ヘルパー追加なし、既存テストパターンを踏襲する。

**Tech Stack:** Rust, axum, tokio, tokio-tungstenite, reqwest, tempfile, `std::os::unix::fs::PermissionsExt`

**Branch:** `test/io-error-integration-tests`（既に作成済み、設計書は commit `4079bc6`）

**Spec:** `docs/superpowers/specs/2026-04-26-io-error-integration-tests-design.md`

---

### Task 1: T1 — HTTP `/api/content` の IO エラー透過テスト追加

**Files:**
- Modify: `tests/integration_test.rs`（`test_存在しないファイル時は404を返す` の直前、L726 付近に挿入）

**前提知識（実装前に必ず確認）:**

- `setup_single_file_server_with_bytes(name, content) -> (Arc<AppState>, SocketAddr, TempDir, PathBuf)` ヘルパー（`tests/integration_test.rs:1715-1729`）
- `assert_json_error_for_paths(addr, paths, expected_status, expected_message)` ヘルパー：`Some(msg)` を渡すと `json["error"]` の **完全一致**を検証する（`tests/integration_test.rs:1731-1748`）
- 既存 `test_websocket_ioエラーでclose_frameが1011を返す`（L1650-1671）：chmod パターンの参照実装
- `ReadMarkdownError::Io(_) → status_code() = 500`（`src/server/files/content.rs:239`）と `user_message() = "ファイルの読み込みに失敗しました"`（同 L255）が既存実装に存在する

- [ ] **Step 1: T1 テストを追加する**

Edit: `tests/integration_test.rs` の `test_存在しないファイル時は404を返す` 関数定義（`async fn test_存在しないファイル時は404を返す()`）の **直前**に以下を挿入：

```rust
#[cfg(unix)]
#[tokio::test]
async fn test_api_content_io_エラーで500を返す() {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;

    let (_state, addr, _tmp_dir, file_path) =
        setup_single_file_server_with_bytes("unreadable.md", b"# content").await;

    // resolve (canonicalize/is_file) はパスし、open(2) のみが EACCES で失敗する状態を作る
    let original_mode = fs::metadata(&file_path).unwrap().permissions().mode();
    fs::set_permissions(&file_path, Permissions::from_mode(0o000)).unwrap();

    assert_json_error_for_paths(
        addr,
        &["/api/content"],
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        Some("ファイルの読み込みに失敗しました"),
    )
    .await;

    // teardown: TempDir drop で失敗しないよう権限を復元
    fs::set_permissions(&file_path, Permissions::from_mode(original_mode)).unwrap();
}
```

注意：ファイル先頭の `use std::os::unix::fs::PermissionsExt;`（L2）と `use std::{fs, os::unix::fs::symlink};`（L7）は既存。重複インポートにならないよう、**関数内 `use` のみ**で完結させる（既存 1011 テストと同パターン）。

- [ ] **Step 2: T1 テストを実行して PASS を確認する**

Run:
```bash
cargo test --test integration_test test_api_content_io_エラーで500を返す -- --nocapture
```

Expected: `test result: ok. 1 passed; 0 failed`

既存実装（`ReadMarkdownError::Io(_) → 500` マッピング）が正しいため初回から PASS する。失敗する場合は (a) root 実行（`0o000` でも読めてしまう）、(b) 実装の回帰、(c) ファイルパスのタイポを疑う。

- [ ] **Step 3: ミューテーション確認（推奨）**

テストが本当に回帰を捕まえることを手動で検証する。

Edit: `src/server/files/content.rs:239` を一時的に変更：

変更前:
```rust
            ReadMarkdownError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
```

変更後（ミューテーション）:
```rust
            ReadMarkdownError::Io(_) => StatusCode::OK,
```

Run:
```bash
cargo test --test integration_test test_api_content_io_エラーで500を返す -- --nocapture
```

Expected: FAIL — `assertion `left == right` failed` on status code（`200 OK` vs `500 INTERNAL_SERVER_ERROR`）

Edit: `src/server/files/content.rs:239` を元に戻す（`StatusCode::OK` → `StatusCode::INTERNAL_SERVER_ERROR`）。

Run:
```bash
cargo test --test integration_test test_api_content_io_エラーで500を返す -- --nocapture
```

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 4: clippy を実行する**

Run:
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: 0 errors, 0 warnings

- [ ] **Step 5: T1 をコミットする**

コミットメッセージファイルを作成（HEREDOC を `git commit -m` に渡すと commit-guard フックに英語と誤判定されるため、ファイル経由）：

```bash
cat > /tmp/commit_msg_t1.txt <<'MSG'
test: HTTP /api/content の IOエラー経路を統合テストでカバー

変更内容:
- tests/integration_test.rs に test_api_content_io_エラーで500を返す を追加
- chmod 0o000 で open(2) を EACCES に落として ReadMarkdownError::Io 経路を踏ませ、status 500 + JSON error 完全一致を検証

変更理由:
- ReadMarkdownError の Io arm が HTTP では 500 にマップされる契約を実サーバー経路で固定し、status_code() / IntoResponse / user_message() の silent な変更を検出可能にする

影響範囲:
- tests/integration_test.rs のみ（テスト追加）。実装コードに変更なし

テスト結果: cargo test --test integration_test test_api_content_io_エラー で 1 passed
MSG
git add tests/integration_test.rs
git commit -F /tmp/commit_msg_t1.txt
rm /tmp/commit_msg_t1.txt
```

Expected: 1 file changed, 約 22 行 insertion

---

### Task 2: T2 — ファイル変更経由 WebSocket Error broadcast の IO エラー透過テスト追加

**Files:**
- Modify: `tests/integration_test.rs`（`test_ファイル削除でwebsocketエラー通知` の直後、L828 付近に挿入）

**前提知識（実装前に必ず確認）:**

- `setup_single_file_server_from_path(file_path) -> (Arc<AppState>, SocketAddr)` ヘルパー（`tests/integration_test.rs:1707-1713`）
- `WatchService::start(state) -> Result<WatchService, _>` と `watch_service.shutdown().await`：既存 `test_ファイル変更でwebsocket更新`（L768-800）参照
- `connect_ws(url, origin)` と `next_ws_message(read)`：既存 watcher 系 WS テスト群で使用
- `build_change_broadcast_message` の `ReadFailed` arm（`src/server/files/content.rs:190-201`）が出力する文字列フォーマット：
  ```
  ファイル読み込みエラー (<file_label>): <user_message>
  ```
  例：`"ファイル読み込みエラー (watch_io_error.md): ファイルの読み込みに失敗しました"`
- 既存 `test_ファイル削除でwebsocketエラー通知`（L803-829）：`ResolveFailed` arm の参照実装。本テストはこれの `ReadFailed` 版

- [ ] **Step 1: T2 テストを追加する**

Edit: `tests/integration_test.rs` の `test_ファイル削除でwebsocketエラー通知` 関数の閉じ括弧 `}`（`watch_service.shutdown().await;` の直後、L828 付近の `}` の後）に、空行を挟んで以下を挿入：

```rust
#[cfg(unix)]
#[tokio::test]
async fn test_ファイル変更_io_エラーでwebsocketエラー通知() {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;

    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("watch_io_error.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;
    let watch_service = WatchService::start(state.clone()).await.unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 初期メッセージを消費
    let _initial_message = next_ws_message(&mut read).await;

    // notify 発火 → debounce window 内に chmod 0o000 で open(2) を EACCES に落とす
    let original_mode = fs::metadata(&file_path).unwrap().permissions().mode();
    tokio::fs::write(&file_path, "# After").await.unwrap();
    fs::set_permissions(&file_path, Permissions::from_mode(0o000)).unwrap();

    // debounce 300ms 後に build_change_broadcast_message が走り、Io arm が Error broadcast を発信
    let msg = next_ws_message(&mut read).await;
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let error = json["error"].as_str().expect("errorフィールドが存在する");
    assert!(
        error.contains("ファイル読み込みエラー"),
        "ReadFailed arm の prefix を期待: {}",
        error
    );
    assert!(
        error.contains("watch_io_error.md"),
        "ファイル名が含まれるべき: {}",
        error
    );
    assert!(
        error.contains("ファイルの読み込みに失敗しました"),
        "user_message() Io arm 文言が含まれるべき: {}",
        error
    );

    // teardown
    fs::set_permissions(&file_path, Permissions::from_mode(original_mode)).unwrap();
    watch_service.shutdown().await;
    drop(tmp_dir);
}
```

注意：

1. `state.clone()` は `Arc<AppState>` のクローンで安価
2. `tokio::fs::write` と `fs::set_permissions` の間に**何も挟まない**（race window 設計の核）
3. 権限復元は `watch_service.shutdown()` より先に行う（spec セクション 3 の決定事項）
4. `drop(tmp_dir)` を最後に明示する（既存 `test_ファイル変更でwebsocket更新` と同パターン）

- [ ] **Step 2: T2 テストを実行して PASS を確認する**

Run:
```bash
cargo test --test integration_test test_ファイル変更_io_エラーでwebsocketエラー通知 -- --nocapture
```

Expected: `test result: ok. 1 passed; 0 failed`

既存実装が正しいため初回から PASS する。失敗する場合：

- `Step 1` のコード貼り付けミス（特に `state.clone()` の引数順）
- `tokio::fs::write` の完了が異常に遅く（>300ms）chmod が debounce window 外になった → タイムアウトで明示失敗
- root 実行で `0o000` が無効

- [ ] **Step 3: ミューテーション確認（推奨）**

T2 が回帰を捕まえることを手動検証。

Edit: `src/server/files/content.rs:255` を一時的に変更：

変更前:
```rust
            ReadMarkdownError::Io(_) => "ファイルの読み込みに失敗しました".to_string(),
```

変更後（ミューテーション、文言改変）:
```rust
            ReadMarkdownError::Io(_) => "別の文言".to_string(),
```

Run:
```bash
cargo test --test integration_test test_ファイル変更_io_エラーでwebsocketエラー通知 -- --nocapture
```

Expected: FAIL — `user_message() Io arm 文言が含まれるべき: ファイル読み込みエラー (watch_io_error.md): 別の文言` のような assertion 失敗

Edit: `src/server/files/content.rs:255` を元に戻す。

Run:
```bash
cargo test --test integration_test test_ファイル変更_io_エラーでwebsocketエラー通知 -- --nocapture
```

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 4: clippy を実行する**

Run:
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: 0 errors, 0 warnings

- [ ] **Step 5: T2 をコミットする**

```bash
cat > /tmp/commit_msg_t2.txt <<'MSG'
test: ファイル変更経由のWebSocket IOエラー broadcast 経路を統合テストでカバー

変更内容:
- tests/integration_test.rs に test_ファイル変更_io_エラーでwebsocketエラー通知 を追加
- ファイル更新で notify 発火 → 直後に chmod 0o000 で open(2) を EACCES に落とし、build_change_broadcast_message の ReadFailed arm が BroadcastMessage::Error として WebSocket クライアントへ透過することを 3 軸 (経路マーカー / ファイル名 / Io arm 文言) で検証

変更理由:
- ReadMarkdownError の Io arm が watcher → forwarder → broadcast → WebSocket の透過経路を抜けて到達する契約を実サーバーで固定し、ResolveFailed arm への誤吸収・ファイル名漏れ・他 arm への誤マップを別々に検出可能にする

影響範囲:
- tests/integration_test.rs のみ（テスト追加）。実装コードに変更なし

テスト結果: cargo test --test integration_test test_ファイル変更_io_エラー で 1 passed
MSG
git add tests/integration_test.rs
git commit -F /tmp/commit_msg_t2.txt
rm /tmp/commit_msg_t2.txt
```

Expected: 1 file changed, 約 53 行 insertion

---

### Task 3: TODO.md 更新と全体検証

**Files:**
- Modify: `docs/todo/TODO.md`（L69-79 の対象 2 項目を `- [x]` にチェック）

- [ ] **Step 1: TODO.md の対象 2 項目を完了マークに更新する**

Edit: `docs/todo/TODO.md`

変更前（L69）:
```
- [ ] HTTP `/api/content` の IO エラー経路 (500) の統合テストを追加
```
変更後:
```
- [x] HTTP `/api/content` の IO エラー経路 (500) の統合テストを追加
```

変更前（L75）:
```
- [ ] `build_change_broadcast_message` / `build_lagged_recovery_message` の IO エラー経路を統合テストでカバー
```
変更後:
```
- [x] `build_change_broadcast_message` / `build_lagged_recovery_message` の IO エラー経路を統合テストでカバー
```

注意：(5)(b) `build_lagged_recovery_message` 側はスコープ外として「将来の宿題」に格上げしたが、TODO 項目自体は (a) のみで完了扱いとする。本来の項目本文に「(a) のみ検討」と明記されており、(a) のカバーで TODO 項目の意図は満たされる。

- [ ] **Step 2: 既存テストの回帰を確認する**

Run:
```bash
cargo test --test integration_test test_websocket_ioエラー -- --nocapture
cargo test --test integration_test test_ファイル削除 -- --nocapture
cargo test --test integration_test test_ファイル変更でwebsocket更新 -- --nocapture
```

Expected: それぞれ `1 passed`（既存 IO エラー透過テストが新規テストと共存して壊れていないこと）

- [ ] **Step 3: 全体検証を実行する**

Run:
```bash
./verify.sh
```

Expected: フォーマット・リント・cargo test 全件・E2E 型チェックが全て PASS

`./verify.sh` 内訳：
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `npm run typecheck`

E2E (`npm run test:e2e`) は `verify.sh` には含まれない（既存運用）。本変更は E2E 影響なし。

- [ ] **Step 4: TODO.md 更新をコミットする**

```bash
cat > /tmp/commit_msg_todo.txt <<'MSG'
docs: TODO.md の IOエラー経路統合テスト 2 項目を完了マークに更新

変更内容:
- docs/todo/TODO.md の HTTP /api/content の IO エラー経路 (500) 項目を [x] に
- docs/todo/TODO.md の build_change_broadcast_message / build_lagged_recovery_message の IO エラー経路項目を [x] に
- build_lagged_recovery_message 側は spec で「将来の宿題」へ格上げ。本来の TODO 項目本文に「(a) のみ検討」と明記されているため、本項目は (a) のカバーで完了扱いとする

変更理由:
- T1 / T2 統合テストの追加完了に伴う TODO 進捗の同期

影響範囲:
- docs/todo/TODO.md のみ

テスト結果: ./verify.sh で全件 pass
MSG
git add docs/todo/TODO.md
git commit -F /tmp/commit_msg_todo.txt
rm /tmp/commit_msg_todo.txt
```

Expected: 1 file changed, 2 insertions(+), 2 deletions(-)

---

## Self-Review

**1. Spec coverage:**

| Spec 要件 | カバーするタスク |
|---|---|
| §やること 1: T1 `test_api_content_io_エラーで500を返す` | Task 1 全 step |
| §やること 2: T2 `test_ファイル変更_io_エラーでwebsocketエラー通知` | Task 2 全 step |
| §やること 3: 新規ヘルパー追加なし | Task 1, 2 共に既存ヘルパーのみ使用、確認済み |
| §設計方針 IO エラー誘発手段 (chmod 0o000) | Task 1 Step 1, Task 2 Step 1 のコード本体 |
| §設計方針 race 戦略 (write→chmod スリープ無し) | Task 2 Step 1 注意書き 2 項目 |
| §設計方針 アサーション粒度 (T1 完全一致 / T2 3 軸 contains) | Task 1 Step 1 の `Some(...)` 引数, Task 2 Step 1 の 3 つの assert! |
| §設計方針 配置 | Task 1 Files 欄, Task 2 Files 欄 |
| §設計方針 プラットフォームガード (`#[cfg(unix)]`) | 両テストともコード冒頭 |
| §設計方針 Single-file モード | 両テストとも `setup_single_file_server_*` 使用 |
| §成功基準 (1) T1 PASS | Task 1 Step 2 |
| §成功基準 (2) T2 PASS | Task 2 Step 2 |
| §成功基準 (3) 既存テスト回帰なし | Task 3 Step 2 |
| §成功基準 (4) clippy PASS | Task 1 Step 4, Task 2 Step 4, Task 3 Step 3 (verify.sh 内) |
| §成功基準 (5) verify.sh PASS | Task 3 Step 3 |
| §成功基準 (6) TODO.md チェック | Task 3 Step 1, 4 |

ギャップなし。

**2. Placeholder scan:**

- "TBD" / "TODO" / "implement later" / "fill in details" → 検索ヒット 0
- "Add appropriate error handling" / "handle edge cases" → 検索ヒット 0
- 「Similar to Task N」省略 → なし、各タスクが完全なコード片を含む

**3. Type consistency:**

- T1 と T2 の `chmod` パターン：両者とも `original_mode` を `metadata().permissions().mode()` で取り、teardown で `Permissions::from_mode(original_mode)` を渡す。一致
- T1 の `assert_json_error_for_paths` シグネチャ：spec の実装スケッチと完全一致（4 引数、`Some(&str)`）
- T2 の `setup_single_file_server_from_path` 戻り値：`(Arc<AppState>, SocketAddr)` の 2-tuple、spec と一致
- T2 の `WatchService::start(state.clone()).await.unwrap()`：既存 `test_ファイル変更でwebsocket更新` の呼び出しと一致

問題なし。

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-26-io-error-integration-tests.md`.

実行方法を 2 つから選んでください：

1. **Subagent-Driven（推奨）** — 各タスクをフレッシュなサブエージェントが実装し、タスク間で私がレビュー。高速反復、コンテキスト分離
2. **Inline Execution** — このセッションで `executing-plans` スキルを使い、チェックポイント付きでバッチ実行

どちらにしますか？
