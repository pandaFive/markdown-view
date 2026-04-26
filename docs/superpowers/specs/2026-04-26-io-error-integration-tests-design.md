# IO エラー経路の統合テスト追加（HTTP 500 / WS broadcast Error）

**日付:** 2026-04-26
**関連 TODO:** `docs/todo/TODO.md` Medium Priority
- HTTP `/api/content` の IO エラー経路 (500) の統合テストを追加
- `build_change_broadcast_message` / `build_lagged_recovery_message` の IO エラー経路を統合テストでカバー

**対象ファイル:** `tests/integration_test.rs`

## 目的

`ReadMarkdownError::Io(_)` が以下 2 経路で **実サーバー + 実クライアント経路を透過して届く** ことを契約として固定する：

1. HTTP `/api/content`：`status_code() = 500` + JSON `{ "error": "ファイルの読み込みに失敗しました" }`
2. WebSocket broadcast（ファイル変更経由）：`build_change_broadcast_message` の `ReadFailed` arm が `BroadcastMessage::Error("ファイル読み込みエラー (<file>): ファイルの読み込みに失敗しました")` を発信

既に WebSocket 初期化経路（`SocketInitError`）の 1011 透過テストは `test_websocket_ioエラーでclose_frameが1011を返す`（`tests/integration_test.rs:1652`）で完了している。本 spec はその対称項目として残る 2 経路を埋める。

## 背景

`ReadMarkdownError` は 3 経路で `BroadcastMessage::Error` / HTTP / WS close frame に分岐する：

| 経路 | エントリ関数 | 出力先 | 既存テスト |
|---|---|---|---|
| WS 初期化 | `load_initial_socket_update` | WS close frame (1011 / 1009 / 1003) | ✅ `test_websocket_ioエラーでclose_frameが1011を返す` ほか |
| HTTP API | `IntoResponse for ReadMarkdownError` (`/api/content` 経由) | HTTP status + JSON body | ⚠ Io arm の透過確認なし。TooLarge / NotUtf8 / NotFound は `assert_json_error_for_paths` で網羅済み |
| WS 変更 broadcast | `build_change_broadcast_message` | `BroadcastMessage::Error(format!(...))` | ⚠ ResolveFailed arm は `test_ファイル削除でwebsocketエラー通知` で確認済み。ReadFailed arm（Io）は未確認 |
| WS lagged recovery | `build_lagged_recovery_message` | 同上 | ❌ 未確認（**本 spec のスコープ外**） |

3 経路目（broadcast）は同じ `ReadMarkdownError` を別経路で format! しているため、一つの経路のリファクタが他経路を silent に壊しうる。

ユニットテストは `src/server/files/content.rs` 内および `src/server/broadcast.rs` 内で網羅されているが、watcher → forwarder → broadcast → WebSocket クライアントの透過経路自体を実サーバーで確認できていない。

## スコープ

### やること

1. **新規テスト T1: `test_api_content_io_エラーで500を返す`**（`#[cfg(unix)]`）
   - `/api/content` への `chmod 0o000` ファイル経由 GET で status 500 + JSON `error` 完全一致を検証
2. **新規テスト T2: `test_ファイル変更_io_エラーでwebsocketエラー通知`**（`#[cfg(unix)]`）
   - 通常ファイルを `WatchService` 監視 → WS 接続 → 内容書き換え（notify 発火）→ debounce window 内に `chmod 0o000` → `BroadcastMessage::Error` を WS で受信し、3 軸（経路マーカー / ファイル名 / Io arm 文言）を contains で検証
3. **新規ヘルパー追加なし**：既存の `setup_single_file_server_with_bytes`、`assert_json_error_for_paths`、`next_ws_message`、`WatchService::start`、`connect_ws` で完結

### やらないこと（スコープ外）

- `build_lagged_recovery_message` の Io arm 透過テスト（broadcast channel 飽和 + 受信側意図的遅延 + chmod の 3 段タイミング合成が必要、再現性低）→ 「将来の宿題」セクションへ
- Windows での同等テスト（`chmod 0o000` は POSIX 専用、既存 1011 テストも `#[cfg(unix)]`）
- TooLarge / NotUtf8 / NotFound の HTTP 経路（既に `assert_json_error_for_paths` で網羅）
- ユニットテスト追加（`build_change_broadcast_message` / `build_lagged_recovery_message` の関数単体テストは既存で十分）
- 他 TODO 項目（`is_hidden_relative` リファクタ、`updateContent` E2E、JS 契約違反ログ）— 別 spec

## 設計方針

### IO エラーの誘発手段

