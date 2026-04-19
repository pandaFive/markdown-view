# TODO Issues

## 現在の未完了タスク

### Low Priority

- [ ] `augmentHashWithTrailingLineHint` 残りの分岐テスト追加
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 行番号: L205付近（旧形式メモテストと併設）
  - 内容: 以下 3 ケースが未カバー。`page.evaluate` で関数を直接呼び出す形で追加するのが既存 negative テスト（L245/L269）と対称で最小コスト
    1. **範囲形式 `L15-L17`**: `end > start` 分岐の `L{start}-L{end}` suffix 生成を直接検証
    2. **新形式優先 (precedence)**: hash が `#section-b:L15` で末尾に余計な `L20` が並ぶ場合、`parseLineHash(hash).lineRange` 早期 return により hash が不変であることを確認
    3. **空 hash 合成経路**: `hash === ''` / `hash === '#'` で augment 時に `#L{start}` が生成される（コロン prefix なし）ことを確認
  - 理由: PR #75 レビューで criticality 3 のテストギャップとして指摘された分岐。いずれも直接関数呼び出しで低コスト検証できる。E2E full flow は不要
  - 優先度: Low（本体機能は E2E full flow + 既存 negative テストでカバー済み、unit-like 補強）
