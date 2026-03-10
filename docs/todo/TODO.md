# TODO Issues

## PRレビュー: セキュリティ強化とtemplate分割 (レビュー日: 2026-03-09)

### Low Priority

- [x] `TargetResolveContext`の抽象度評価 → `&'static str`パラメータに簡素化済み

## PRレビュー: server/templateモジュール分割 (レビュー日: 2026-03-10)

### Low Priority

- [x] `consume_initial_ws_message`のエラー無視を修正済み

## PRレビュー: serverファサード化とサブモジュール分割 (レビュー日: 2026-03-10)

### Low Priority

- [x] `MAX_FILE_SIZE`と`file_size_limit_error_message()`を`files.rs`に移動済み

- [x] `file_size_limit_error_message()`を`&'static str`定数に変換済み

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

## PRレビュー: server files取得フロー集約 (レビュー日: 2026-03-10)

### 巨大な修正（要別途対応）

- [ ] [Medium] validate-build-renderパターンの3重重複を共通ヘルパーに抽出
  - ファイル: `src/server/files.rs` L148-260
  - 影響範囲: `initial_socket_update`, `lagged_recovery_broadcast_message`, `update_broadcast_message`
  - 修正方針: 共通の`validate_and_render`ヘルパーを抽出し、各関数をエラーマッピングのみのラッパーに
  - 理由: 3関数のエラーハンドリング戦略が異なり統一にアーキテクチャ検討が必要

### Low Priority

- [ ] `file_label`算出ロジックが`ResolvedTarget::new`と`update_broadcast_message`で重複
  - ファイル: `src/server/files.rs` L217-220
  - 内容: 共通関数`file_display_name(path: &Path) -> String`を抽出
  - 理由: Issue #1（パラメータ削除）修正後に再評価

- [ ] `lagged_recovery_message`が単純な委譲関数。直接呼び出しで除去可能
  - ファイル: `src/server/websocket.rs` L27-29
  - 内容: `lagged_recovery_broadcast_message`を直接呼び出しに変更
  - 理由: websocket.rsとfiles.rsの両方を変更する必要あり

- [ ] `handle_socket`内の`if let Some` + `match`のネストを2ステップに分離
  - ファイル: `src/server/websocket.rs` L34-40
  - 内容: 中間変数に束縛してから`if let`で分岐
  - 理由: 可読性改善のみでリスクに見合わない

- [ ] `ResolvedTarget::update`メソッド名を`attach_file_info`等に改名
  - ファイル: `src/server/files.rs` L69
  - 内容: `UpdateMessage`との名前衝突を解消
  - 理由: 全呼び出し元に影響し他の修正と同時に行うと差分が大きくなる
