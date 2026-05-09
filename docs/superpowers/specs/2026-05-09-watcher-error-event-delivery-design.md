# watcher error event 送達保証設計

## 目的

watcher 内で発生した `WatchEvent::Error` が、`FileChanged` の大量発生と同じ `try_send` 経路で破棄されないようにする。

現状の `send_watch_event` は `WatchEvent::FileChanged` と `WatchEvent::Error` を同じ `mpsc::Sender::try_send` で送るため、`WATCHER_MESSAGE_BUFFER` が満杯のときに notify error、internal channel full/disconnected、新規ディレクトリ監視追加失敗、thread panic などの異常通知も破棄される。これにより foreground 側が「監視が劣化・停止した理由」を受け取れない silent failure になり得る。

今回の変更では、通常変更イベントの過負荷耐性は維持しつつ、異常通知の代表イベントを receiver が開いている限り送達する。送達中に追加発生した異常通知は watcher 単位で件数集約し、unbounded queue は作らない。

## 非目的

- `WatchEvent::FileChanged` の完全送達保証は追加しない。
- watcher 自動再起動は追加しない。
- 外部 health API、UI 表示、WebSocket JSON 形式は変更しない。
- `panic::catch_unwind` の init 前 panic を init result として返す別 TODO は扱わない。
- Host/Origin 検証、security headers、CSP、HTML sanitization、path validation の境界は変更しない。

## 影響範囲

- `src/watcher/runtime.rs`
  - `send_watch_event` をイベント種別ごとの送信方針に分ける。
- `WatchEvent::Error` の満杯時代表送達、送達中追加エラーの集約、helper thread 起動失敗時 fallback、receiver closed 経路のテストを追加する。
  - 既存の満杯時 drop テストを `FileChanged` の契約として明確化する。
- `src/server/broadcast.rs`
  - production code の変更は予定しない。既存の `WatchEvent::Error` forwarder 処理をそのまま利用する。

`docs/todo/TODO.md` は実装完了後の整理対象とし、この設計書作成時点では変更しない。

## アーキテクチャ

`send_watch_event` は引き続き watcher runtime 内の単一送信窓口とする。ただし、内部で `WatchEvent` の variant を見て送信方針を分ける。

`WatchEvent::FileChanged` は現行どおり `mpsc::Sender::try_send` を使う。チャネル満杯時や receiver closed 時は warn ログを出し、イベントを破棄する。これはファイル変更通知の burst で watcher thread を詰まらせないための既存契約として維持する。

`WatchEvent::Error` は watcher 単位の bounded helper に代表エラーとして渡し、`mpsc::Sender::blocking_send` で送る。チャネルが満杯でも receiver が開いていれば、空きができるまで待って代表エラーを送達する。すでに代表エラーの送達が進行中なら、追加 `Error` は event として積まず件数だけ集約する。receiver が閉じている場合だけ `SendError` として warn ログに残し、送達不能を観測可能にする。

helper thread の起動に失敗した場合は、同じ代表エラーを現在スレッドから同期 `blocking_send` へ fallback する。これは thread/resource exhaustion 時にも silent drop しないための異常系 fallback であり、通常の `FileChanged` burst 経路には適用しない。

呼び出し側の `handle_debounced_watch_result`、`send_internal_watch_result`、`process_debounced_events_with_watch_and_unwatch`、`handle_watcher_panic` は、原則として `send_watch_event(...)` を呼ぶままにする。error 送達保証の差分は送信 helper が吸収する。

## データフロー

通常変更イベントの流れは変えない。

1. notify/debouncer から変更イベントを受け取る。
2. `WatchStrategy` が対象 Markdown path を収集する。
3. `send_watch_event(WatchEvent::FileChanged(...))` を呼ぶ。
4. `try_send` が成功すれば forwarder へ流れ、満杯なら warn して drop する。

異常通知は次の流れにする。

1. notify error、internal channel failure、watch registration failure、thread panic などを検出する。
2. 既存どおり `WatcherHealthState::store_failed(...)` を先に呼び、failure を latch する。
3. `WatchError` を `WatchEvent::Error` に包んで `send_watch_event(...)` を呼ぶ。
4. 送達中の代表エラーがなければ bounded helper で `blocking_send` し、helper thread 起動失敗時は同期 fallback で receiver へ送達する。
5. 送達中の代表エラーがあれば追加 `Error` は件数集約し、代表エラー送達後に集約件数を warn ログへ残す。
6. receiver closed の場合は warn し、送達不能だった事実をログに残す。

