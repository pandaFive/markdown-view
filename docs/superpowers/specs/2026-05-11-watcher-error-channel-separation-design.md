# watcher 異常通知配送分離設計

**作成日**: 2026-05-11
**対象 issue**: GitHub Issue #140「watcher の異常通知配送を通常変更通知から分離する」

## 目的

watcher runtime で通常の `WatchEvent::FileChanged` と異常系の `WatchEvent::Error` が同じ bounded `mpsc` チャネルに乗り、チャネル満杯時にどちらも `try_send` で破棄される問題を解消する。

通常変更通知は高頻度に発生しうるため、現行どおり best-effort として過負荷時に破棄してよい。一方、notify error、watcher internal channel 異常、追加 watch 失敗、watcher panic などの異常通知は、通常変更通知の backlog と同列に破棄しない。

`Watcher::spawn()` は引き続き `(Watcher, mpsc::Receiver<WatchEvent>)` を返し、`WatchEvent` と WebSocket broadcast 契約は維持する。停止 API は内部 forwarder の完了待ちで async runtime を塞がないように、`Watcher::shutdown(self) -> WatcherHealth` から `Watcher::shutdown(self).await -> WatcherHealth` へ変更する。

## 非目的

- `Watcher::spawn()` の公開戻り値を 2 receiver に変更しない。
- WebSocket JSON shape、`BroadcastMessage::Error`、client 側表示を変更しない。
- `WatcherHealth` / `WatcherFailureKind` の公開型を増やさない。
- watcher の自動再起動機構を追加しない。
- すべての異常通知を無限に保持する durable queue は作らない。
- path 解決、HTML sanitize、CSP、Host/Origin 検証などのセキュリティ境界は変更しない。

## 採用方針

watcher 内部で通常変更通知と異常通知の入力経路を分け、外部へ返す直前に既存の `WatchEvent` に再統合する。

`Watcher::spawn()` は次の内部チャネルを作る。

- `file_tx/file_rx`: `PathBuf` または file event 専用型を流す bounded channel。容量は既存 `WATCHER_MESSAGE_BUFFER` を使い、送信は `try_send`。
- `error_tx/error_rx`: `WatchError` を流す異常通知専用 bounded channel。容量は `WATCHER_ERROR_MESSAGE_BUFFER` として明示し、送信は `try_send` を使う。通常変更通知とは別容量にし、`FileChanged` の満杯で破棄されない。専用 channel 自体が満杯の場合は watcher thread を停止不能にしないため `error!` log に残して破棄する。
- `merged_tx/merged_rx`: 既存公開 API 用の bounded `mpsc::Sender<WatchEvent>` / `Receiver<WatchEvent>`。

watcher 内部 forwarder が `file_rx` と `error_rx` を読み、`WatchEvent::FileChanged` / `WatchEvent::Error` に戻して `merged_tx` へ転送する。caller には `merged_rx` だけを返すため、`src/server/watch.rs` と `src/server/broadcast.rs` の公開的な扱いは維持する。

内部 forwarder は error 側を優先する。両方の receiver にイベントがある場合、まず `error_rx` を drain し、その後 `file_rx` を処理する。これにより大量の file event が発生しても、error event が同じ入力キューの後ろに埋もれることを避ける。

## 代替案

### 案A: 公開 API も 2 receiver 化する

`Watcher::spawn()` が `WatchReceivers { file_events, error_events }` を返し、server forwarder 側で `tokio::select!` する。分離は型として最も明確だが、`server/watch.rs` と `server/broadcast.rs` まで影響が広がる。今回の目的は watcher 内部の配送保証強化なので採用しない。

### 案B: 内部 2 channel + 再統合する

watcher 内部では file/error を分け、`Watcher::spawn()` と `WatchEvent` の公開契約を維持する。#140 の「通常変更通知と異常通知を同じ best-effort queue に乗せない」という目的を満たしつつ、server 層の受信契約変更を避けられるため採用する。停止 API は `shutdown().await` へ変更する。

### 案C: 1 channel 維持で Error だけ blocking send にする

実装量は最小だが、Error は同じ queue 上で FileChanged の背後に並ぶ。通常変更通知の backlog と異常通知が構造的に分離されないため、今回の設計方針には合わない。

