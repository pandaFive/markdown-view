# BACKLOG P1 Contract Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** BACKLOG P1 の低リスク契約整理として、Update JSON fallback、memo read size limit 契約、WebSocket select cancel-safe コメントを実装する。

**Architecture:** 既存の `BroadcastMessage`、`MemoFs`、WebSocket session 境界を保ち、責務を各モジュール内で明確化する。ユーザー可視の WebSocket / HTTP / HTML 契約は変えず、テストとコメントで将来改修時の安全境界を固定する。

**Tech Stack:** Rust, Tokio, axum, serde_json, tracing, cargo test, `./verify.sh`

---

## File Structure

- Modify: `src/server/messages.rs`
  - `BroadcastMessage::Update` の通常 JSON 契約をテストで固定する。
  - Update fallback helper を追加し、fallback payload を valid JSON としてテストする。
  - `to_json()` の Update arm で直列化失敗時に `tracing::error!` を残し、fallback JSON を返す。
- Modify: `src/server/files/memo_fs.rs`
  - `MemoFs::read_with_limit` の doc コメントを具体化する。
  - `TokioMemoFs::read_with_limit` の上限超過契約を module test で固定する。
- Modify: `src/server/files/memo.rs`
  - `read_memo_file_if_present` の読み込み後 `bytes.len()` 重複チェックを削除する。
  - 既存の `MemoReadError::TooLarge` 経由の API error 変換を維持する。
- Modify: `src/server/session.rs`
  - `tokio::select!` の直前に cancel-safe 前提コメントを追加する。
- Test: `src/server/messages.rs` module tests, `src/server/files/memo_fs.rs` module tests, existing `src/server/files/tests/memo_route.rs` and integration tests.

## Task 1: Update JSON fallback 契約を固定する

**Files:**
- Modify: `src/server/messages.rs`
- Test: `src/server/messages.rs`

- [ ] **Step 1: 通常 Update JSON の既存契約テストを追加する**

`src/server/messages.rs` の `#[cfg(test)] mod tests` に次のテストを追加する。既存 `test_broadcast_message_lagged_recoveryのjson直列化` の前後に置く。

```rust
    #[test]
    fn test_broadcast_message_updateのjson直列化はcontent_toc_fileを維持する() {
        let json = BroadcastMessage::Update(UpdateMessage::new(
            crate::renderer::render_markdown("# title"),
            crate::toc::generate_toc("# title"),
            Some("docs/guide.md".to_string()),
        ))
        .to_json()
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value["content"].as_str().unwrap().contains("title"));
        assert!(value["toc"].as_str().unwrap().contains("title"));
        assert_eq!(value["file"], "docs/guide.md");
        assert!(value.get("refresh").is_none());
        assert!(value.get("error").is_none());
    }
```

- [ ] **Step 2: テストを実行し、現状の通常契約が通ることを確認する**

Run:

```bash
cargo test server::messages::tests::test_broadcast_message_updateのjson直列化はcontent_toc_fileを維持する
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 3: fallback helper のテストを追加する**

同じ test module に次のテストを追加する。

```rust
    #[test]
    fn test_update_fallback_jsonは空content_tocだけを返す() {
        let json = update_fallback_json();
        let value: serde_json::Value = serde_json::from_str(json).unwrap();

        assert_eq!(value["content"], "");
        assert_eq!(value["toc"], "");
        assert!(value.get("file").is_none());
        assert!(value.get("refresh").is_none());
        assert!(value.get("error").is_none());
    }
```

- [ ] **Step 4: fallback helper 未定義で失敗することを確認する**

Run:

```bash
cargo test server::messages::tests::test_update_fallback_jsonは空content_tocだけを返す
```

Expected: FAIL with `cannot find function update_fallback_json`

- [ ] **Step 5: fallback helper を実装する**

`impl BroadcastMessage` の直前または直後に、module-private helper を追加する。

```rust
fn update_fallback_json() -> &'static str {
    r#"{"content":"","toc":""}"#
}
```

- [ ] **Step 6: fallback helper テストが通ることを確認する**

Run:

```bash
cargo test server::messages::tests::test_update_fallback_jsonは空content_tocだけを返す
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 7: `BroadcastMessage::to_json()` の Update arm を fallback 経路へ変更する**

`src/server/messages.rs` の `BroadcastMessage::to_json()` を次の形へ変更する。

