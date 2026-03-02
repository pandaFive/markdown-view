# TODO Issues

## 包括的PRレビュー Round 3 (レビュー日: 2026-02-27)

### 型設計

- [ ] `SanitizedHtml`コンストラクタ可視性の厳格化
  - ファイル: `src/renderer.rs`
  - 理由: `pub(crate)`がrenderer/toc以外からの呼び出しを型レベルで防止できない

## PRレビュー: notify_updateエラーファイル名修正 (レビュー日: 2026-03-01)

### Low Priority

- [ ] 単一ファイルモードのnotify_updateエラーのユニットテスト追加
  - ファイル: `src/server.rs` (テスト)
  - 理由: 統合テストは存在するが、軽量なユニットテストで高速フィードバックを得る

- [ ] 統合テストの`.unwrap()`チェーンを`.expect()`に置き換え
  - ファイル: `tests/integration_test.rs` L1159-1163, L1197-1201
  - 理由: テスト失敗時のエラーメッセージが不明瞭

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