## アーキテクチャ

### watcher runtime 送信境界

既存の `send_watch_event(tx, WatchEvent, label)` を variant 混在の境界として使い続けない。代わりに次の helper に分ける。

- `send_file_changed_event(file_tx, path, label)`: `try_send`。満杯時は warn log を出して破棄する。
- `send_error_event(error_tx, error, label)`: error 専用 channel へ `try_send` する。file channel の満杯とは独立させる。error channel full 時は `error!`、closed 時は `warn!` に残して破棄し、watcher thread の停止不能化を避ける。

notify callback、debounced event 処理、追加 watch 失敗、internal channel 異常、panic 経路は、それぞれイベント種別に応じて専用 helper を呼ぶ。

### 内部統合 forwarder

`Watcher::spawn()` は内部統合 forwarder を起動する。forwarder は `file_rx` / `error_rx` から受け取った内部イベントを `merged_tx` へ流す。

転送規則:

- `error_rx` に保留イベントがあれば、`WatchEvent::Error` を優先して送る。
- `file_rx` の保留イベントは `WatchEvent::FileChanged` として送る。
- `merged_tx` が閉じた場合は forwarder を終了する。
- `file_rx` と `error_rx` が両方閉じた場合も forwarder を終了する。

`merged_tx` は外部 receiver 側の処理停止を表すため、ここでは無限蓄積しない。error 入力は file 入力 backlog からは守るが、server forwarder が完全停止している場合の最終状態は既存どおり health latch とログで観測する。

### `WatchRuntime` の所有関係

`WatchRuntime` は watcher thread に加えて内部統合 forwarder の停止ハンドルを保持する。

`Watcher::shutdown().await` は watcher thread の join と内部 forwarder の完了待ちを blocking pool に隔離する。これにより current-thread runtime 上で明示停止しても、内部 forwarder が完了通知を送るための Tokio runtime を塞がない。`Drop` は async にできないため同期 fallback として残し、明示停止は `shutdown().await` を正規経路とする。

停止順:

1. `shutdown_flag` を立てる。
2. watcher thread の終了を待つ。
3. watcher thread 側の `file_tx` / `error_tx` drop により内部 receiver を close させる。
4. 内部統合 forwarder の終了を待つ。
5. timeout 時は warn log を残し、既存の停止 timeout 方針に合わせて abort または join failure を処理する。

## データフロー

### FileChanged

1. watcher runtime が Markdown 変更候補 path を検出する。
2. 既存の `WatchStrategy::collect_changed_paths()` と後段の再検証契約は維持する。
3. `send_file_changed_event(file_tx, path, strategy.change_label())` を呼ぶ。
4. `file_tx.try_send(path)` が成功すれば内部 forwarder へ渡る。
5. 満杯なら warn log を出し、その file event は破棄する。
6. 内部 forwarder が `WatchEvent::FileChanged(path)` として `merged_tx` に流す。
7. server forwarder が既存どおり `notify_update()` を呼ぶ。

### Error

1. watcher runtime が `WatchError` を生成する。
2. 必要な経路では既存どおり `WatcherHealthState` を `Failed(...)` に latch する。
3. `send_error_event(error_tx, error, strategy.error_label())` を呼ぶ。
4. `error_tx` は `file_tx` と別なので、file event backlog により即時破棄されない。
5. 内部 forwarder が error 側を優先して `WatchEvent::Error(error)` を `merged_tx` に流す。
6. server forwarder が既存どおり `BroadcastMessage::Error` を送る。

## エラー処理

- file channel full: 現行どおり warn log + 破棄。
- file channel closed: watcher shutdown 中なら終了文脈として扱い、過剰に騒がせない。稼働中に発生した場合は warn log。
- error channel full: `error!` log に detail を残して破棄する。FileChanged backlog とは分離されるが、専用 channel 自体の過負荷で watcher thread をブロックしない方針とする。
- error channel closed: panic せず warn/error log。元の notify/panic failure は health に latch 済みとする。
- merged channel closed: 外部 receiver が閉じた状態なので内部 forwarder を終了する。
- internal forwarder panic/join failure: shutdown 時に warn/error log を残す。元の watcher health を不用意に `Stopped` へ上書きしない。

