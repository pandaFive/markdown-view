# TODO Issues

## 包括的PRレビュー Round 3 (レビュー日: 2026-02-27)

### テストカバレッジ

- [ ] `BroadcastMessage::Refresh`/`Error`のJSON直列化ユニットテスト追加
  - ファイル: `src/server.rs`
  - 理由: `{"refresh":true}`/`{"error":"..."}`のJSON契約がサーバー・クライアント間で暗黙的

- [ ] `resolve_file`のバックスラッシュ・エンコードパストラバーサルテスト追加
  - ファイル: `src/server.rs` (`resolve_file`)
  - 理由: `..\\`や`docs/../../../etc/passwd`等の攻撃バリエーションを明示的にカバー

- [ ] WebSocket lagged-clientリカバリの動作テスト追加
  - ファイル: `src/server.rs` (`handle_socket`)
  - 理由: broadcast容量超過時のファイル再読み込み・Refreshメッセージ送信パスが未テスト

- [ ] `csp_hash_sources`の複数テーマテスト追加
  - ファイル: `src/template.rs`
  - 理由: テーマごとにCSS内容が変わるためハッシュ計算の回帰防止

- [ ] `AppMode::new_single_file(dir)`/`new_directory(file)`のエラーパステスト追加
  - ファイル: `src/server.rs`
  - 理由: コンストラクタのバリデーションエラーパスが未テスト

- [ ] `render_markdown`の大量入力・深いネストの耐性テスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: 1MB入力やネスト深度500でのパニック・スタックオーバーフロー防止

- [ ] `MAX_FILE_LIST` 1000件截断テスト追加
  - ファイル: `src/server.rs` (`list_markdown_files`)
  - 理由: 截断が削除された場合のOOM・レスポンス遅延防止

- [ ] `notify_update`ディレクトリモードでファイル読み込み失敗時テスト追加
  - ファイル: `src/server.rs` (`notify_update`)
  - 理由: ファイル削除後の変更イベントでBroadcastMessage::Error送信を検証

- [ ] `UpdateMessage`の`file`フィールド直列化テスト追加
  - ファイル: `src/template.rs`
  - 理由: `file: Some("docs/guide.md")`時のJSON出力検証

- [ ] `/api/content?file=`（空文字列パラメータ）のテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: `file=`空文字列がresolve_fileに渡されるエッジケース

### エラー処理

- [ ] `render_markdown`のcatch-all `_ => {}`にdebugログ追加
  - ファイル: `src/renderer.rs` (L454, L588)
  - 理由: pulldown-cmark新イベント型が将来追加された際にサイレント除去を検出可能にする

- [ ] `syntax_theme_css`失敗時のユーザー通知改善
  - ファイル: `src/renderer.rs`
  - 理由: ハイライト無効化が発生してもユーザーには視覚的フィードバックがない

- [ ] CSPフォールバック時のセキュリティ低下通知
  - ファイル: `src/server.rs`
  - 理由: フォールバックCSPでscript-src/style-srcハッシュ制約が失われる点がユーザーに不可視

- [ ] `is_target_file`フォールバック比較の信頼性向上
  - ファイル: `src/watcher.rs`
  - 理由: canonicalize失敗時のファイル名+親ディレクトリ比較が非正規化パスで偽陽性/偽陰性の可能性

- [ ] `list_markdown_files`ベースディレクトリcanonicalize失敗時のエラー伝播検討
  - ファイル: `src/server.rs`
  - 理由: 空visited_dirsでサイクル検出が不完全になる（MAX_DIR_DEPTHで安全性は確保済み）

- [ ] WebSocket JSON直列化エラー時のクライアント通知
  - ファイル: `src/server.rs` (`handle_socket`)
  - 理由: 直列化失敗時にcontinueでメッセージ破棄し、クライアントはstaleコンテンツのまま

- [ ] watcherスレッドパニック時のクライアント通知
  - ファイル: `src/watcher.rs`
  - 理由: パニックでライブリロード停止してもユーザーに通知されない

- [ ] クライアント側JSONパースエラーの視覚的通知
  - ファイル: `src/template.rs` (JS)
  - 理由: console.errorのみでユーザーに不可視

### 型設計

- [ ] `UpdateMessage`のpubフィールド・構築後mutation排除
  - ファイル: `src/server.rs`, `src/template.rs`
  - 理由: `read_and_render_file`後に`update.file = ...`で後付け設定するパターンが脆弱

- [ ] `UpdateMessage::error()`メソッドの配置見直し
  - ファイル: `src/template.rs`
  - 理由: `UpdateMessage`の関連関数だが戻り値が`serde_json::Value`で型が不一致

- [ ] `SanitizedHtml`コンストラクタ可視性の厳格化
  - ファイル: `src/renderer.rs`
  - 理由: `pub(crate)`がrenderer/toc以外からの呼び出しを型レベルで防止できない

- [ ] `CanonicalPath`に`AsRef<Path>`実装
  - ファイル: `src/server.rs`
  - 理由: 標準ライブラリのパス受取APIとの互換性向上

### CI・インフラ

- [ ] CI workflowのpushトリガーにブランチフィルタ追加（develop/mainのみ）
  - ファイル: `.github/workflows/ci.yml`
  - 理由: 全ブランチへのpushでCIが実行されCI分数を浪費。`workflow`スコープのトークンが必要

### コメント・ドキュメント

- [ ] エラーメッセージの「10MB」ハードコードのdrift対策
  - ファイル: `src/server.rs`, `src/template.rs` (JS)
  - 理由: `MAX_FILE_SIZE`変更時に3箇所のハードコード文字列が更新漏れするリスク

- [ ] セキュリティ注記ブロックの配置修正
  - ファイル: `src/template.rs` (L554-561)
  - 理由: `const JS`直前に移動して関連性を明確化。現在は`FileTreeNode`の直前に浮遊

- [ ] `list_markdown_files`のソート順保証の文書化
  - ファイル: `src/server.rs`
  - 理由: 走査順→ソート→截断の動作で1000件超過時に正確なアルファベット順が保証されない点が未ドキュメント
