# TODO Issues

## PRレビュー: セキュリティ強化とtemplate分割 (レビュー日: 2026-03-09)

### Low Priority

- [ ] `TargetResolveContext`の抽象度評価
  - ファイル: `src/server.rs` L356-376
  - 内容: 現状2メソッド×2バリアントで薄い抽象化。将来エンドポイント固有の処理が増えなければ`&'static str`パラメータに簡素化を検討
  - 理由: 過剰設計にならないよう定期的に評価
