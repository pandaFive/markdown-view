# watcher error event 送達保証設計

> **Superseded:** この設計は 2026-05-11 の `watcher 異常通知配送分離設計` に置き換えた。現行実装は `WatchEvent::Error` を `FileChanged` とは別の bounded ring queue に分離し、FileChanged backlog から独立して優先配送する。専用 queue 自体が満杯の場合は OOM を避けるため最古の error を warn log に残して evict し、最新の error を保持する。旧設計内の同期送信や完全送達保証は採用しない。

## 目的

watcher 内で発生した `WatchEvent::Error` が、`FileChanged` の大量発生と同じ `try_send` 経路で破棄されないようにする。

旧実装では `WatchEvent::FileChanged` と `WatchEvent::Error` を同じ `mpsc::Sender::try_send` で送っていたため、`WATCHER_MESSAGE_BUFFER` が満杯のときに notify error、internal channel full/disconnected、新規ディレクトリ監視追加失敗、thread panic などの異常通知も破棄され得た。これにより foreground 側が「監視が劣化・停止した理由」を受け取れない silent failure になり得た。

今回の設計は旧案として残す。現行実装は 2026-05-11 の設計に従い、通常変更イベントの過負荷耐性を維持しつつ、異常通知を別の bounded ring queue へ分離して優先配送する。

## 非目的

- `WatchEvent::FileChanged` の完全送達保証は追加しない。
- watcher 自動再起動は追加しない。
- 外部 health API、UI 表示、WebSocket JSON 形式は変更しない。
- `panic::catch_unwind` の init 前 panic を init result として返す別 TODO は扱わない。
- Host/Origin 検証、security headers、CSP、HTML sanitization、path validation の境界は変更しない。

## 影響範囲

- `src/watcher/runtime.rs`
  - file/error の入力経路を分け、merge forwarder で既存公開APIへ再統合する。
  - `WatchEvent::Error` の満杯時送達と receiver closed 経路のテストを追加する。
  - 既存の満杯時 drop テストを `FileChanged` の契約として明確化する。
- `src/server/broadcast.rs`
  - production code の変更は予定しない。既存の `WatchEvent::Error` forwarder 処理をそのまま利用する。

`docs/todo/TODO.md` は実装完了後の整理対象とし、この設計書作成時点では変更しない。

## アーキテクチャ

現行実装では watcher runtime 内部で `FileChanged` と `Error` の入力経路を分け、merge forwarder が既存公開APIの `WatchEvent` channel へ再統合する。

`WatchEvent::FileChanged` は現行どおり `mpsc::Sender::try_send` を使う。チャネル満杯時や receiver closed 時は warn ログを出し、イベントを破棄する。これはファイル変更通知の burst で watcher thread を詰まらせないための既存契約として維持する。

`WatchEvent::Error` は専用 bounded ring queue に積む。FileChanged backlog からは独立して優先配送し、queue 自体が満杯の場合は OOM を避けるため最古の error を evict して最新の error を保持する。

呼び出し側の `handle_debounced_watch_result`、`send_internal_watch_result`、`process_debounced_events_with_watch_and_unwatch`、`handle_watcher_panic` は、file/error の別 sender を通じてそれぞれの配送方針へ委譲する。

## データフロー

通常変更イベントの流れは変えない。

1. notify/debouncer から変更イベントを受け取る。
2. `WatchStrategy` が対象 Markdown path を収集する。
3. file channel へ best-effort に送る。
4. merge forwarder が公開用 `WatchEvent::FileChanged` に戻す。満杯なら warn して drop する。

異常通知は次の流れにする。

1. notify error、internal channel failure、watch registration failure、thread panic などを検出する。
2. 既存どおり `WatcherHealthState::store_failed(...)` を先に呼び、failure を latch する。
3. `WatchError` を error queue へ積む。
4. merge forwarder が error を優先して公開用 `WatchEvent::Error` に戻す。
5. error queue が満杯の場合は最古の error を evict し、warn ログへ残す。

health latch を error event 送信前に維持するため、仮に receiver 側が詰まっていても、`Watcher::health()` では先に `Failed(_)` を観測できる。

## エラー処理

`WatchEvent::Error` の送信失敗として扱うのは、公開用 receiver が閉じている場合である。この場合、foreground が既に終了している、または watch event stream が消費されない状態なので、再送機構は追加しない。代わりに error kind と context を warn ログへ残す。

shutdown 中に停止 timeout や forwarder panic を検出した場合も、可能な範囲で `WatchEvent::Error` として送る。merged channel が閉じている、または短い診断送信 timeout に達した場合は warn で終える。

thread panic 経路は `handle_watcher_panic` から error queue へ流すため、panic detail を含む `ThreadPanic` error event は FileChanged backlog だけでは破棄されなくなる。

## テスト計画

`src/watcher/runtime.rs` の既存 unit tests に以下を追加・更新する。

- `FileChanged` は channel full 時に従来どおり drop され、送信側をブロックしないことを確認する。
- `Error` は FileChanged backlog が満杯でも送達されることを確認する。
- receiver closed 状態で `Error` を送っても panic せず戻ることを確認する。
- 既存の notify error、internal channel full/disconnected、thread panic の health failure テストが引き続き通ることを確認する。

`Error` の backpressure テストは、公開用 channel や file channel を `FileChanged` で埋めた状態でも `Error` を受信できることを確認する。これにより、`Error` が FileChanged backlog で破棄されない契約を再現可能にする。

最終検証は以下を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

## 受け入れ条件

- `WatchEvent::Error` は FileChanged backlog が満杯でも優先配送される。
- `WatchEvent::FileChanged` は従来どおり過負荷時に drop され、watcher thread を詰まらせない。
- notify error、internal channel failure、watch registration failure、thread panic の `WatcherHealth` failure latch は維持される。
- WebSocket error payload、HTTP API、UI、外部設定は変わらない。
- 追加・更新した unit tests が通る。
- `cargo test --all-targets --all-features` と `./verify.sh` が通る。

## セキュリティ考慮

今回の変更は HTTP surface を増やさないため、Host/Origin 検証、security headers、CSP、HTML sanitization、path validation の適用範囲は変わらない。

`WatchError` の detail は既存どおりログと WebSocket error payload に使われる。外部由来になり得る notify error 文字列を、コマンド、SQL、HTML、ポリシーとして解釈しない。error event を送達しやすくすることで、監視失敗の隠蔽を減らし、異常時の検知性を上げる。

異常通知は bounded ring queue に限定し、通常の `FileChanged` burst で watcher thread が長時間詰まる経路や unbounded memory growth を増やさない。

## ロールバック

file/error 経路分離、merge forwarder、追加・更新テストを revert すれば元に戻せる。

外部 API、WebSocket payload、UI は変更しないため、ロールバック時の利用者向け互換性リスクは低い。残リスクは、error queue 自体が満杯の場合に古い error が evict されることだが、failure health は送信前に latch され、silent failure と unbounded queue の回避を優先する判断とする。