既存 1011 テストと同じく **`chmod 0o000`** を採用する。`set_permissions(0o000)` は inode メタデータを変えるだけで、`canonicalize`（= `realpath`）は親ディレクトリの execute 権限のみ要求するため通る。`is_file` も親 stat で OK。`open(2)` 段で初めて EACCES が発生し、`ReadMarkdownError::Io` arm に到達する。

| 誘発手段 | resolve パス | IO 発火 | 判定 |
|---|---|---|---|
| **ファイル `chmod 0o000`**（Unix） | ✅ canonicalize / is_file が親権限で動く | ✅ `open(2)` が EACCES | **◯ 採用** |
| 親ディレクトリ `chmod 0o111` | ❌ canonicalize が親 read 権限を要求し失敗 → `ResolveFailed` 経路に逸れる | — | × Io arm に到達しない |
| ファイル削除 | ❌ `ResolveFileError::NotFound` で `ResolveFailed` arm | — | × `test_ファイル削除でwebsocketエラー通知` でカバー済み |

### T2 の race 戦略：write → set_permissions（スリープ無し）

watcher 経路で `Io(_)` を踏ませる手順：

1. 通常ファイル作成 → `WatchService::start` → WS 接続 → 初期メッセージ消費
2. `tokio::fs::write(&file_path, "# After")` で notify 発火（debounce 300ms タイマ起動）
3. **直後**に `fs::set_permissions(&file_path, 0o000)` を同期実行
4. debounce 300ms 後に `build_change_broadcast_message` が走る時点で `open(2)` が EACCES → `BroadcastMessage::Error` 発信
5. WS で受信、JSON `error` を contains で 3 軸検証

**スリープを一切挟まない設計**：`tokio::fs::write` は数ミリ秒で完了し、その直後に同期 `set_permissions` を呼べば 300ms の debounce window に確実に間に合う。スリープを挟むと「どれだけ待つべきか」がマシン負荷に依存し、CI の flaky 化要因になる。スリープ無しで write→chmod を**できるだけ近接**させるのが最も race window に強い。

理論的に write→chmod の順序が逆転するのは、`tokio::fs::write` 完了から `set_permissions` 呼び出しまでの間に 300ms 以上のスケジューラ遅延が発生したケースのみ。実用上ほぼ起きないし、もし起きてもテストは**タイムアウトで明示的に失敗**するので silent な誤検知にはならない。

### アサーション粒度

| テスト | 粒度 | 理由 |
|---|---|---|
| T1 (HTTP 500) | **完全一致** (`assert_json_error_for_paths` の `Some(msg)` 引数) | 既存 1011 テストの `assert_close_frame_message` も完全一致。HTTP 側もユーザ向け文言の silent な変更を検出すべき |
| T2 (WS broadcast) | **3 軸 contains** | `build_change_broadcast_message` の format! 文字列は `"ファイル読み込みエラー (<file>): <user_message>"` と 3 要素から組み立てられている。3 軸独立で contains することで、(a) `ResolveFailed` arm への誤吸収、(b) ファイル名の漏れ、(c) `Io` 以外 arm への誤マップ を別々に検出できる |

T2 の 3 軸：

1. `"ファイル読み込みエラー"` ← `ReadFailed` arm の prefix（`ResolveFailed` arm の `"ファイル検証エラー"` と区別）
2. `"watch_io_error.md"` ← `target.file_label()` の透過
3. `"ファイルの読み込みに失敗しました"` ← `user_message()` Io arm 文言

### 配置

| テスト | 配置 | 理由 |
|---|---|---|
| T1 | 既存 `assert_json_error_for_paths` を使う HTTP テスト群（`test_存在しないファイル時は404を返す` 周辺、L737 付近） | 役割別に並べる方がスキャンしやすい |
| T2 | `test_ファイル削除でwebsocketエラー通知`（L803）の直後 | watcher 経由 WS テスト群と並べる。同じ `ResolveFailed` vs `ReadFailed` の対比を物理的に近接させる |

### プラットフォームガード

両テストとも `#[cfg(unix)]`。既存パーミッション系テスト（L306, L348, L411, L452, L576, L940, L1652）と同じ扱い。Windows 実行は諦める（CI 無し + 個人用前提）。

### Single-file モードを使う

両テストとも single-file モードで実施する。ディレクトリモードでも同じ `read_markdown_with_limit` を経由するが、single-file モードの方が test fixture が単純で、`resolve_change_target` の path 揺らぎが入らない。Io arm 透過の純度を保つ。

## 実装スケッチ

