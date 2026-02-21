# TODO Issues (レビュー日: 2026-02-22)

## Low Priority

- [x] テンプレートの単体テスト追加
  - ファイル: `src/template.rs`
  - 理由: `render_page` のXSSエスケープ、ダークモード切替等のテストが未整備
  - 対応: `tests/renderer_test.rs` にタイトルエスケープ、ダークモード、基本構造テストを追加

- [x] ファイルサイズ上限のテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: `MAX_FILE_SIZE` 超過時の動作テストが未整備
  - 対応: `test_ファイルサイズ上限超過でエラーメッセージが返る` を追加

- [ ] 未知言語コードブロックのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: syntectが認識しない言語指定時のフォールバック動作のテストが未整備

- [ ] テーブルalignment対応
  - ファイル: `src/renderer.rs`（`Event::Start(Tag::Table(alignments))`付近）
  - 理由: テーブルのセル揃え（left/center/right）が未実装

- [ ] 順序付きリストのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: `<ol>` のレンダリング動作テストが未整備

- [ ] WebSocket Origin検証のエッジケーステスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: Origin未送信、不正スキーム、ポート不一致等の境界テストが未整備

- [ ] セキュリティ関数のドキュメントコメント追加
  - ファイル: `src/renderer.rs`, `src/server.rs`
  - 対象: `sanitize_href`, `is_safe_href`, `is_allowed_ws_origin`, `normalize_authority`
  - 理由: セキュリティ上重要な関数にdocコメントがない

- [ ] 監視スレッドの名前付け
  - ファイル: `src/watcher.rs`
  - 理由: `std::thread::spawn` で無名スレッド。`thread::Builder::new().name(...)` を使用すべき

- [ ] `sanitize_href` のホワイトスペースパディングテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: `"  javascript:alert(1)  "` のようなパディング付きURLのテストが未整備

- [ ] `notify_update` でクライアント不在時のレンダリングスキップ
  - ファイル: `src/server.rs`
  - 修正方針: `state.tx.receiver_count() == 0` なら早期リターン
  - 理由: 接続クライアントがいない場合の無駄なレンダリングを回避

## 巨大な修正（要別途対応）

- [ ] [High] read_and_render のエラー型を Result に変更
  - ファイル: `src/server.rs`
  - 影響範囲: index_handler, api_content_handler, handle_socket, notify_update
  - 修正方針: `(String, String)` → `Result<(String, String), std::io::Error>` に変更。各呼び出し元でエラーを適切に処理（HTTPは500、WebSocketはログのみ等）
  - 理由: エラーが200 OKのHTMLコンテンツとして返され、ログも出力されない

- [ ] [High] ファイル監視ランタイムエラーの伝播
  - ファイル: `src/watcher.rs`
  - 影響範囲: debouncer callback, mpsc channel, tokio spawn task
  - 修正方針: mpscチャネルの型を `Result<(), String>` に変更し、エラー時にログ出力＋WebSocket通知を検討
  - 理由: 初期化後の監視エラーでライブリロードが静かに停止する

- [ ] [Medium] broadcast チャネルを `String` から `UpdateMessage` 型に変更
  - ファイル: `src/server.rs`, `src/template.rs`, `src/watcher.rs`
  - 影響範囲: AppState, handle_socket, notify_update, watcher bridge
  - 修正方針: `broadcast::Sender<String>` → `broadcast::Sender<Arc<UpdateMessage>>` に変更し、JSON化をWebSocket送信直前に移動
  - 理由: 型安全性向上。現状は任意文字列をbroadcastできてしまう

- [ ] [Medium] AppState のフィールドをprivate化してコンストラクタを追加
  - ファイル: `src/server.rs`, `src/main.rs`, `tests/integration_test.rs`
  - 影響範囲: AppState生成箇所すべて（main.rs, 統合テスト内のAppState直接生成箇所）
  - 修正方針: pub フィールド → `pub(crate)` + `AppState::new()` コンストラクタ
  - 理由: 不変条件の強制。バリデーションなしに生成可能な現状を改善

- [ ] [Medium] `is_target_file` のアトミックセーブ対応
  - ファイル: `src/watcher.rs`
  - 影響範囲: ファイル変更検知
  - 修正方針: `canonicalize` 失敗時にファイル名＋親ディレクトリで比較するフォールバック
  - 理由: Vim/Emacs等のアトミックセーブ時に一時的にファイルが存在せず、変更イベントがドロップされる

- [ ] [Low] watcher JoinHandle の監視
  - ファイル: `src/watcher.rs`
  - 影響範囲: watch_file関数
  - 修正方針: `tokio::spawn` の返り値を保持し、パニック時にログ出力する仕組みを追加
  - 理由: 現状JoinHandleがdropされ、tokioタスクのパニックが検知されない
