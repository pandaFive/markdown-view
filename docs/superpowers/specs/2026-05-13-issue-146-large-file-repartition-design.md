# Issue 146: 大きい watcher/search/guards/test ファイル責務再分割設計

## 背景

Issue 146 は、watcher、search、guards、統合テスト周辺に大きいファイルが残り、機能追加時の追記集中とレビュー局所性低下を招いている問題を扱う。

現状確認では、主な対象は次の通り。

- `src/watcher/runtime.rs`: 3205行
- `src/server/files/tests.rs`: 3574行
- `tests/integration_test.rs`: 2335行
- `src/server/files/search.rs`: 1183行
- `src/server/guards.rs`: 1177行

今回の主対象は issue 本文で明示されている `watcher/runtime.rs`、`src/server/files/tests.rs`、`tests/integration_test.rs` とする。`search.rs` と `guards.rs` は本設計の観察対象には含めるが、実装範囲には含めない。必要が見つかった場合は別 issue として切り出す。

## ゴール

- 巨大ファイルを責務ごとの module/test file に分ける。
- 分割に合わせて watcher runtime の内部境界と型の所有先を整理する。
- 既存の public API、CLI 挙動、HTTP/WebSocket 契約、セキュリティ検証を維持する。
- 一つの作業ブランチ内で段階コミットし、各段階を独立して検証・revert できるようにする。

## 非ゴール

- watcher の監視戦略や debounced event 処理順の変更。
- search、memo、guards、renderer の外部仕様変更。
- `watcher/mod.rs` から公開している `Watcher`、`WatcherHealth`、`WatcherFailureKind`、`WATCH_SHUTDOWN_TIMEOUT_SECS` の公開面変更。
- `cargo test --test integration_test` の呼び出し名変更。
- 大規模な性能改善や新機能追加。

## 実装順序

### 1. `src/server/files/tests.rs` の分割

production code の責務境界に沿って、`src/server/files/tests.rs` を対象 module ごとの test module に分割する。

想定構成:

```text
src/server/files/tests/
  mod.rs
  support.rs
  memo_sidecar.rs
  resolve.rs
  catalog.rs
  content.rs
  memo_route.rs
  socket_update.rs
  change_broadcast.rs
```

`support.rs` には `create_test_dir`、`create_markdown_fixture`、permission helper、複数 module で共有される test-only `MemoFs` 実装だけを置く。単一 test file でしか使わない helper は、その file 内に残す。

既存の `src/server/files/test_support.rs` は、mock AppState や MockMemoFs など production module 側の unit test が使う基盤として残す。新しい `tests/support.rs` は巨大 test file 分割のための局所 helper に限定し、役割を混ぜない。

### 2. `tests/integration_test.rs` の分割

integration tests は production module 対応ではなく、HTTP/WebSocket 経由の外部挙動 spec 単位で分ける。

想定構成:

```text
tests/integration_test.rs
tests/integration/
  support.rs
  single_file.rs
  directory.rs
  memo.rs
  security.rs
  websocket.rs
  search.rs
  rendering.rs
```

`tests/integration_test.rs` は残し、`mod integration;` で束ねる。これにより、既存の `cargo test --test integration_test` を維持する。

分類方針:

- `single_file.rs`: 単一ファイル mode の index/content/default behavior
- `directory.rs`: directory mode の files/content/default/readme/tree
- `memo.rs`: memo API、legacy migration、atomic save/delete
- `security.rs`: Host/Origin、traversal、hidden file、symlink、CSP/security headers
- `websocket.rs`: connect、broadcast、close frame、lagged recovery
- `search.rs`: directory search、query length、raw query、result limit
- `rendering.rs`: rendered HTML、line attributes、tab UI visibility

複数カテゴリにまたがる test は、主に守っている契約で配置する。たとえば WebSocket origin 拒否は WebSocket の形をしていても、守っている契約は Origin security なので `security.rs` に置く。

### 3. `src/watcher/runtime.rs` の分割

