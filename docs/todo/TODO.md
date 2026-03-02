# TODO Issues

## 包括的PRレビュー Round 3 (レビュー日: 2026-02-27)

### エラー処理

- [x] ~~WebSocket close frameで`user_message()`を使用~~ (完了: 2026-02-28)

- [x] ~~`notify_update`エラーブロードキャストにファイル名を含める~~ (完了: 2026-03-01)

### 型設計

- [ ] `SanitizedHtml`コンストラクタ可視性の厳格化
  - ファイル: `src/renderer.rs`
  - 理由: `pub(crate)`がrenderer/toc以外からの呼び出しを型レベルで防止できない

## PRレビュー: WebSocket close frame修正 (レビュー日: 2026-02-28)

### Low Priority

- [ ] WebSocket close codeをエラーバリアント別に精緻化
  - ファイル: `src/server.rs` L668-672
  - 現状: 全バリアントで`1011`（Internal Error）を使用
  - 推奨: `Io`→1011、`TooLarge`→1009（Message Too Big）、`NotUtf8`→1003（Unsupported Data）
  - 理由: RFC 6455準拠のセマンティクス改善。ローカルツールのため実影響は最小限

## PRレビュー: notify_updateエラーファイル名修正 (レビュー日: 2026-03-01)

### Medium（スコープ外）

- [ ] `lagged_recovery_message`のエラーメッセージにもファイル名を含める
  - ファイル: `src/server.rs` L642-654
  - 理由: `notify_update`にはファイル名が含まれるが`lagged_recovery_message`には含まれず、UXが不統一
  - 対応方針: `file_path.file_name()`でファイル名を取得して同様のフォーマットに統一

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