```rust
    pub(super) fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            BroadcastMessage::Update(update) => {
                Ok(serde_json::to_string(update).unwrap_or_else(|error| {
                    tracing::error!(
                        "[markdown-view] UpdateメッセージJSON化失敗。空updateへfallbackします: {}",
                        error
                    );
                    update_fallback_json().to_string()
                }))
            }
            BroadcastMessage::MemoUpdate(update) => serde_json::to_string(update),
            BroadcastMessage::LaggedRecovery(message) => serde_json::to_string(message),
            BroadcastMessage::Refresh => serde_json::to_string(&serde_json::json!({
                "refresh": true,
                "memo_refresh": true
            })),
            BroadcastMessage::Error(message) => serde_json::to_string(&error_message_json(message)),
        }
    }
```

この変更で `Update` arm は `Ok` を常に返す。ほかの variant は既存どおり `serde_json::Error` を呼び出し側へ返す。

- [ ] **Step 8: messages tests を実行する**

Run:

```bash
cargo test server::messages
```

Expected: `test result: ok`

- [ ] **Step 9: 変更をコミットする**

```bash
git add src/server/messages.rs
git commit -m "fix: Updateメッセージのfallback契約を明示"
```

## Task 2: memo read size limit 契約を `MemoFs` 側で固定する

**Files:**
- Modify: `src/server/files/memo_fs.rs`
- Test: `src/server/files/memo_fs.rs`

- [ ] **Step 1: `TokioMemoFs::read_with_limit` の上限超過テストを追加する**

`src/server/files/memo_fs.rs` の末尾に `#[cfg(test)] mod tests` が既にある場合はそこへ追加する。ない場合は末尾に次を追加する。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::files::MAX_FILE_SIZE;

    #[tokio::test]
    async fn test_tokio_memo_fs_read_with_limitは実読み取り上限超過をtoo_largeにする() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".README.md.memo.md");
        tokio::fs::write(&path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
            .await
            .unwrap();

        let result = TokioMemoFs.read_with_limit(&path).await;

        assert!(matches!(result, Err(MemoReadError::TooLarge)));
    }
}
```

既存 test module がある場合は、`use super::*;` と `use crate::server::files::MAX_FILE_SIZE;` が重複しないよう統合する。

- [ ] **Step 2: テストを実行し、既存実装で通ることを確認する**

Run:

```bash
cargo test server::files::memo_fs::tests::test_tokio_memo_fs_read_with_limitは実読み取り上限超過をtoo_largeにする
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 3: `MemoFs::read_with_limit` の doc コメントを具体化する**

`src/server/files/memo_fs.rs` の trait method コメントを次の内容へ置き換える。

```rust
    /// バイト列読み込み。
    ///
    /// 呼び出し側の metadata 事前チェック後にファイルが増える TOCTOU へ備え、
    /// 実読み取り量が `MAX_FILE_SIZE` を超えた場合は `MemoReadError::TooLarge`
    /// を返す。成功時に返す `Vec<u8>` は上限以下である。
    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError>;
```

- [ ] **Step 4: memo_fs のテストを実行する**

Run:

```bash
cargo test server::files::memo_fs
```

Expected: `test result: ok`

- [ ] **Step 5: 変更をコミットする**

```bash
git add src/server/files/memo_fs.rs
git commit -m "test: メモ読込上限契約を固定"
```

## Task 3: `read_memo_file_if_present` の重複サイズチェックを削除する

**Files:**
- Modify: `src/server/files/memo.rs`
- Test: `src/server/files/tests/memo_route.rs`, existing integration memo tests

- [ ] **Step 1: metadata 超過の既存 API 契約を確認する targeted test を実行する**

Run:

```bash
cargo test server::files::tests::memo_route -- メモ
```

Expected: `test result: ok`

このコマンドで filter が広すぎる場合は、次を実行する。

```bash
cargo test server::files::tests::memo_route
```

Expected: `test result: ok`

- [ ] **Step 2: 読み込み後の重複サイズチェックを削除する**

`src/server/files/memo.rs` の `read_memo_file_if_present` から次の block を削除する。

```rust
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }
```

削除後、`String::from_utf8(bytes)` が `fs.read_with_limit` 成功結果をそのまま処理する形にする。

- [ ] **Step 3: unused import が出ないか確認する**

`src/server/files/memo.rs` 冒頭の `use super::content::MAX_FILE_SIZE;` は保存時 raw size check と metadata check でまだ使われるため残す。`StatusCode` も既存 error mapping で使われるため残す。

- [ ] **Step 4: memo route tests を実行する**

Run:

```bash
cargo test server::files::tests::memo_route
```

Expected: `test result: ok`

- [ ] **Step 5: memo integration tests を実行する**

Run:

```bash
cargo test --test integration_test memo
```

Expected: `test result: ok`

- [ ] **Step 6: 変更をコミットする**

```bash
git add src/server/files/memo.rs
git commit -m "refactor: メモ読込サイズチェックを契約へ集約"
```

## Task 4: `tokio::select!` cancel-safe 前提をコメントする

**Files:**
- Modify: `src/server/session.rs`
- Test: compile through targeted test or full cargo test

- [ ] **Step 1: `tokio::select!` 直前にコメントを追加する**

`src/server/session.rs` の `loop {` と `tokio::select! {` の間に次を追加する。

```rust
        // 現在の branch はどちらも cancel-safe な受信待機だけに限定する。
        // 新しい branch を追加する場合は、select! で中断されても
        // WebSocket frame や broadcast message を失わないことを確認する。
```

結果は次の形にする。

```rust
    loop {
        // 現在の branch はどちらも cancel-safe な受信待機だけに限定する。
        // 新しい branch を追加する場合は、select! で中断されても
        // WebSocket frame や broadcast message を失わないことを確認する。
        tokio::select! {
            incoming = socket.recv() => {
```

- [ ] **Step 2: formatting check を実行する**

Run:

```bash
cargo fmt --all -- --check
```

Expected: no output ending in an error, exit code 0

- [ ] **Step 3: session 関連を含む tests を実行する**

Run:

```bash
cargo test server::session
```

Expected: `test result: ok` or no matching tests with successful compile. If no tests match, run:

```bash
cargo test --test integration_test websocket
```

Expected: `test result: ok`

- [ ] **Step 4: 変更をコミットする**

```bash
git add src/server/session.rs
git commit -m "docs: WebSocket selectのcancel-safe前提を明記"
```

## Task 5: 全体検証と BACKLOG 更新判断

**Files:**
- Modify if implementing cleanup: `docs/todo/BACKLOG.md`
- Test: repository verification

- [ ] **Step 1: full Rust tests を実行する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: `test result: ok`

- [ ] **Step 2: required verification を実行する**

Run:

```bash
./verify.sh
```

Expected: format, clippy, tests all pass.

- [ ] **Step 3: BACKLOG 更新方針を確認する**

実装完了後に `docs/todo/BACKLOG.md` の対象 3 件を Done へ移すか、別コミットで処理するかを判断する。今回のコード差分だけで PR を小さく保つ場合は、BACKLOG 更新を separate docs commit にする。

対象 3 件:

```markdown
- [ ] `BroadcastMessage::Update` 系のシリアライズ失敗時の fallback JSON を整備する
- [ ] `read_route_memo` の二重サイズチェックを単一化する
- [ ] `tokio::select!` の cancel-safe 性をコメントで明記する
```

- [ ] **Step 4: BACKLOG を更新する場合は対象 3 件を Done へ移す**

更新する場合は、各項目に完了根拠として実行した verification を追記する。新規の未検証主張は書かない。

Commit:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: BACKLOG P1契約整理を完了扱いに更新"
```

- [ ] **Step 5: 最終差分を確認する**

Run:

```bash
git status --short --branch
git log --oneline -5
```

Expected:

- working tree is clean, or only intentionally uncommitted files remain.
- recent commits correspond to the tasks above.

## Self-Review

Spec coverage:

- `BroadcastMessage::Update` fallback: Task 1 が通常契約、fallback helper、`to_json()` fallback を扱う。
- メモ読込サイズ契約: Task 2 が `MemoFs::read_with_limit` 契約、Task 3 が重複チェック削除を扱う。
- `tokio::select!` cancel-safe コメント: Task 4 が扱う。
- 検証とセキュリティ: Task 5 と各 task の targeted tests が扱う。Host/Origin、CSP、HTML sanitize、path validation は変更対象外として plan 上でも触らない。

Placeholder scan:

- 未確定語、後回し指示、未定義の作業は含めていない。
- 各 code step は実際に追加・削除する code block を示している。

Type consistency:

- `update_fallback_json()` は `src/server/messages.rs` 内の module-private helper として定義し、test から同一 module 内で参照する。
- `MemoReadError::TooLarge`、`TokioMemoFs`、`MAX_FILE_SIZE` は既存型・定数を使う。
- `cargo test` filter は既存 module path に合わせ、filter が合わない場合の代替コマンドも明記した。