テスト配置を整理した後に watcher 本体を分割する。外部公開面は維持し、内部 module 間の可視性は `pub(super)` または `pub(in crate::watcher::runtime)` に絞る。

想定構成:

```text
src/watcher/runtime/
  mod.rs
  health.rs
  error_queue.rs
  thread.rs
  dispatch.rs
  registration.rs
  shutdown.rs
```

責務:

- `health.rs`: `WatcherHealth`、`WatcherFailureKind`、`WatcherHealthState`
- `error_queue.rs`: `PriorityErrorSender`、`PriorityErrorReceiver`、`PriorityErrorQueue`、`priority_error_channel`、error queue 容量
- `registration.rs`: `WatchDirectoryRegistry`、`normalize_watch_registry_path`、`register_watch_plan_with`
- `dispatch.rs`: `WatchEventSenders`、`BestEffortFileSender`、`send_*_event`、debounced event 処理
- `thread.rs`: `WatchRuntime`、`spawn_watcher_thread`、`run_watcher_event_loop`、panic handling
- `shutdown.rs`: `WatcherThreadStopResult`、`MergeForwarderHandle`、`join_watcher_thread_with_timeout`、merge forwarder、shutdown diagnostic
- `mod.rs`: `Watcher` public methods、`Drop`、module re-export、外部に必要な定数

単なる移動に留めず、境界名と型の所有先を整理する。ただしアルゴリズム変更は行わない。debounced event の処理順、error queue の優先配送、dynamic directory registration の watch/unwatch 順、shutdown timeout と health 遷移は既存挙動を維持する。

## セキュリティ考慮

- 分割後も path traversal、symlink、非UTF-8、hidden/generated directory、atomic memo 書き込み前後の差し替え検証を維持する。
- integration helper の共有化で security-sensitive な前提を隠しすぎない。symlink や permission 操作を行う helper は、名前から危険な前提が分かるようにする。
- watcher が受け取る `WatchEvent::FileChanged(PathBuf)` は未検証入力として扱う前提を維持する。base 外 path の検証責務は server/files 側の revalidation に残す。
- Host/Origin、CSP、file size、path validation の既存テストが分割で抜け落ちないことを受け入れ条件に含める。

## 受け入れ条件

- 既存の public API と CLI 挙動が変わらない。
- `cargo test --test integration_test` が従来通り使える。
- watcher の health/error/shutdown/dynamic registration 系テストが移動後も通る。
- path traversal、symlink、Host/Origin、CSP、memo atomic save のセキュリティ系テストが維持される。
- 各巨大ファイルが責務単位に分かれ、単一ファイルへの追記集中が減る。
- `./verify.sh` が通る。

## 検証計画

段階ごとに次を実行する。

```text
server/files tests 分割後:
  cargo test --lib server::files

integration tests 分割後:
  cargo test --test integration_test

watcher runtime 分割後:
  cargo test --lib watcher
  cargo test --test integration_test
  ./verify.sh
```

docs-only の本設計書作成では、Markdown の placeholder、矛盾、scope、曖昧さを自己レビューし、`git diff --check` を validation として実行する。

## Rollback path

作業は次の段階コミットに分ける。

1. `src/server/files/tests.rs` 分割
2. `tests/integration_test.rs` 分割
3. `src/watcher/runtime.rs` 分割

問題が出た場合は、該当段階の commit だけを revert する。watcher 分割は最もリスクが高いため、テスト分割2段階とは独立した commit にする。

## 残留リスク

- Rust module split により、`pub(super)` 調整で意図せず可視性が広がる可能性がある。
- integration helper の共有化で、テストごとの前提が読み取りにくくなる可能性がある。
- watcher の concurrency 周辺で、移動中に shutdown、forwarder、error queue の結合を壊す可能性がある。
- `search.rs` と `guards.rs` は今回の実装範囲外のため、issue title に含まれる「search/guards 周辺の大きさ」は完全には解消されない。

## 見積もり

- Human effort: 1.5〜2.5日
- Codex/AI-assisted effort: 3〜6時間

watcher 分割後のコンパイルエラー収束、module visibility 調整、監視系テストの再確認で上下する。