### T1: `test_api_content_io_エラーで500を返す`

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

### T2: `test_ファイル変更_io_エラーでwebsocketエラー通知`

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

## 検出できる回帰

### T1 が捕まえる
- `ReadMarkdownError::Io(_) → status_code()` の誤マッピング（500 以外への変更）
- `IntoResponse for ReadMarkdownError` の JSON ボディ生成変更（`error` フィールド名変更、`error_message_json` 改変）
- `user_message()` Io arm の silent な文言変更

### T2 が捕まえる
- `build_change_broadcast_message` の `ReadFailed` arm が削除 / 別 arm に統合される変更
- `Io` が `Error` 以外の `BroadcastMessage` variant に乗る変更
- `target.file_label()` の埋め込みが取れる変更
- `user_message()` Io arm の silent な文言変更（T1 と二重防御）
- watcher → forwarder → broadcast の透過経路自体が壊れる変更

### 両方が捕まえない
- canonicalize 段の `Io(_)` → これは `ResolveFileError::Io` 経路で、`ReadMarkdownError::Io` には到達しない
- `metadata().len() > MAX_FILE_SIZE` 後の `open()` 失敗のような特殊レース → 別テスト範囲
- `build_lagged_recovery_message` の Io arm → 「将来の宿題」

## リスクと緩和

| リスク | 影響 | 緩和 |
|---|---|---|
| T2 の write→chmod 順序がマシン負荷で逆転 | テストがタイムアウト | スリープ無し設計で window を最大化。逆転しても症状はタイムアウト失敗で silent 誤検知ではない。CI 無し + 個人用前提で受容 |
| root 実行時は `0o000` でも読めるためテスト失敗 | false failure | 既存パーミッション系テストと同じリスク。受容 |
| assert パニック時に権限復元が走らず TempDir cleanup が失敗 | ログに cleanup warning | 既存パターンと同じ扱い。`scopeguard` 等の Drop ガードは YAGNI |
| `#[cfg(unix)]` で Windows 不実行 | カバレッジ偏り | CI 無し + 個人用で受容 |
| T2 の watcher debounce が将来 300ms から大きく変わる | テストタイムアウト調整必要 | `next_ws_message` 内のデフォルトタイムアウトに依存。debounce 仕様変更時は他 watcher テストも一斉に追従が必要なので、本テスト固有の追加リスクは無い |

## 検証計画

```bash
# 新規テスト単体
cargo test --test integration_test test_api_content_io_エラー
cargo test --test integration_test test_ファイル変更_io_エラー

# 既存 IO エラー透過テストの回帰確認
cargo test --test integration_test test_websocket_ioエラー
cargo test --test integration_test test_ファイル削除
cargo test --test integration_test test_ファイル変更

# 全体検証
./verify.sh
```

## 成功基準

- [ ] T1 `test_api_content_io_エラーで500を返す` が Unix で Pass する
- [ ] T2 `test_ファイル変更_io_エラーでwebsocketエラー通知` が Unix で Pass する
- [ ] 既存テスト（`test_websocket_ioエラーで...`、`test_ファイル変更で...`、`test_ファイル削除で...`）が全て Pass する
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` が Pass する
- [ ] `./verify.sh` が全て Pass する
- [ ] `TODO.md` Medium Priority の対象 2 項目にチェックが入る

## 将来の宿題

### `build_lagged_recovery_message` の Io arm 透過テスト

**スコープ外とした理由**：
- `broadcast::channel(16)` の容量飽和 + 受信側の意図的遅延 + その間に chmod 0o000 という 3 段階タイミング合成が必要
- テスト再現性が低く、もし lagged 経路自体が壊れた場合は構造的問題なので個別 chmod テストでは捕まえにくい
- 現状ユニットテストレベルでは `build_lagged_recovery_message` の挙動は確認可能（必要なら関数直接呼び出しの単体テストで Io arm を埋められる）

**再開トリガ**：
- lagged recovery 経路で本番障害が発生した場合
- broadcast 飽和の挙動を変更する PR が出る場合
- WebSocket クライアントの再接続ロジックで lagged 経路の error フィールドに新たに依存する場合

### Windows での同等テスト

**スコープ外とした理由**：`chmod 0o000` は POSIX 専用。Windows で `Io(_)` arm を踏ませるには ACL 操作（`icacls` 相当）か、replace + 共有違反などプラットフォーム固有の細工が必要。既存 1011 テストも `#[cfg(unix)]`。

**再開トリガ**：Windows サポートを公式化する場合、または Windows 環境で `Io(_)` arm に関する回帰が出た時。