## テスト方針

TDD で進める。

`src/watcher/runtime.rs`:

- `FileChanged` は file channel 満杯時に破棄され、送信 helper がブロックしない。
- `Error` は file channel が満杯でも error channel 経由で `merged_rx` に届く。
- file/error の両方に保留イベントがある場合、内部 forwarder は `Error` を先に `merged_rx` へ送る。
- error channel receiver closed 時に `send_error_event` は panic しない。
- `send_internal_watch_result` の internal channel full 経路は health failed と error event を維持する。
- notify error 経路は health failed と error event を維持する。
- watcher panic 経路は health failed と error event を維持する。
- `Watcher::spawn()` の単一ファイル、ディレクトリ、新規サブディレクトリ監視の既存統合テストを維持する。

内部 forwarder は純粋 helper に切り出し、実 filesystem watcher を起動せずに file/error 優先順位と close 条件をユニットテストする。

## 受け入れ条件

- `WatchEvent::FileChanged` は過負荷時に best-effort で破棄される既存仕様を維持する。
- `WatchEvent::Error` は `FileChanged` 用 channel の満杯で破棄されない。
- file/error が同時に保留される場合、error が優先して外部 `WatchEvent` receiver に届く。
- `Watcher::spawn()` の公開戻り値は変わらない。
- `Watcher::shutdown()` は async API になり、呼び出し側は `.await` して最終 `WatcherHealth` を受け取る。
- `server/watch.rs` と `server/broadcast.rs` の外部契約は変わらない。
- watcher failure の health latch は既存テストどおり維持される。
- shutdown 時に watcher thread と内部 forwarder が停止し、timeout 時は診断ログが残る。
- `./verify.sh` が通る。

## セキュリティ考慮

notify event、filesystem path、error detail は未信頼入力として扱う。今回の変更は配送経路の分離であり、path の安全性は引き続き canonical base 検証、`resolve_change_target()`、読み込み直前再検証で守る。

`WatchError::detail()` は外部由来の文字列を含みうる。server forwarder は既存どおり `BroadcastMessage::Error` の JSON 文字列として送信し、HTML として解釈しない。client 側の表示契約を変更しないため、error detail を shell、SQL、HTML、ポリシー、コマンドとして扱わない。

Error を優先配送しても、存在しない path や base 外 path の情報を新たに公開しない。今回の変更で直接表示、検索、memo、renderer の入力境界は広げない。

残リスクとして、error 専用 channel 自体が満杯の場合は異常通知をブロックして保持せず、`error!` log に残して破棄する。これは watcher thread の停止不能化を避けるための trade-off であり、`FileChanged` backlog からの分離と health latch による観測性は維持する。

## 影響範囲

- `src/watcher/runtime.rs`: 内部 channel 分離、統合 forwarder、送信 helper、shutdown handling、テスト。
- `src/watcher/mod.rs`: 公開 `WatchEvent` shape は変更しない。
- `src/server/watch.rs`: `Watcher::shutdown().await` に委譲し、内部 forwarder の完了待ちで async runtime を塞がないようにする。
- `src/server/broadcast.rs`: 変更しない。
- `docs/todo/TODO.md`: 実装完了時に対象 TODO / issue の追跡状態を更新する。

## ロールバック

実装コミットを revert すれば、従来の単一 `mpsc::Sender<WatchEvent>` と `send_watch_event(... try_send ...)` に戻せる。

内部 forwarder だけに問題が出た場合は、`Watcher::spawn()` を単一 channel 生成へ戻し、送信 helper を旧 `send_watch_event` に戻す一時 rollback が可能。ただし #140 の Error 配送分離効果は失われる。

## 工数見積もり

人間の作業見積もり: 4-7 時間。内部 channel 分離、forwarder 停止処理、既存 watcher tests の調整、shutdown 回帰確認、`./verify.sh` を含む。

Codex / AI 支援込み見積もり: 2-4 時間。既存 watcher runtime のテストが厚く、境界は `send_watch_event` 周辺に集約されているが、shutdown と優先順位のテストは慎重に固定する必要がある。
