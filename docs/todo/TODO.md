# TODO Issues

## PRレビュー: セキュリティ強化とtemplate分割 (レビュー日: 2026-03-09)

### Low Priority

- [ ] `TargetResolveContext`の抽象度評価
  - ファイル: `src/server/files.rs` L24-44
  - 内容: 現状2メソッド×2バリアントで薄い抽象化。将来エンドポイント固有の処理が増えなければ`&'static str`パラメータに簡素化を検討
  - 理由: 過剰設計にならないよう定期的に評価

## PRレビュー: server/templateモジュール分割 (レビュー日: 2026-03-10)

### Low Priority

- [ ] `consume_initial_ws_message`のエラー無視を修正
  - ファイル: `tests/integration_test.rs` L1067付近
  - 内容: `let _ =` を `next_ws_message` に置き換え、テスト失敗を明示化
  - 理由: テストでのサイレントエラー防止

## PRレビュー: serverファサード化とサブモジュール分割 (レビュー日: 2026-03-10)

### Low Priority

- [ ] `MAX_FILE_SIZE`と`file_size_limit_error_message()`を`files.rs`に移動
  - ファイル: `src/server/messages.rs` → `src/server/files.rs`
  - 内容: ファイル読み込みの関心事を`files.rs`に集約し、ファサードから再エクスポート
  - 理由: `messages.rs`はメッセージ型の責務に集中すべき

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
