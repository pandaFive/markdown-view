# TODO Issues

## PRレビュー: セキュリティ強化とtemplate分割 (レビュー日: 2026-03-09)

### Low Priority

- [x] `TargetResolveContext`の抽象度評価 → `&'static str`パラメータに簡素化済み

## PRレビュー: server/templateモジュール分割 (レビュー日: 2026-03-10)

### Low Priority

- [ ] `consume_initial_ws_message`のエラー無視を修正
  - ファイル: `tests/integration_test.rs` L1067付近
  - 内容: `let _ =` を `next_ws_message` に置き換え、テスト失敗を明示化
  - 理由: テストでのサイレントエラー防止

## PRレビュー: serverファサード化とサブモジュール分割 (レビュー日: 2026-03-10)

### Low Priority

- [x] `MAX_FILE_SIZE`と`file_size_limit_error_message()`を`files.rs`に移動済み

- [ ] `file_size_limit_error_message()`を`&'static str`定数に変換
  - ファイル: `src/server/files.rs` L27-33
  - 内容: `FILE_SIZE_LIMIT_MB`はコンパイル時定数のため、毎回`format!`で`String`を生成する代わりに定数化してヒープ割り当てを削減
  - 理由: 不要なヒープ割り当ての排除

- [ ] `CanonicalPath`を`pub(super)`に降格
  - ファイル: `src/server/state.rs` L12
  - 内容: re-exportされず公開APIにも不使用のため可視性を縮小
  - 理由: 可視性の一貫性向上

- [ ] `relative_path_of`内の`tracing::warn!`を呼び出し側に移動
  - ファイル: `src/server/state.rs` L172-187
  - 内容: データ型メソッドから副作用（ログ出力）を分離し、呼び出し側で処理
  - 理由: データ型と副作用の分離

- [ ] `state.rs`の未使用テストヘルパー`create_single_file_state`を削除
  - ファイル: `src/server/state.rs` L333
  - 内容: `#[allow(dead_code)]`付きの未使用ヘルパーを削除
  - 理由: デッドコードの除去

- [ ] `AppState`に`#[derive(Debug)]`を追加
  - ファイル: `src/server/state.rs` L191
  - 内容: 診断性向上のためDebug traitを導出
  - 理由: サーバー状態のログ出力・デバッグ支援

## PRレビュー: watcher APIリファクタリング (レビュー日: 2026-03-10)

### 巨大な修正（要別途対応）

- [ ] [Medium] `WatchConfig`の`&'static str`フィールド6つを`WatchStrategy`メソッドに統合
  - ファイル: `src/watcher.rs` L32-43, L144-176
  - 影響範囲: `WatchConfig`, `WatchStrategy`, `spawn_watcher_thread`
  - 修正方針: `WatchStrategy`にラベル導出メソッドを追加し、`WatchConfig`を`(watch_dir, strategy)`に簡素化。`recursive_mode`も`strategy`から導出
  - 理由: 30行以上の変更が必要。コピペミスリスクの排除とコード簡素化

### Low Priority

- [ ] `handle_debounced_events()`のユニットテスト追加
  - ファイル: `src/watcher.rs` L273-327
  - 内容: SingleFile/Directoryの両戦略、非`.md`ファイル、隠しファイル、重複排除のテスト
  - 理由: コアイベント処理ロジックの回帰防止

- [ ] `Watcher::spawn`ディレクトリモードのend-to-endテスト追加
  - ファイル: `src/watcher.rs` テストモジュール
  - 内容: `test_watcher_spawn_単一ファイルモードでイベント受信できる`のディレクトリ版
  - 理由: ディレクトリモード固有のフィルタリングの検証

- [ ] `WatchStrategy`で`CanonicalPath`型を使用
  - ファイル: `src/watcher.rs` L26-29
  - 内容: `PathBuf`の代わりに既存の`CanonicalPath` newtypeを使い、正規化の不変条件を型で保証
  - 理由: 型安全性の向上

- [ ] `WatchEvent::Error(String)`の構造化エラー化
  - ファイル: `src/watcher.rs` L14-23
  - 内容: 将来コンシューマが増えた場合に`WatchErrorKind`列挙型への移行を検討
  - 理由: 現在は単一コンシューマのため優先度低
