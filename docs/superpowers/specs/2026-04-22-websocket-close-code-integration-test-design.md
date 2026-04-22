# WebSocket close_code マッピングの統合テスト拡充

**日付:** 2026-04-22
**関連 TODO:** `docs/todo/TODO.md` High Priority（WebSocket close_code マッピングの統合テストを追加）
**対象ファイル:** `tests/integration_test.rs`

## 目的

`ReadMarkdownError::close_code()` の数値マッピング (`Io → 1011`, `TooLarge → 1009`, `NotUtf8 → 1003`) が、実サーバー + 実 WebSocket クライアント経路で **ワイヤ上の数値として透過して届く** ことを契約として固定する。

## 背景

- `ReadMarkdownError::close_code()` のユニットテストは `src/server/files/tests.rs` L20-34 に存在（1011 / 1009 / 1003 の単体検証）。
- 実 WebSocket フレームまで確認する統合テストは `tests/integration_test.rs` L1572-1629 に「WebSocket close frame テスト」セクションが存在し、以下の 3 件が実装済み：
  - `test_websocket_non_utf8ファイルでclose_frameにuser_messageが含まれる` → `CloseCode::Unsupported`
  - `test_websocket_削除済みファイルでclose_frameにuser_messageが含まれる` → `CloseCode::Policy` (1008, これは `SocketInitError` 経路で `ReadMarkdownError` ではない)
  - `test_websocket_サイズ超過ファイルでclose_frameにuser_messageが含まれる` → `CloseCode::Size`
- TODO 記述は古く、「統合 E2E は一切ない」は誤り。実際に欠けているのは **`ReadMarkdownError::Io` → 1011 の 1 経路のみ**。
- 加えて既存 3 テストは tungstenite の `CloseCode` enum 経由の比較であり、「1011 等の数値が透過する」という TODO の本来の契約を直接検証していない。enum 表現が将来変わっても数値契約が崩れないよう **数値直接比較** に揃える。

## スコープ

### やること

1. **新規テスト追加**: `test_websocket_ioエラーでclose_frameが1011を返す`。`ReadMarkdownError::Io` 経路を実 WebSocket で検証し、close_code が 1011、reason が `"ファイルの読み込みに失敗しました"` であることを確認。
2. **ヘルパ `assert_close_frame_message` のシグネチャ変更**: 第2引数 `expected_code` の型を `tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode` から `u16` に変更。内部比較は `u16::from(frame.code)` で実施。
3. **既存 3 テストの呼び出し更新**: `CloseCode::Unsupported / Policy / Size` を `1003 / 1008 / 1009` の数値リテラルへ置換。

### やらないこと（スコープ外）

- HTTP 経路の close_code / status code 検証（`ReadMarkdownError::Io` は HTTP では 500 に落ちるが対象外）。
- TOC / 監視経路の透過確認。
- `SocketInitError` と `ReadMarkdownError` の統合リファクタ。
- Windows クロスプラットフォームサポート（`#[cfg(unix)]` 限定）。
- 既存セクションの命名変更やファイル分割。

## 設計方針

### IO エラーの誘発手段

`ReadMarkdownError::Io` を実経路で引くには、resolve (`revalidate_single_file_target` の `canonicalize()` + `is_file()` + 拡張子チェック) をパスした上で、`read_markdown_with_limit` 内部の `tokio::fs::metadata()` か `tokio::fs::File::open()` が失敗する必要がある。

| 誘発手段 | resolve パス | IO 発火 | 判定 |
|---|---|---|---|
| **ファイル `chmod 0o000`** (Unix) | ✅ `canonicalize()` は親ディレクトリ権限で動く、`is_file()` も親の stat で OK | ✅ `open(2)` が `EACCES` で `ReadMarkdownError::Io` | **◯ 採用** |
| ディレクトリ `chmod 0o000` | ❌ `canonicalize()` が失敗 → `NotFound` (close_code 1008) | — | × 別エラーに落ちる |
| シンボリックリンク → 存在しない先 | ❌ `canonicalize()` で失敗 → `NotFound` | — | × |
| テスト中にファイル削除 | ❌ revalidate で弾かれる → 1008 | — | × |
| FIFO / デバイスファイル | ❌ `is_file()` が false → `NotFound` | — | × 不確実 |
| 読込中のストリーム切断 | ✅ | ❓ レース条件で不安定 | × |

