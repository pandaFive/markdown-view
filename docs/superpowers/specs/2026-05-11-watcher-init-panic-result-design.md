# watcher 初期化前 panic 結果返却設計

**作成日**: 2026-05-11
**対象 issue**: GitHub Issue #141「watcher 初期化前 panic を init 結果として返す」

## 目的

watcher thread が初期化結果を返す前に panic した場合、`Watcher::spawn(...).await` の戻り値として `WatchErrorKind::ThreadPanic` を返す。

現行実装では `init_tx` がまだ残っている段階で panic しても、panic 詳細は内部 `WatchEvent::Error` 補助通知に流れるだけで、初期化 API の戻り値には反映されない可能性がある。呼び出し側から見ると watcher 起動失敗の原因が init result で観測できず、初期化中 failure と稼働後 failure の境界が曖昧になる。

今回の変更では、init 前 panic は起動 API の失敗として返しつつ、既存の health failure と内部 error channel への `WatchEvent::Error` 補助通知も維持する。ただし init 前 panic では `Watcher::spawn()` が `Err` を返すため、公開 `WatchEvent` receiver や WebSocket/client への配送は保証しない。

## 非目的

- `WatchEvent` の公開 shape は変更しない。
- file/error 分離 channel と merge forwarder の設計は変更しない。
- WebSocket JSON shape、`BroadcastMessage::Error`、client 表示は変更しない。
- shutdown API と shutdown 中 panic の扱いは変更しない。
- 稼働後 panic を `Watcher::spawn()` の結果へ戻さない。spawn 成功後の panic は既存どおり health failed と `WatchEvent::Error` で扱う。
- watcher の自動再起動機構は追加しない。

## 採用方針

`src/watcher/runtime.rs` の `handle_watcher_panic` を panic detail の単一生成点として維持し、init result の返却責務を追加する。

`handle_watcher_panic` は `&mut Option<oneshot::Sender<InitResult>>` を受け取り、panic detail から `WatchError::thread_panic(...)` を生成する。`init_tx` が `Some` の場合は `send_init_result(init_tx, Err(watch_error.clone()))` で init result に返す。その後、同じ種別と detail を持つ `WatchError` を内部 error channel に送る。`init_tx` が `None` の場合は稼働後 panic として init result には触らず、従来どおり health failed と内部 error channel への補助通知だけを行う。

これにより、panic payload の文字列化、health latch、init result、補助通知の分類が同じ関数内で揃う。

## 代替案

### 案A: `handle_watcher_panic` に init 結果返却も持たせる

panic detail 抽出と `WatchError::thread_panic` 生成を既存の panic handler に集約する。init 前 panic と稼働後 panic は `init_tx.is_some()` で分岐する。重複が少なく、今回の問題に直接対応できるため採用する。

### 案B: `catch_unwind` の `Err` 分岐で init result だけ返す

`spawn_watcher_thread` の `if let Err(panic_payload)` 内で `init_tx` を見て init result を返す。変更箇所は局所的だが、panic detail 抽出や `WatchError::thread_panic` 生成が `handle_watcher_panic` と重複し、分類ずれが起きやすい。

### 案C: init 前 panic 専用 helper を新設する

`handle_initialization_panic` のような helper を作り、init 前と稼働後を明示的に呼び分ける。責務は読みやすいが、現時点では `handle_watcher_panic` と処理がほぼ同じになり、追加抽象としては過剰である。

## アーキテクチャ

### watcher thread panic handler

`handle_watcher_panic` は次の責務を持つ。

1. `Box<dyn Any + Send>` の panic payload から detail を抽出する。
2. `WatcherHealthState` を `Failed(WatcherFailureKind::ThreadPanic)` に latch する。
3. `WatchError::thread_panic(detail.clone())` を生成する。
4. `init_tx` が残っていれば `Err(WatchErrorKind::ThreadPanic)` として init result に返す。
5. `WatchEvent::Error` 用の内部 error channel に同じ panic detail を送る。
6. panic message と detail を error log に残す。

init result への返却は `send_init_result` を使い、既存の「二重送信時は warn して破棄」「receiver closed 時は warn」の契約を維持する。

### init 前と稼働後の境界

`spawn_watcher_thread` 内では `init_tx` を `Option` として保持している。debouncer 初期化失敗、watch plan 登録失敗、正常初期化完了のいずれかで `send_init_result` が呼ばれると `init_tx` は `None` になる。

このため、panic 捕捉時点で `init_tx.is_some()` なら init 前 panic、`None` なら init 完了後 panic と判定できる。追加の boolean state は導入しない。

## データフロー

### init 前 panic

