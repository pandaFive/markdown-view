# TODO Issues

## PRレビュー: セキュリティ強化とtemplate分割 (レビュー日: 2026-03-09)

### Low Priority

- [ ] `TargetResolveContext`の抽象度評価
  - ファイル: `src/server.rs` L356-376
  - 内容: 現状2メソッド×2バリアントで薄い抽象化。将来エンドポイント固有の処理が増えなければ`&'static str`パラメータに簡素化を検討
  - 理由: 過剰設計にならないよう定期的に評価

## PRレビュー: server/templateモジュール分割 (レビュー日: 2026-03-10)

### Low Priority

- [ ] 新サブモジュールにモジュールレベルdoc(`//!`)を追加
  - ファイル: `src/server/files.rs`, `src/server/guards.rs`, `src/server/websocket.rs`, `src/template/assets/css_bundle.rs`, `src/template/assets/inline_script.rs`
  - 理由: 各モジュールの責務を明示し保守性を向上

- [ ] `websocket.rs`のclose-frame送信パターンをヘルパー関数に抽出
  - ファイル: `src/server/websocket.rs` L62-112
  - 内容: 3箇所の`Message::Close(Some(CloseFrame{...}))`パターンを共通化
  - 理由: DRY原則、コード重複の削減

- [ ] `consume_initial_ws_message`のエラー無視を修正
  - ファイル: `tests/integration_test.rs` L1067付近
  - 内容: `let _ =` を `next_ws_message` に置き換え、テスト失敗を明示化
  - 理由: テストでのサイレントエラー防止

- [ ] `#[cfg(test)]` importをサブモジュール内テストに移動
  - ファイル: `src/server.rs` L21-42
  - 内容: test-onlyのre-importを各サブモジュールのテストに移動
  - 理由: 親モジュールの簡素化、テストと実装の近接配置
