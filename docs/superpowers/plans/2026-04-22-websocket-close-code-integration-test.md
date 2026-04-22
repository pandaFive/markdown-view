# WebSocket close_code 統合テスト拡充 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ReadMarkdownError::close_code()` の数値マッピング (Io→1011, TooLarge→1009, NotUtf8→1003) が実 WebSocket ワイヤまで透過することを統合テストで固定し、既存 3 テストを数値直接比較に揃える。

**Architecture:** `tests/integration_test.rs` のみに対する変更。ヘルパ `assert_close_frame_message` のシグネチャを `CloseCode` 型から `u16` へ変更し既存 3 呼び出しを数値リテラルに更新するリファクタリングと、`ReadMarkdownError::Io` 経路を Unix の `chmod 0o000` で誘発する新規テスト 1 件を追加する。

**Tech Stack:** Rust 2021, tokio, axum, tokio-tungstenite 0.26 (CloseCode→u16 変換), `std::os::unix::fs::PermissionsExt` (Unix限定テスト)

**関連 spec:** `docs/superpowers/specs/2026-04-22-websocket-close-code-integration-test-design.md`

---

## File Structure

### 変更ファイル

- **Modify:** `tests/integration_test.rs`
  - L1587, L1607, L1625 — 既存 3 呼び出しの `CloseCode::*` を `u16` 数値リテラルへ変更
  - L1803-1817 — ヘルパ `assert_close_frame_message` のシグネチャと比較方式を変更
  - L1629 の直後 — 新規テスト `test_websocket_ioエラーでclose_frameが1011を返す` を追加
- **Modify:** `docs/todo/TODO.md`
  - High Priority の WebSocket close_code 項目を `[x]` に変更

### 作成ファイルなし

---

## Task 1: ヘルパ `assert_close_frame_message` の数値直接比較化と既存呼び出し更新

**目的:** 「`ReadMarkdownError::close_code()` が返す数値がワイヤまで透過する」という契約を enum 経由ではなく `u16` で直接固定する。これは **振る舞いを変えないリファクタリング**。既存 3 テストは前後で同じく Pass する。

**Files:**
- Modify: `tests/integration_test.rs` L1587, L1607, L1625, L1803-1817

- [ ] **Step 1: ヘルパシグネチャと比較方式を変更**

`tests/integration_test.rs` L1803-1817 を以下の内容に置換する:

```rust
async fn assert_close_frame_message(
    read: &mut WsReadHalf,
    expected_code: u16,
    expected_reason: &str,
) {
    let msg = next_ws_message(read).await;
    match msg {
        tokio_tungstenite::tungstenite::Message::Close(Some(frame)) => {
            assert_eq!(u16::from(frame.code), expected_code);
            let reason: &str = frame.reason.as_ref();
            assert_eq!(reason, expected_reason);
        }
        other => panic!("Close frameを期待したが {:?} を受信", other),
    }
}
```

変更点:
- `expected_code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode` → `expected_code: u16`
- `assert_eq!(frame.code, expected_code)` → `assert_eq!(u16::from(frame.code), expected_code)`

- [ ] **Step 2: 既存呼び出し 3 箇所を数値リテラルへ置換**

`tests/integration_test.rs` の各 `CloseCode::*` を対応する `u16` リテラルに置換する:

L1587 (test_websocket_non_utf8ファイルでclose_frameにuser_messageが含まれる):
```rust
// 変更前
        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Unsupported,
// 変更後
        1003,
```

L1607 (test_websocket_削除済みファイルでclose_frameにuser_messageが含まれる):
```rust
// 変更前
        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy,
// 変更後
        1008,
```

L1625 (test_websocket_サイズ超過ファイルでclose_frameにuser_messageが含まれる):
```rust
// 変更前
        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Size,
// 変更後
        1009,
```

- [ ] **Step 3: コンパイルチェック**

Run: `cargo check --all-targets --all-features`
Expected: PASS (警告なし、エラーなし)

- [ ] **Step 4: 既存 3 テストの回帰確認**

Run: `cargo test --test integration_test test_websocket_non_utf8`
Expected: PASS (1 passed)

Run: `cargo test --test integration_test test_websocket_削除済み`
Expected: PASS (1 passed)

Run: `cargo test --test integration_test test_websocket_サイズ超過`
Expected: PASS (1 passed)

- [ ] **Step 5: clippy チェック**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS (警告ゼロ)

- [ ] **Step 6: コミット**