1. watcher thread が `catch_unwind` 内で debouncer 初期化、watch 登録、または init result 送信前の処理中に panic する。
2. `catch_unwind` が panic payload を捕捉する。
3. `handle_watcher_panic` が `WatchError::thread_panic(detail)` を生成する。
4. `init_tx` が `Some` なので `send_init_result(..., Err(watch_error.clone()))` を呼ぶ。
5. `await_watcher_init` が `Err(WatchErrorKind::ThreadPanic)` を受け取り、`Watcher::spawn()` が失敗する。
6. health は `Failed(ThreadPanic)` になり、`WatchEvent::Error` も補助通知として内部 error channel に残る。ただし `Watcher::spawn()` は `Err` で戻るため、公開 receiver や WebSocket/client への配送は保証しない。

### init 完了後 panic

1. watcher thread が `send_init_result(&mut init_tx, Ok(()))` を呼び、`init_tx` が `None` になる。
2. その後の event loop で panic する。
3. `handle_watcher_panic` は `init_tx` が `None` であることを見て init result へは送らない。
4. health は `Failed(ThreadPanic)` になり、`WatchEvent::Error` が補助通知として送られる。

## エラー処理

- init receiver closed: `send_init_result` の既存 warn に任せる。panic handler 自体は panic しない。
- init result 二重送信: `send_init_result` の既存 warn に任せる。`init_tx.take()` 済みなら稼働後 panic と同じ扱いになる。
- error channel closed: `PriorityErrorSender` の既存 warn と破棄方針を維持する。
- panic payload が `&str`、`String`、`anyhow::Error` 以外: 既存どおり `"不明なパニック"` として扱う。

## テスト方針

TDD で進める。

`src/watcher/runtime.rs`:

- `handle_watcher_panic` に `Some(init_tx)` を渡すと、init receiver が `Err(WatchErrorKind::ThreadPanic)` を受け取る。
- init 前 panic 経路でも health が `Failed(WatcherFailureKind::ThreadPanic)` になり、error channel に `WatchErrorKind::ThreadPanic` が残る。
- `anyhow::Error` payload の detail が init result と error event の両方に保持される。
- `init_tx` が `None` の既存 panic handler テストは、health failed と error event のみを確認する稼働後 panic テストとして維持する。

可能であれば、panic handler を直接叩くユニットテストを中心にする。実 filesystem watcher の panic 注入は不要で、watcher thread 起動や notify backend の環境差をテストに持ち込まない。

## 受け入れ条件

- init 前 panic では `Watcher::spawn(...).await` が `WatchErrorKind::ThreadPanic` を返す。
- init 前 panic の detail は init result に保持される。
- init 前 panic でも `WatcherHealth` は `Failed(WatcherFailureKind::ThreadPanic)` になる。
- init 前 panic でも内部 error channel への `WatchEvent::Error` 補助通知は維持される。ただし公開 receiver や WebSocket/client への配送は保証しない。
- init 完了後 panic は `Watcher::spawn()` の戻り値には影響せず、既存どおり health failed と error event で扱う。
- `WatchEvent`、server broadcast、client 表示の契約は変わらない。
- `./verify.sh` が通る。

## セキュリティ考慮

panic detail は未信頼の診断文字列として扱う。ログと `BroadcastMessage::Error` の JSON 文字列以上の意味を持たせず、HTML、shell、SQL、ポリシー、コマンドとして解釈しない。

今回の変更は watcher 初期化結果の伝播だけであり、path validation、Host/Origin validation、HTML sanitization、CSP、file size limit、directory traversal protection は変更しない。

`WatchError::detail()` が外部由来文字列を含み得る点は既存と同じである。client 表示契約を変更しないため、error detail を DOM に raw HTML として挿入する新規経路は作らない。

## 影響範囲

- `src/watcher/runtime.rs`: `handle_watcher_panic` の引数と init result 返却、関連ユニットテスト。
- `src/watcher/error.rs`: 変更しない見込み。
- `src/watcher/mod.rs`: 変更しない。
- `src/server/watch.rs`: 変更しない。
- `docs/todo/TODO.md`: 実装完了時に issue 141 の追跡状態を更新対象にする。

## ロールバック

実装コミットを revert すれば、init 前 panic は旧挙動である health failed と内部 error channel への `WatchEvent::Error` 補助通知のみへ戻る。

問題が `handle_watcher_panic` の signature 変更に限定される場合は、init result 送信部分だけを削除し、呼び出し側から `init_tx` を渡さない形へ戻せる。

## 工数見積もり

人間の作業見積もり: 45-90 分。panic handler の小変更、ユニットテスト追加、既存 watcher tests の調整、`./verify.sh` を含む。

Codex / AI 支援込み見積もり: 20-45 分。変更範囲は狭いが、panic detail と init/error event の一致をテストで固定する必要がある。