**結論**: Unix の `chmod 0o000` が唯一の現実解。既存テスト (L306, L348, L411, L452, L576, L940 の `#[cfg(unix)]` + `std::fs::set_permissions` パターン) と完全に整合する。

### 設定順序（重要）

サーバー起動時 (`AppMode::new_single_file` 内の canonicalize) は **読み取り権限なしでも通る**（`canonicalize()` は親権限依存）が、`0o000` を先に当てると別経路で干渉する可能性がある。安全のため以下の順：

1. `setup_single_file_server_with_bytes` でファイル作成 + `AppState` 構築 + サーバー起動
2. ファイルを `chmod 0o000`
3. WebSocket 接続を試行
4. close frame を受信・検証
5. ファイル権限を元に戻す（TempDir の drop cleanup が失敗しないため）

### プラットフォームガード

- `#[cfg(unix)]` で新規テストを囲う。
- 既存パーミッション系テスト (L306, L348, L411, L452, L576, L940) と同じ扱い。
- Windows 実行は諦める。CI は無く、個人用プロジェクトのため。

### 数値直接比較化の根拠

TODO H3 の本来の意図は「`ReadMarkdownError::close_code()` が返す **数値** がワイヤまで透過する」契約の固定。既存の `CloseCode::Unsupported` 経由比較は：

- tungstenite が将来 `CloseCode` enum の variant を増減させると、コンパイルは通っても数値契約に変化が起きる可能性がある。
- ReadMarkdownError が返すのは `u16` であり、ワイヤ上も `u16`。中間で enum に変換するのは冗長。

`u16::from(frame.code)` は tungstenite 0.26 の `CloseCode` に `From<CloseCode> for u16` が実装されているため使用可能。

## 実装スケッチ

### ヘルパ変更

```rust
// 変更前 (L1803-1817)
async fn assert_close_frame_message(
    read: &mut WsReadHalf,
    expected_code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode,
    expected_reason: &str,
) {
    let msg = next_ws_message(read).await;
    match msg {
        tokio_tungstenite::tungstenite::Message::Close(Some(frame)) => {
            assert_eq!(frame.code, expected_code);
            let reason: &str = frame.reason.as_ref();
            assert_eq!(reason, expected_reason);
        }
        other => panic!("Close frameを期待したが {:?} を受信", other),
    }
}

// 変更後
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

### 呼び出し側更新（3箇所）

| 行 | 旧 | 新 |
|---|---|---|
| L1587 | `CloseCode::Unsupported` | `1003` |
| L1607 | `CloseCode::Policy` | `1008` |
| L1625 | `CloseCode::Size` | `1009` |

### 新規テスト

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

## リスクと緩和

| リスク | 影響 | 緩和 |
|---|---|---|
| root 実行時は `0o000` でも読めるためテスト失敗 | false failure | 個人用・root 運用外なので受容。既存パーミッション系テストと同じリスク |
| assert パニック時に権限復元が走らず TempDir cleanup が失敗 | テストログに cleanup warning | 既存パターン (L363-374, L425-436 等) と同じ扱い。`scopeguard` 等の Drop ガードは YAGNI |
| `#[cfg(unix)]` ガードで Windows ではテスト不実行 | カバレッジ偏り | CI 無し + 個人用で受容。既存パターンと整合 |
| `u16::from(CloseCode)` が tungstenite 将来バージョンで消失 | コンパイル失敗 | `Cargo.toml` で 0.26 固定済み。依存更新時に再検証する通常の運用で十分 |

## 検証計画

```bash
# 新規テスト単体
cargo test --test integration_test test_websocket_ioエラー

# 既存 3 テストの回帰
cargo test --test integration_test test_websocket_non_utf8
cargo test --test integration_test test_websocket_削除済み
cargo test --test integration_test test_websocket_サイズ超過

# 全体検証
./verify.sh
```

## 成功基準

- [ ] 新規テスト `test_websocket_ioエラーでclose_frameが1011を返す` が Unix で Pass する
- [ ] 既存 3 テストが `u16` 数値直接比較に変更後も Pass する
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` が Pass する
- [ ] `./verify.sh` が全て Pass する
- [ ] `TODO.md` High Priority の WebSocket close_code 項目にチェックが入る
