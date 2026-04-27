# `build_lagged_recovery_message` IO エラー統合テスト 実装プラン

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `build_lagged_recovery_message` が遅延回復経路で IO エラー (`ReadMarkdownError::Io`) を `BroadcastMessage::Error` に畳み込み、WS Text フレームとして送出する配線を統合テストで保証する。

**Architecture:** `tests/integration_test.rs` に Unix 限定の `#[tokio::test]` 関数を 1 件追加する。`current_thread` ランタイム上で `state.tx().send(BroadcastMessage::Refresh)` を await を挟まず 17 回連続実行することで容量 16 の broadcast を決定的にオーバーフローさせ、`RecvError::Lagged(1)` を誘発。ファイルは chmod 0o000 で読み取り不可にしてある状態で recovery を走らせ、`session.rs:121` の `socket.send(Message::Text(payload))` まで到達した結果を JSON パースして検証する。

**Tech Stack:** Rust / tokio (broadcast channel) / axum / tokio-tungstenite (WS client) / tempfile / `#[cfg(unix)]` chmod。

**Spec:** [`docs/superpowers/specs/2026-04-27-lagged-recovery-io-error-integration-test-design.md`](../specs/2026-04-27-lagged-recovery-io-error-integration-test-design.md)

---

## ファイル構造

| ファイル | 役割 | 変更種別 |
|---------|------|---------|
| `tests/integration_test.rs` | Unix 限定 IO 統合テストを 1 件追加 (現 L1732 直後) | 修正 |
| `docs/todo/TODO.md` | 該当 Medium 項目を `[ ]` → `[x]` | 修正 |

新規ファイルなし。新規ヘルパなし。既存 helper (`setup_single_file_server_from_path`, `connect_ws`, `next_ws_message`, `make_file_unreadable`, `FilePermissionGuard`) と既存 import (`BroadcastMessage`, `state.tx()`) のみで完結する。

---

## Task 1: 遅延回復 IO エラー統合テストの追加

**Files:**
- Modify: `tests/integration_test.rs` (現 L1732 の直後、`test_websocket_ioエラーでclose_frameが1011を返す` テスト直後に追加)
- Modify: `docs/todo/TODO.md` (L80-83 の項目を `[x]` 化)

### - [ ] Step 1: テスト関数を追加

`tests/integration_test.rs` の `test_websocket_ioエラーでclose_frameが1011を返す` 関数の直後（現 L1732 の `}` の次の空行の後）に、以下を挿入する。

**追加位置の前後コンテキスト** (現状の L1730-1734):

```rust
    // teardown: TempDir drop で失敗しないよう権限を復元
    fs::set_permissions(&file_path, Permissions::from_mode(original_mode)).unwrap();
}

// ==============================
// ヘルパー関数
// ==============================
```

**変更後** (新規テストを `}` と `// =====` の間に挿入):

```rust
    // teardown: TempDir drop で失敗しないよう権限を復元
    fs::set_permissions(&file_path, Permissions::from_mode(original_mode)).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する() {
    // current_thread runtime 前提: tokio::test のデフォルト。flavor = "multi_thread" を
    // 指定すると burst 中に session task が並走してしまい、決定的に Lagged を誘発できない。
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("lagged_io.md");
    tokio::fs::write(&file_path, "# Before lagged").await.unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 初期 Update を消費。session task が rx.subscribe 済みかつ recv ループに入った
    // ことの暗黙的バリアになる。
    let _initial_message = next_ws_message(&mut read).await;

    // recovery 実行中は chmod 0o000 が維持されている必要があるため、burst 直前で取得し
    // assert 完了後に drop して権限復元する。
    let Some(permission_guard) = make_file_unreadable(&file_path) else {
        drop(tmp_dir);
        return;
    };

    // burst 件数 = 容量 16 + 1 = 17。await を挟まず synchronous に発火することで、
    // 受信タスクが起きる前にチャネルが overflow し、次の recv で Lagged(1) が確定する。
    for _ in 0..17 {
        state.tx().send(BroadcastMessage::Refresh).unwrap();
    }

    // 次フレームは Lagged → build_lagged_recovery_message → Io ReadFailed →
    // BroadcastMessage::Error 経路で送出される
    let msg = next_ws_message(&mut read).await;
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let error = json["error"].as_str().expect("errorフィールドが存在する");
    assert_eq!(
        error,
        "ファイル読み込みエラー (lagged_io.md): ファイルの読み込みに失敗しました"
    );

    // 順序: assert 後に権限復元 → tempdir 自動 drop。recovery 実行中は guard 生存中で
    // chmod 0o000 が維持されている必要がある。
    drop(permission_guard);
    drop(tmp_dir);
}

// ==============================
// ヘルパー関数
// ==============================
```

