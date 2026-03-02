# TODO Issues

## PRレビュー: notify_updateエラーファイル名修正 (レビュー日: 2026-03-01)

### Low Priority

- [x] 統合テストの`.unwrap()`チェーンを`.expect()`に置き換え
  - ファイル: `tests/integration_test.rs` L1159-1163, L1186-1190, L1197-1201
  - 理由: テスト失敗時のエラーメッセージが不明瞭
  - 対応済み: 2026-03-02

- [ ] 統合テストの残りの`.unwrap()`チェーンも`.expect()`に統一
  - ファイル: `tests/integration_test.rs` L90-94, L111-113, L126-130, L245-247, L255-259 付近
  - 理由: 同じ `tokio::time::timeout(read.next()).await.unwrap().unwrap().unwrap()` パターンが残っており、一貫性向上のため

- [ ] WebSocketメッセージ受信後の`.unwrap()`も`.expect()`に改善
  - ファイル: `tests/integration_test.rs` L1165, L1203 付近
  - 対象: `msg.into_text().unwrap()` や `serde_json::from_str(&text).unwrap()`
  - 理由: テスト診断性の更なる向上（今回のスコープ外だが関連する改善）

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