health latch を error event 送信前に維持するため、仮に receiver 側が詰まり代表エラーの `blocking_send` が待っていても、`Watcher::health()` では先に `Failed(_)` を観測できる。

## エラー処理

`WatchEvent::Error` の送信失敗として扱うのは、receiver が閉じている場合だけである。この場合、foreground が既に終了している、または watch event stream が消費されない状態なので、再送機構は追加しない。代わりに `label` と `error_kind` を warn ログへ残す。

shutdown 中に `WatchEvent::Error` が発生した場合も同じ送信 helper を使う。receiver が生きていれば送達し、閉じていれば warn で終える。停止処理の timeout、panic join、health latch の方針は変更しない。

thread panic 経路は `handle_watcher_panic` から同じ `send_watch_event(WatchEvent::Error(...))` を通るため、panic detail を含む `ThreadPanic` error event は channel full だけでは破棄されなくなる。

## テスト計画

`src/watcher/runtime.rs` の既存 unit tests に以下を追加・更新する。

- `FileChanged` は channel full 時に従来どおり drop され、送信側をブロックしないことを確認する。
- `Error` は channel full 時でも receiver が開いていれば代表イベントが送達されることを確認する。
- 送達中の追加 `Error` は全件 queue へ積まず件数集約されることを確認する。
- helper thread 起動失敗時でも同期 fallback で代表 `Error` が送達され、送達状態が解除されることを確認する。
- 別 watcher state の送達待機が互いに詰まらないことを確認する。
- receiver closed 状態で `Error` を送っても panic せず戻ることを確認する。
- 既存の notify error、internal channel full/disconnected、thread panic の health failure テストが引き続き通ることを確認する。

`Error` の channel full テストは、容量 1 の channel を `FileChanged` で埋めた状態で `send_watch_event(WatchEvent::Error(...))` を呼ぶ。メイン側が先に `FileChanged` を drain した後、代表 `Error` を受信できることを確認する。helper thread 起動失敗テストは spawner を注入し、失敗時に同期 fallback が代表 `Error` を送ることを固定する。

最終検証は以下を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

## 受け入れ条件

- `WatchEvent::Error` は `mpsc` channel が満杯でも、receiver が開いている限り代表イベントが破棄されない。
- 代表 `Error` の送達中に発生した追加 `Error` は件数集約され、unbounded queue を作らない。
- helper thread 起動失敗時も同期 fallback で代表 `Error` を送達する。
- `WatchEvent::FileChanged` は従来どおり過負荷時に drop され、watcher thread を詰まらせない。
- notify error、internal channel failure、watch registration failure、thread panic の `WatcherHealth` failure latch は維持される。
- WebSocket error payload、HTTP API、UI、外部設定は変わらない。
- 追加・更新した unit tests が通る。
- `cargo test --all-targets --all-features` と `./verify.sh` が通る。

## セキュリティ考慮

今回の変更は HTTP surface を増やさないため、Host/Origin 検証、security headers、CSP、HTML sanitization、path validation の適用範囲は変わらない。

`WatchError` の detail は既存どおりログと WebSocket error payload に使われる。外部由来になり得る notify error 文字列を、コマンド、SQL、HTML、ポリシーとして解釈しない。error event を送達しやすくすることで、監視失敗の隠蔽を減らし、異常時の検知性を上げる。

`blocking_send` は異常系の代表 `Error` だけに限定する。通常の `FileChanged` burst で watcher thread が長時間詰まる経路は増やさない。送達中の追加 `Error` は全件 payload としては残さず、件数ログで観測する。

## ロールバック

`send_watch_event` の variant 分岐、`WatchEvent::Error` 用 bounded helper / fallback、追加・更新テストを revert すれば元に戻せる。

外部 API、WebSocket payload、UI は変更しないため、ロールバック時の利用者向け互換性リスクは低い。残リスクは、receiver が長時間詰まっている場合に代表 error 送信中の helper または fallback 実行スレッドが待つこと、および送達中の追加 `Error` が全件 event としては届かないことだが、failure health は送信前に latch され、silent failure と unbounded queue の回避を優先する判断とする。