`BroadcastMessage` は既に L14 で import 済み (`use markdown_view::server::{AppMode, AppState, BroadcastMessage, WatchService};`)。新規 import は不要。

### - [ ] Step 2: テストが Pass することを確認

```bash
cargo test --test integration_test test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する -- --nocapture
```

**期待結果:**
```
running 1 test
test test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する ... ok

test result: ok. 1 passed; 0 failed
```

`tracing::warn!("[markdown-view] WebSocketクライアントが1メッセージ遅延")` 等の warn ログが出るが、`ok` 判定であれば成功。

### - [ ] Step 3: 配線回帰検出力を一時改変で確認 (sanity check)

このテストが本当に「Lagged → IO エラー → Error JSON」の配線を検証していることを確認するため、**一時的に** `src/server/files/content.rs:151-155` を以下に書き換え、テストが FAIL することを確認する。

**一時改変** (`src/server/files/content.rs:151-155`):

```rust
        // 一時改変: ReadFailed の Error メッセージを別文字列に
        ValidateRenderOutcome::ReadFailed(_target, _error) => {
            BroadcastMessage::Error("REGRESSION_TEST_MARKER".to_string())
        }
```

```bash
cargo test --test integration_test test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する
```

**期待結果:** FAIL — `assert_eq!` で `"REGRESSION_TEST_MARKER"` と期待文字列が一致せず panic。

確認後、**改変を必ず revert** して元に戻す:

```bash
git checkout -- src/server/files/content.rs
```

再度テストを実行して PASS を確認:

```bash
cargo test --test integration_test test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する
```

**期待結果:** PASS。

### - [ ] Step 4: 決定性確認 (10 連続実行で flake 0)

```bash
for i in $(seq 1 10); do
  echo "=== Run $i ==="
  cargo test --test integration_test test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する || { echo "FAILED on run $i"; break; }
done
```

**期待結果:** 10 回すべて PASS。1 回でも失敗した場合は flake あり → プラン要再設計。

### - [ ] Step 5: 全 Rust テストが引き続き通ることを確認

```bash
cargo test --all-targets --all-features
```

**期待結果:** 既存のすべてのテストと新テスト 1 件を含めて全て PASS。

### - [ ] Step 6: フォーマット・リント確認

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

**期待結果:** 両方とも warnings 0 で完了。

### - [ ] Step 7: `verify.sh` で総合検証

```bash
./verify.sh
```

**期待結果:** fmt / clippy / cargo test / npm typecheck すべて PASS。

### - [ ] Step 8: TODO.md の該当項目を完了マーク

`docs/todo/TODO.md` の L80-83 を以下のように更新する。

**変更前** (L80):
```markdown
- [ ] `build_lagged_recovery_message` の IO エラー透過を統合テストでカバー
```

**変更後**:
```markdown
- [x] `build_lagged_recovery_message` の IO エラー透過を統合テストでカバー
```

L81-83 のサブ箇条書き (`ファイル:` `対応:` `理由:`) は変更しない。

### - [ ] Step 9: コミット

事前にステージング:

```bash
git add tests/integration_test.rs docs/todo/TODO.md
git status --short
```

**期待結果:**
```
M  docs/todo/TODO.md
M  tests/integration_test.rs
```

コミットメッセージを `/tmp/commit-msg-lagged-impl.txt` に書き出して使う（heredoc は guard hook と相性が悪い）:

```
test: build_lagged_recovery_message の IO エラー統合テストを追加

変更内容:
- tests/integration_test.rs に test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する を追加
- docs/todo/TODO.md の該当 Medium 項目を [x] 化

変更理由:
- session.rs の Lagged arm から build_lagged_recovery_message を呼び、Io ReadFailed を BroadcastMessage::Error に畳み込み、WS Text フレームとして送出するまでの配線が未テスト
- 兄弟経路 build_change_broadcast_message の IO 統合テスト (test_ファイル変更_io_エラーでwebsocketエラー通知) と対称の網羅を達成
- current_thread ランタイム上で broadcast::Sender::send() を await を挟まず 17 回連続実行することで Lagged(1) を決定的に誘発、TODO 著者が懸念した再現性問題を解消

影響範囲:
- 新規テスト 1 件 (Unix 限定)。既存テスト・実装コードへの変更なし

テスト結果: cargo test 全件 Pass、verify.sh Pass、10 連続実行で flake 0
```

コミット実行:

```bash
git commit -F /tmp/commit-msg-lagged-impl.txt
rm /tmp/commit-msg-lagged-impl.txt
git log --oneline -1
```

**期待結果:** 直近コミットが `test: build_lagged_recovery_message の IO エラー統合テストを追加` であること。

---

## 自己レビュー

### Spec カバレッジチェック

| Spec 要求事項 | 実装タスク | 状態 |
|--------------|----------|------|
| `tests/integration_test.rs` に追加 | Step 1 | ✓ |
| 配置: `test_websocket_ioエラーでclose_frameが1011を返す` 直後 | Step 1 (位置指定あり) | ✓ |
| 名前: `test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する` | Step 1 | ✓ |
| `#[cfg(unix)]` + `#[tokio::test]` 属性 | Step 1 | ✓ |
| current_thread ランタイム維持 (flavor 指定なし) | Step 1 | ✓ |
| 17 回 burst-send | Step 1 (`for _ in 0..17`) | ✓ |
| chmod 0o000 で IO 経路誘発 | Step 1 (`make_file_unreadable`) | ✓ |
| FilePermissionGuard で RAII 復元 | Step 1 (`drop(permission_guard)`) | ✓ |
| 初期 Update メッセージ消費 | Step 1 | ✓ |
| `assert_eq!` 完全一致での error 検証 | Step 1 | ✓ |
| 受け入れ基準 #1: cargo test PASS | Step 2 | ✓ |
| 受け入れ基準 #2: 10 連続 flake 0 | Step 4 | ✓ |
| 受け入れ基準 #3: cargo test 全体 PASS | Step 5 | ✓ |
| 受け入れ基準 #4: fmt/clippy PASS | Step 6 | ✓ |
| 受け入れ基準 #5: verify.sh PASS | Step 7 | ✓ |
| 受け入れ基準 #6: 3 つのコメント挿入 | Step 1 (3 箇所のコメント記述あり) | ✓ |
| 受け入れ基準 #7: TODO.md `[x]` 更新 | Step 8 | ✓ |
| 配線回帰検出力の確認 | Step 3 (sanity check) | ✓ (spec の「リファクタ時の回帰を検知」を明示確認) |

漏れなし。

### Placeholder スキャン

- `TBD` / `TODO` / `implement later`: なし
- `Add appropriate error handling`: なし
- `Similar to Task N`: なし (タスクは 1 つのみ)
- 説明だけで code が抜けている step: なし。code-changing step 全てに具体コード提示

### 型・名前一貫性

- `state.tx()` — `state.rs:240` で `pub fn tx(&self) -> &broadcast::Sender<BroadcastMessage>` と一致
- `BroadcastMessage::Refresh` — `messages.rs:40` で unit variant、`pub enum` の一部として import 済
- `setup_single_file_server_from_path` — `tests/integration_test.rs:1768` 既存
- `connect_ws` — `tests/integration_test.rs:1893` 既存
- `next_ws_message` — `tests/integration_test.rs:1929` 既存
- `make_file_unreadable` — `tests/integration_test.rs:1825` 既存、戻り値 `Option<FilePermissionGuard>` と一致
- アサーション文字列 `"ファイル読み込みエラー (lagged_io.md): ファイルの読み込みに失敗しました"` — `content.rs:151-156` の `format!("ファイル読み込みエラー ({}): {}", target.file_label(), error.user_message())` と一致
  - `target.file_label()` はファイル名 `lagged_io.md` を返す (兄弟テスト L882 で `watch_io_error.md` が同じ位置に来ている事実から確認)
  - `ReadMarkdownError::Io(_).user_message()` は `"ファイルの読み込みに失敗しました"` (兄弟テストでの実証）

不整合なし。
