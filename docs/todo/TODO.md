# TODO Issues

## 現在の未完了タスク

### Low Priority

- [ ] `augmentHashWithTrailingLineHint` 残りの分岐テスト追加
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 行番号: L205付近（既存 negative/positive テストと併設）
  - 内容: 以下 2 ケースが未カバー。`page.evaluate` で関数を直接呼び出す既存パターンで追加可能
    1. **新形式優先 (precedence)**: hash が `#section-b:L15` で末尾に余計な `L20` が並ぶ場合、`parseLineHash(hash).lineRange` 早期 return により hash が不変であることを確認
    2. **空 hash 合成経路**: `hash === ''` / `hash === '#'` で augment 時に `#L{start}` が生成される（コロン prefix なし）ことを確認
  - 理由: PR #75 レビューで指摘された criticality 3 のテストギャップ。範囲形式 (`L15-L17`) と逆転範囲 (`L17-L15`) は PR #75 本体で対応済み
  - 優先度: Low（本体機能は E2E full flow + 既存 negative/positive テストでカバー済み、unit-like 補強）
