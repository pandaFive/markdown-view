# `build_lagged_recovery_message` IO エラー透過の統合テスト

## 背景と動機

`docs/todo/TODO.md` の Medium Priority 最終項目。`build_lagged_recovery_message` (`src/server/files/content.rs:129`) は、WebSocket クライアントが `RecvError::Lagged(n)` で取りこぼした際に呼ばれる回復メッセージ生成関数で、内部の `ReadMarkdownError::Io(_)` を `BroadcastMessage::Error(format!("ファイル読み込みエラー (...): ..."))` に畳み込む。

ユニットテスト (`src/server/files/tests.rs:1583-1680`) では `Refresh` / `LaggedRecovery` / `ResolveFailed` / `TooLarge` / `NotUtf8` の各分岐がカバー済だが、

- **Io variant の単体検証**
- **session.rs の `Lagged` arm から `build_lagged_recovery_message` を呼び、その結果を WS Text として送出するまでの配線**

の 2 点が未検証。リファクタで `ReadMarkdownError::Io(_)` のフォーマットや `BroadcastMessage::Error` への畳み込みが silent に変わると、遅延回復経路だけ検知されない回帰となる。

兄弟経路 `build_change_broadcast_message` には対応する IO 統合テスト `test_ファイル変更_io_エラーでwebsocketエラー通知` (`tests/integration_test.rs:850-889`) が存在する。本テストはこれと対称な位置づけで Lagged 経路を埋める。

なお TODO 著者は「`broadcast::channel(1)` の飽和などで lag recovery を意図的に発生させる必要があり、再現性が低いため将来の宿題」と保留していたが、`#[tokio::test]` のデフォルトである `current_thread` ランタイム上で `broadcast::Sender::send()` を await を挟まず連続実行すれば、受信タスクが起きる前にチャネルが決定的にオーバーフローする。再現性 100% が達成可能と判明したため本タスクを起こす。

## スコープ

`ReadMarkdownError::Io(_)` 経路 1 ケースのみ。

- TooLarge / NotUtf8 / ResolveFailed は既にユニットテストで variant 分岐が確認できているため、配線テストとして追加しない
- 兄弟経路（`build_change_broadcast_message`）の統合テストも IO のみで配線を保証している。対称性を維持

## 設計

### テストの位置と属性

| 項目 | 値 |
|------|----|
| ファイル | `tests/integration_test.rs` |
| 配置 | `test_websocket_ioエラーでclose_frameが1011を返す` (現 L1713) の直後 |
| 名前 | `test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する` |
| 属性 | `#[cfg(unix)]` + `#[tokio::test]` |
| ランタイム | current_thread（`flavor` 指定なし。決定性のキー） |

### 実行手順

1. tempdir 作成、`<name>.md` (例: `lagged_io.md`) に短いマークダウンを書き込む
2. `setup_single_file_server_from_path(&file_path)` で `AppState` と `addr` を取得
3. WS connect (`connect_ws(&url, &origin)`)、`split()` で `read` を分離
4. `next_ws_message(&mut read)` で初期 Update メッセージを 1 件消費 — session 側の rx が recv ループに入ったことの暗黙的バリア
5. `make_file_unreadable(&file_path)` で `FilePermissionGuard` を取得（chmod 0o000）。`None` が返る環境（root 等）はテストを早期 return（兄弟テストと同じ取り扱い）
6. **await を挟まず** 同期ループで `state.tx().send(BroadcastMessage::Refresh).unwrap()` を 17 回連続実行
   - 容量 16 の broadcast がオーバーフロー
   - 受信側の次 `recv()` は `Err(Lagged(1))` を確定的に返す
7. `next_ws_message(&mut read)` で次のフレームを取得し、テキスト・JSON パース
8. `assert_eq!(error, "ファイル読み込みエラー (lagged_io.md): ファイルの読み込みに失敗しました");`
9. `drop(permission_guard);` で原始 mode 復元
10. tempdir 自動 drop で teardown

### 検証される配線

```
broadcast::Sender::send() x17 (テストコード)
  → channel overflow (capacity 16)
  → session.rs:100 RecvError::Lagged(n) arm
  → build_lagged_recovery_message(state)
  → resolve_single_file_target / validate_and_render
  → ValidateRenderOutcome::ReadFailed(target, ReadMarkdownError::Io(_))   [chmod 0o000 で誘発]
  → BroadcastMessage::Error("ファイル読み込みエラー (lagged_io.md): ファイルの読み込みに失敗しました")
  → recovery.to_json()
  → socket.send(Message::Text(payload))                                    [session.rs:121]
  → クライアント側 next_ws_message → JSON parse → assert_eq!
```

未テストだった「Lagged → recovery 生成 → Io ReadFailed → JSON 直列化 → WS Text 送信」を単一テストで通す。

### 副作用と無害性

- burst-send した 17 件の `Refresh` のうち、Lagged 報告で位置 0 が落ち、後続 16 件 (位置 1〜16) は recv ループ復帰後に Refresh フレームとして送出される
- 本テストは **最初の 1 フレームのみ assert** し、即 `read` を drop して接続クローズ → 後続 Refresh フレームは無視されるだけで assert を汚染しない
- `tracing::warn!` 出力（"WebSocketクライアントが1メッセージ遅延" 等）はテスト時 captured で副作用なし

### 利用する既存ヘルパ

| ヘルパ | 役割 |
|--------|------|
| `setup_single_file_server_from_path` (`tests/integration_test.rs:1768`) | サーバ起動 |
| `connect_ws` (既存) | WS 接続 |
| `next_ws_message` (既存) | フレーム取得 |
| `make_file_unreadable` / `FilePermissionGuard` (`L1812-`) | RAII chmod 0o000 |
| `state.tx()` (`src/server/state.rs:240`) | broadcast::Sender へのアクセス |

新規ヘルパは作らない。

## YAGNI で除外

- TooLarge / NotUtf8 / ResolveFailed の Lagged 経路追加テスト（既ユニット網羅）
- 後続 16 Refresh フレームの内容検証（配線の関心外）
- テスト用に容量 1 の `broadcast::channel` を持つ別ヘルパ（prod 容量 16 で配線確認したい）
- `n` の値の assert（`session.rs` の Lagged arm は `n` に依存しない）

## リスク / 制限

| 項目 | 内容 | 影響 / 対応 |
|------|------|-----------|
| ランタイム依存 | `flavor = "multi_thread"` を将来テストファイル全体で指定すると、burst 中に session task が並走して途中 recv してしまい決定性が崩れる | 既存ファイルは `flavor` 指定なし (current_thread)。本テスト追加時もデフォルト維持。コード上のコメントで「current_thread 前提」を明示 |
| Unix 限定 | `chmod 0o000` 必須 | `#[cfg(unix)]` で兄弟 IO テストと同じ制限。Windows ユーザでは skip されるだけ |
| root 環境 | chmod が無視される / 読み取りが通る | `make_file_unreadable` が `None` を返す。早期 return で skip（兄弟テスト同等） |
| 後続 Refresh フレーム | 16 件の Text フレームが送出されるが assert しない | 接続を即 drop して無害化 |

## 受け入れ基準

1. `cargo test test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する` が Pass
2. ローカル Linux 環境（chmod 効く）で 10 連続実行しても flake 0
3. `cargo test` 全体が引き続き Pass
4. `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings` Pass
5. `./verify.sh` Pass
6. テスト本体に「current_thread runtime 前提」「burst 件数 = 容量 + 1」「権限復元順序」のコメントが入っている
7. `docs/todo/TODO.md` の `build_lagged_recovery_message` 行が `[x]` にマーク変更