```bash
git add tests/integration_test.rs
git commit -F - <<'EOF'
refactor: WebSocket close_code assertion を u16 数値直接比較に変更

変更内容:
- assert_close_frame_message ヘルパの expected_code を CloseCode 型から u16 型へ
- frame.code の比較を u16::from(frame.code) で実施
- 既存 3 テストの呼び出しを CloseCode::Unsupported/Policy/Size から 1003/1008/1009 へ置換

変更理由:
- ReadMarkdownError::close_code() が返すのは u16 であり、ワイヤ上も u16。中間で enum に変換しない直接比較に揃えることで、tungstenite の CloseCode enum 表現が将来変わっても数値契約が崩れないことを保証する。

影響範囲:
- tests/integration_test.rs のみ（テストコードのリファクタリング）
- 本体コードの変更なし、振る舞いの変更なし

テスト結果: 既存 3 テスト Pass、clippy 警告ゼロ
EOF
```

---

## Task 2: `ReadMarkdownError::Io` 経路の close_code 1011 統合テスト追加

**目的:** 欠落していた `ReadMarkdownError::Io` 経路を Unix の `chmod 0o000` で誘発し、close_code 1011 と reason `"ファイルの読み込みに失敗しました"` がワイヤまで届くことを確認する。

**Files:**
- Modify: `tests/integration_test.rs` L1629 直後に追加

- [ ] **Step 1: 新規テストを追加**

`tests/integration_test.rs` の L1629 (「WebSocket close frame テスト」セクションの末尾、サイズ超過テストの閉じ括弧の次) の直後に以下を追加する:

```rust
#[cfg(unix)]
#[tokio::test]
async fn test_websocket_ioエラーでclose_frameが1011を返す() {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;

    let (_state, addr, _tmp_dir, file_path) =
        setup_single_file_server_with_bytes("unreadable.md", b"# content").await;

    // 読込 IO を誘発: resolve (canonicalize/is_file) はパスするが open(2) が EACCES で失敗
    let original_mode = fs::metadata(&file_path).unwrap().permissions().mode();
    fs::set_permissions(&file_path, Permissions::from_mode(0o000)).unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    assert_close_frame_message(
        &mut read,
        1011,
        "ファイルの読み込みに失敗しました",
    )
    .await;

    // teardown: TempDir drop で失敗しないよう権限を復元
    fs::set_permissions(&file_path, Permissions::from_mode(original_mode)).unwrap();
}
```

設計上のポイント:
- `#[cfg(unix)]` ガードで Windows 不実行（既存パーミッション系テスト L306, L348, L411 等と同じ扱い）
- サーバー起動 → `chmod 0o000` → 接続、の順序が重要。`AppMode::new_single_file` 内の `canonicalize()` は起動時に完了しているため、後から権限を落としても resolve は通る
- `ReadMarkdownError::Io(_).user_message()` = `"ファイルの読み込みに失敗しました"` (src/server/files/content.rs:252)
- `ReadMarkdownError::Io(_).close_code()` = 1011 (src/server/files/content.rs:244)

- [ ] **Step 2: 新規テストを実行して PASS することを確認**

Run: `cargo test --test integration_test test_websocket_ioエラー`
Expected: PASS (1 passed; 0 failed)

**もし FAIL した場合の診断:**
- `ファイル検証に失敗しました` を含む reason なら resolve で弾かれている → `setup_single_file_server_with_bytes` 後の `chmod` タイミングを確認
- close_code が 1008 なら `ResolveFileError` 経路に落ちている → 同上
- 接続自体が拒否されるなら Origin / Host の問題 → `connect_ws` の引数を確認

- [ ] **Step 3: テストが実効的であることを確認（仮に期待値を壊して FAIL を確認）**

テストが実際に `1011` を検証していることを確認するため、一時的に `assert_close_frame_message` の第 2 引数を `9999` に変更する:

```rust
    assert_close_frame_message(
        &mut read,
        9999,  // 一時変更: 実効性確認用
        "ファイルの読み込みに失敗しました",
    )
    .await;
```

Run: `cargo test --test integration_test test_websocket_ioエラー`
Expected: FAIL (`assertion failed: left: 1011, right: 9999`)

**確認後、値を `1011` に戻す**:
```rust
    assert_close_frame_message(
        &mut read,
        1011,
        "ファイルの読み込みに失敗しました",
    )
    .await;
```

Run: `cargo test --test integration_test test_websocket_ioエラー`
Expected: PASS

- [ ] **Step 4: WebSocket close frame テスト全件を回帰確認**

Run: `cargo test --test integration_test test_websocket_ -- --test-threads=1`
Expected: 4 テスト全て PASS（non_utf8 / 削除済み / サイズ超過 / ioエラー）

- [ ] **Step 5: clippy チェック**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS (警告ゼロ)

- [ ] **Step 6: TODO.md を完了マーク**

