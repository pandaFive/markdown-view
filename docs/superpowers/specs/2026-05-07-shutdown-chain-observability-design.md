# shutdown チェーン観測性統合設計

## 目的

watcher shutdown 時に、停止 timeout の原因をログだけで切り分けられるようにする。

具体的には、監視スレッドと WebSocket 向け forwarder task の停止ログに共通の診断観点を持たせる。timeout / abort 直前には、経過時間、最後に処理した監視イベント種別、残存 WebSocket receiver 数、timeout 秒数を 1 行の warn ログで確認できるようにする。

これにより、HTTP サーバー再起動経路などでファイルハンドルリークや shutdown 遅延が疑われる場合に、「watcher thread が close しない」のか「forwarder task が mpsc close を待っている」のかをログから判断しやすくする。

## 非目的

- watcher shutdown の挙動は変更しない。
- forwarder task の abort 条件は変更しない。
- watcher 自動再起動機構は追加しない。
- HTTP API、WebSocket payload、UI 表示は追加しない。
- ファイル監視イベントの種類や WebSocket broadcast 契約は変更しない。
- Host/Origin 検証、CSP、HTML sanitization、パス検証の境界は変更しない。

## 影響範囲

- `src/server/broadcast.rs`
  - forwarder 専用の診断状態 tracker を追加する。
  - イベント処理時に最後のイベント種別を更新する。
  - forwarder 正常終了ログに診断 snapshot を含める。
- `src/server/watch.rs`
  - forwarder shutdown timeout 時の warn ログに診断 snapshot を含める。
  - watcher thread と forwarder task の timeout 秒数を共通定数へ寄せる。
- `src/watcher/runtime.rs`
  - watcher thread shutdown timeout が共通定数を使うようにする。

テストは `src/server/broadcast.rs` と `src/server/watch.rs` のユニットテスト中心に追加する。外部 API や E2E の観測可能 payload は変えないため、統合テストの追加は必須にしない。

## 設計

### timeout 定数

現在は watcher thread 側に `SHUTDOWN_TIMEOUT_SECS`、forwarder task 側に `WATCH_FORWARDER_SHUTDOWN_TIMEOUT_SECS` があり、どちらも 2 秒である。

これを `WATCH_SHUTDOWN_TIMEOUT_SECS` のような共通名へ寄せる。同期 thread と tokio task では停止手段が異なるため shutdown 処理自体は統合しない。共通化するのは値と意味だけに留める。

### forwarder 診断状態

`src/server/broadcast.rs` に forwarder 専用の内部状態を追加する。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchForwarderEventKind {
    FileChanged,
    Error,
}

struct WatchForwarderDiagnostics {
    last_event_kind: Arc<AtomicU8>,
}
```

名称は実装時に周辺コードへ合わせて調整してよい。重要なのは、forwarder の処理状態を `spawn_watch_event_forwarder` のローカル変数だけに閉じ込めず、shutdown 側から snapshot として読めるようにすることである。

forwarder は `WatchEvent::FileChanged` を処理する直前に `FileChanged` を記録し、`WatchEvent::Error` を処理する直前に `Error` を記録する。timeout 診断では「処理に入った最後のイベント」を見ることを優先する。

診断 snapshot は次の観点を持つ。

- `elapsed_ms`
- `timeout_secs`
- `last_event_kind`
- `receiver_count`

`receiver_count` は保持せず、snapshot 取得時に `state.tx().receiver_count()` から読む。これにより古い receiver 数をログに出さない。

### forwarder 終了ログ

`spawn_watch_event_forwarder` の mpsc receive loop が自然終了したとき、既存の info ログを維持しつつ structured field を追加する。

含める値は `last_event_kind` と `receiver_count` とする。これにより dropped events が疑われる shutdown でも、最後に処理したイベント種別と残存 receiver 数を確認できる。

### timeout / abort ログ

`src/server/watch.rs` の `shutdown_watch_forwarder` は、timeout した場合に `abort()` を呼ぶ直前で warn ログを出す。

この warn ログには次を structured field として含める。

- `elapsed_ms`
- `timeout_secs`
- `last_event_kind`
- `receiver_count`

ログメッセージ本文は既存の日本語メッセージを踏襲しつつ、「abort します」だけで終わらず shutdown 診断情報を field として出す。

`abort()` 後の join error ログは既存どおり残す。`JoinError::is_cancelled()` の場合に正常な abort 結果として return する挙動も変えない。

## エラー処理

診断 tracker の更新失敗で shutdown 処理を止めない。lock poisoning を避けるため、`Option<WatchForwarderEventKind>` は `AtomicU8` で表現し、async task と shutdown 側で軽量に共有する。

`WatchEvent::Error` の WebSocket broadcast は既存どおり維持する。今回追加する `last_event_kind = Error` は、error 内容そのものではなくイベント種別だけを記録する。

## テスト計画

- `WatchForwarderDiagnostics` の初期 snapshot が `last_event_kind = None` を返すことを確認する。
- `FileChanged` 記録後の snapshot が `FileChanged` を返すことを確認する。
- `Error` 記録後の snapshot が `Error` を返すことを確認する。
- forwarder の自然終了ログに `last_event_kind` と `receiver_count` が含まれることを `tracing_test` で確認する。
- forwarder shutdown timeout ログに `elapsed_ms`、`timeout_secs`、`last_event_kind`、`receiver_count` が含まれることを確認する。
- watcher thread と forwarder task が同じ timeout 定数を参照していることをコード構造で固定する。

最終検証は以下を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

## 受け入れ条件

- watcher thread と forwarder task の shutdown timeout 秒数が共通定数で表現されている。
- forwarder task が最後に処理へ入った `WatchEvent` 種別を内部状態として保持する。
- forwarder 自然終了ログで `last_event_kind` と `receiver_count` を確認できる。
- forwarder timeout / abort 直前ログで `elapsed_ms`、`timeout_secs`、`last_event_kind`、`receiver_count` を確認できる。
- shutdown の成功・timeout・abort の既存挙動が変わらない。
- HTTP API、WebSocket payload、UI 表示の外部契約が増えない。

## セキュリティ考慮

追加するログ項目は、経過時間、timeout 秒数、receiver 数、イベント種別に限定する。ファイルパス、監視エラー本文、外部から受け取った文字列は新しい shutdown 診断ログへ含めない。

`WatchEvent::Error` の詳細は既存経路どおり扱い、新しい診断状態では `Error` という種別だけを保持する。外部由来の notify error 文字列をコマンド、SQL、ポリシー、HTML として解釈する経路は作らない。

Host/Origin 検証、CSP、HTML sanitization、canonical path 検証、base 配下検証には触れない。WebSocket broadcast payload も変更しないため、クライアント側の信頼境界は現状維持とする。

## ロールバック

追加した forwarder 診断 tracker、共通 timeout 定数、ログ field、関連テストを revert すれば元に戻せる。

外部 API と WebSocket payload は変更しないため、ロールバック時の利用者向け互換性リスクは低い。ログ field を参照する運用手順が追加されている場合は、その手順だけを旧ログ形式へ戻す。
