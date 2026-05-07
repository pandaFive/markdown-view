# watcher health と atomic save 耐性設計

## 目的

watcher が silent に停止した状態を内部 API から区別できるようにし、エディタの atomic save 相当のファイル差し替え後も WebSocket 更新が届くことを回帰テストで固定する。

具体的には、`WatchService` から watcher の状態を enum として取得できるようにする。`ThreadPanic` 後は「監視スレッドが死んだ」状態を `Failed(ThreadPanic)` として表現し、単一ファイルモードとディレクトリモードの両方で `tmp/swp/backup -> rename` 後に更新通知を受け取れることを統合テストで確認する。

## 非目的

- HTTP の `/api/health` など新しい外部 API は追加しない。
- UI に watcher 状態を表示しない。
- notify watcher の自動再起動機構は追加しない。
- watcher 再帰監視の除外パターンや ENOSPC ユーザー文言は変更しない。
- WebSocket のエラーメッセージ形式は変更しない。
- Host/Origin 検証、CSP、HTML sanitization、パス検証の境界は変更しない。

## 影響範囲

- `src/watcher/runtime.rs`
  - watcher の状態保持と状態遷移を追加する。
  - `Watcher::health()` と `Watcher::is_alive()` を追加する。
- `src/server/watch.rs`
  - `WatchService::health()` と `WatchService::is_alive()` を追加する。
- `tests/integration_test.rs`
  - 単一ファイルモードの atomic save WebSocket 更新テストを追加する。
  - ディレクトリモードの atomic save WebSocket 更新テストを追加する。

状態 enum は `src/watcher/runtime.rs` に置く。外部 crate API として広げる必要はないため、公開範囲は現行の `markdown_view::server::WatchService` から使える最小限に留める。

## 状態モデル

`WatcherHealth` と `WatcherFailureKind` を追加する。

```rust
pub enum WatcherHealth {
    Starting,
    Alive,
    Failed(WatcherFailureKind),
    Stopping,
    Stopped,
}

pub enum WatcherFailureKind {
    Notify,
    ThreadPanic,
}
```

`WatcherFailureKind::Notify` は notify callback が `Err` を返した状態を表す。notify エラー後も watcher thread が継続する可能性はあるが、利用者視点では監視品質が劣化しているため `is_alive()` は false を返す。詳細な分類は `health()` から取得できる。

`WatcherFailureKind::ThreadPanic` は watcher thread の `catch_unwind` が panic を捕捉した状態を表す。この場合、監視スレッドは実際に停止しているため `is_alive()` は必ず false を返す。

初期化失敗は `Watcher::spawn()` が `Err` を返し、`WatchService` 自体が作られないため稼働中 health には含めない。

## 状態遷移

- thread spawn 直後から init 完了までは `Starting`
- init 成功後は `Alive`
- notify callback の `Err` 受信時は `Failed(Notify)`
- watcher thread panic 捕捉時は `Failed(ThreadPanic)`
- `shutdown()` または `Drop` で停止開始時は `Stopping`
- watcher thread の join 成功後は `Stopped`

停止タイムアウトで join できない場合は `Stopping` のまま残す。プロセス終了時に OS が回収する既存挙動は変えない。

`Failed(_)` は latch する。一度 failure を記録した後は、後続の `Stopping` / `Stopped` / 別種の failure で上書きしない。これにより shutdown や Drop が「停止前に既に監視品質が劣化していた」事実を消さない。

`WatchService::health()` は service が保持する `Watcher::health()` を返す。`WatchService::shutdown(self)` と `Watcher::shutdown(self)` は self を消費し、停止処理後の最終 `WatcherHealth` を返す。`WatchService::is_alive()` は `matches!(self.health(), WatcherHealth::Alive)` の convenience API とする。

## atomic save 統合テスト

統合テストは production の notify 経路を実際に通す。補助関数として、対象ファイルと同じディレクトリで以下の保存シーケンスを実行する。

1. `target.md.swp` に新内容を書く。
2. `target.md~` に旧内容相当を書く、または既存対象を backup 名へ rename する。
3. `target.md.swp` を `target.md` へ rename する。
4. backup を削除する。

単一ファイルモードでは、`AppMode::new_single_file(target.md)` で `WatchService` を開始する。WebSocket 初期メッセージを消費した後に atomic save を実行し、受信した update の `content` に新本文が含まれることを確認する。

ディレクトリモードでは、既存の directory server setup に合わせて `README.md` へ同じ atomic save を実行する。受信 JSON の `content` に新本文が含まれ、`file` が `README.md` であることを確認する。

一時ファイルや backup 名は `.md` ではなく `.swp` / `~` を使う。これにより、ディレクトリモードの `.md` フィルタが一時ファイルを誤通知しないことも間接的に固定する。

## エラー処理

既存の `WatchError` と `WatchEvent::Error` は維持する。`WatchErrorKind::Notify` を受けたときは health を `Failed(Notify)` にし、`WatchErrorKind::ThreadPanic` を受けたときは health を `Failed(ThreadPanic)` にする。

`WatchEvent::Error` の WebSocket broadcast は既存どおり継続する。今回の変更は通知内容ではなく、watcher の状態を内部から問い合わせ可能にするための変更である。

## テスト計画

- `Watcher` のユニットテストで、生成直後 `Alive`、shutdown 後 `Stopped` を確認する。
- `Watcher` の内部 helper またはテスト用構成で `Failed(ThreadPanic)` への遷移を固定する。
- `Watcher` の notify error 経路で `Failed(Notify)` への遷移を固定する。
- `Failed(_)` が shutdown 後も `Stopped` で上書きされないことを固定する。
- `shutdown()` が停止後の `WatcherHealth` を返すことを固定する。
- `WatchService::health()` と `WatchService::is_alive()` のユニットテストを追加する。
- 単一ファイルモードの atomic save WebSocket 統合テストを追加する。
- ディレクトリモードの atomic save WebSocket 統合テストを追加する。

最終検証は以下を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

## 受け入れ条件

- `WatchService` から watcher 状態を enum として取得できる。
- `WatchService::is_alive()` が `Alive` の場合のみ true を返す。
- `ThreadPanic` が `Failed(ThreadPanic)` として表現される。
- notify error が `Failed(Notify)` として表現される。
- failure 状態が latch され、shutdown / Drop で失われない。
- shutdown 呼び出し側が最終 `WatcherHealth` を取得できる。
- 単一ファイルモードで atomic save 後に WebSocket update が届く。
- ディレクトリモードで atomic save 後に WebSocket update が届き、`file` が対象 Markdown ファイルを指す。
- HTTP API、UI、WebSocket エラー JSON の外部契約が増えない。

## セキュリティ考慮

今回の変更は HTTP surface を増やさないため、Host/Origin 検証、security headers、CSP、HTML sanitization の適用範囲は変わらない。

watcher health は内部状態として扱い、外部入力で任意に変更できる経路を作らない。`WatchError` の detail は既存どおりログと WebSocket エラーに使われるため、外部由来の notify error 文字列を新たにコマンド、SQL、ポリシー、HTML として解釈しない。

atomic save テストでは同一一時ディレクトリ内の test fixture だけを rename し、パストラバーサルや symlink 境界は変更しない。既存の canonical path 検証と base 配下検証を迂回しない。

## ロールバック

追加した `WatcherHealth`、`WatcherFailureKind`、`health()`、`is_alive()`、および関連テストを revert すれば元に戻せる。

HTTP/API/UI は増やさないため、ロールバック時の利用者向け互換性リスクは低い。atomic save テストだけを戻す場合も production code の動作契約は変わらない。
