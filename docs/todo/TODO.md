# TODO Issues

## PRレビュー: SanitizedHtmlコンストラクタ可視性厳格化 (レビュー日: 2026-03-02)

### Low Priority

- [ ] `SanitizedHtml` structのdocコメント表現をより正確にする
  - ファイル: `src/renderer/mod.rs` L15
  - 現状: 「`render_markdown` / `generate_toc` 経由でのみ生成する設計」
  - 提案: 「`render_markdown` / `generate_toc` が主たる生成経路」（テストコードや将来のサブモジュールからも構築可能なため）
  - 理由: 設計意図としては現状で十分理解可能。低リスク

## PRレビュー: テキスト選択中DOM更新延期 (レビュー日: 2026-03-02)

### Low Priority

- [ ] `data.refresh`パスがテキスト選択延期機構をバイパスする
  - ファイル: `src/template.rs`（クライアントJS）
  - 現状: `selectFile(currentFile, false)`経由でDOM更新が走り選択が破壊される
  - 理由: refresh発生頻度が低く、ユーザー影響は最小限。対応するにはrefreshパスにも延期チェックが必要

- [ ] テキスト選択延期機構のPlaywright E2Eテスト追加
  - ファイル: 新規テストファイル
  - 内容: ドラッグ選択→WS更新→選択維持、ファイル遷移時の保留クリア、30秒フォールバック等
  - 理由: クライアントJS状態マシンのエッジケースを自動テストでカバー