`docs/todo/TODO.md` の High Priority の WebSocket close_code 項目を更新する:

変更前（L25-29）:
```markdown
- [ ] WebSocket close_code マッピングの統合テストを追加
  - ファイル: `tests/integration_test.rs`
  - 現状: `ReadMarkdownError::close_code()` のユニットテストは存在、`load_initial_socket_update` のエラー arm も Low 側で TODO 化済み。だが実際の WebSocket フレームまで透過確認する E2E はない
  - 追加観点: IO → 1011、TooLarge → 1009、NotUtf8 → 1003 の 3 シナリオを実サーバー + WebSocket クライアントで検証
  - 理由: WebSocket プロトコル境界。クライアント側の再接続ロジックが close_code に依存するため、中間層のどこかで書き換わると下流が壊れる
```

変更後:
```markdown
- [x] WebSocket close_code マッピングの統合テストを追加
  - ファイル: `tests/integration_test.rs`
  - 現状: `ReadMarkdownError::close_code()` のユニットテストは存在、`load_initial_socket_update` のエラー arm も Low 側で TODO 化済み。だが実際の WebSocket フレームまで透過確認する E2E はない
  - 追加観点: IO → 1011、TooLarge → 1009、NotUtf8 → 1003 の 3 シナリオを実サーバー + WebSocket クライアントで検証
  - 理由: WebSocket プロトコル境界。クライアント側の再接続ロジックが close_code に依存するため、中間層のどこかで書き換わると下流が壊れる
```

差分は L25 の `[ ]` → `[x]` のみ。

- [ ] **Step 7: 全体検証 (verify.sh)**

Run: `./verify.sh`
Expected: fmt / clippy / cargo test / typecheck 全て PASS

- [ ] **Step 8: コミット**

```bash
git add tests/integration_test.rs docs/todo/TODO.md
git commit -F - <<'EOF'
test: WebSocket ReadMarkdownError::Io 経路の close_code 1011 検証を追加

変更内容:
- test_websocket_ioエラーでclose_frameが1011を返す を追加 (Unix 限定)
- chmod 0o000 で読込権限を落とし EACCES を誘発する手法で resolve パス後の ReadMarkdownError::Io を再現
- close_code 1011 と reason "ファイルの読み込みに失敗しました" がワイヤまで届くことを検証
- TODO.md High Priority の該当項目を完了マーク

変更理由:
- ReadMarkdownError::Io → 1011 の透過確認が欠落しており、中間層で書き換わった場合にクライアント再接続ロジックが壊れても検知できなかった
- ユニットテスト (src/server/files/tests.rs:20-34) と HTTP 側の統合テストはあるが、実 WebSocket フレームまでの数値透過は未検証だった

影響範囲:
- tests/integration_test.rs に 1 テスト追加
- docs/todo/TODO.md に 1 行の完了マーク変更
- 本体コード変更なし

テスト結果: 追加テスト Pass、close_frame テスト 4 件全て Pass、verify.sh 全通し
EOF
```

---

## Self-Review

### Spec coverage

| Spec の成功基準 | 実装タスク |
|---|---|
| 新規テスト `test_websocket_ioエラーでclose_frameが1011を返す` が Unix で Pass する | Task 2 Step 1-3 |
| 既存 3 テストが `u16` 数値直接比較に変更後も Pass する | Task 1 Step 4 |
| `cargo clippy --all-targets --all-features -- -D warnings` が Pass する | Task 1 Step 5 + Task 2 Step 5 |
| `./verify.sh` が全て Pass する | Task 2 Step 7 |
| `TODO.md` High Priority の WebSocket close_code 項目にチェックが入る | Task 2 Step 6 |

全項目カバー。

### Placeholder scan

- 「TODO」「TBD」「後で」「適切に」等のプレースホルダなし
- 全てのコードブロックは実行可能な Rust / bash
- 失敗時の診断手順は Task 2 Step 2 に具体化
- ファイルパスは `tests/integration_test.rs` と `docs/todo/TODO.md` で一貫

### Type consistency

- `assert_close_frame_message` のシグネチャ: `u16` 型を Task 1 Step 1 で定義、Task 1 Step 2 と Task 2 Step 1 で同じ `u16` リテラル (1003/1008/1009/1011) を渡している
- `setup_single_file_server_with_bytes` は既存 (L1673) を使用。Task 2 の呼び出しで 4 タプル `(_state, addr, _tmp_dir, file_path)` を受けており、既存の呼び出し (L1577, L1595, L1616) と同じ戻り値構造
- `connect_ws`, `next_ws_message`, `WsReadHalf` は既存のまま変更なし

矛盾なし。
