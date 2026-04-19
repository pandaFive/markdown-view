# TODO Issues

## 現在の未完了タスク

### Low Priority

- [ ] `augmentHashWithTrailingLineHint` の行範囲形式（`L15-L17`）ブランチ E2E テスト追加
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 行番号: L205付近（旧形式メモテスト隣）
  - 内容: 現在は `L15` 単独のみカバー。`(?:-L(\d+))?` 分岐と `end > start` の suffix 生成 (`L15-L17`) が無カバーのため、旧形式メモ fixture に `L15-L17` を含めるテストを 1 ケース追加するか、関数を export して Node/jsdom 単体テスト化する
  - 理由: 将来 `augmentHashWithTrailingLineHint` を編集した際の回帰検出力を上げる（PR #75 レビューの Low 指摘）
  - 優先度: Low（本体ロジックは旧形式メモテストで動作確認済み）
