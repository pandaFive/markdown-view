# TODO Issues (レビュー日: 2026-02-19)

## Low Priority

- [ ] `SyntaxSet::load_defaults_newlines()` を `LazyLock` でキャッシュ
  - ファイル: `src/renderer.rs:17`
  - 理由: 毎回ロードは非効率。初回ロードのみにすることでレンダリング性能が向上する
  - 備考: `std::sync::LazyLock`（Rust 1.80+）を使用

- [ ] テンプレートの単体テスト追加
  - ファイル: `src/template.rs`
  - 理由: `render_page` のXSSエスケープ、ダークモード切替等のテストが未整備

- [ ] 未知言語コードブロックのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: syntectが認識しない言語指定時のフォールバック動作のテストが未整備

- [ ] テーブルalignment対応
  - ファイル: `src/renderer.rs`（`Event::Start(Tag::Table(alignments))`付近）
  - 理由: テーブルのセル揃え（left/center/right）が未実装

- [ ] ファイルサイズ上限のテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: `MAX_FILE_SIZE` 超過時の動作テストが未整備

- [ ] 順序付きリストのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: `<ol>` のレンダリング動作テストが未整備

- [ ] WebSocket Origin検証のエッジケーステスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: Origin未送信、不正スキーム等の境界テストが未整備

- [ ] セキュリティ関数のドキュメントコメント追加
  - ファイル: `src/renderer.rs`, `src/server.rs`
  - 対象: `sanitize_href`, `is_safe_href`, `is_allowed_ws_origin`
  - 理由: セキュリティ上重要な関数にdocコメントがない

- [ ] 監視スレッドの名前付け
  - ファイル: `src/watcher.rs:39`
  - 理由: `std::thread::spawn` で無名スレッド。`thread::Builder::new().name(...)` を使用すべき

## 巨大な修正（要別途対応）

- [ ] [Medium] broadcast チャネルを `String` から `UpdateMessage` 型に変更
  - ファイル: `src/server.rs`, `src/template.rs`, `src/watcher.rs`
  - 影響範囲: AppState, handle_socket, notify_update, watcher bridge
  - 修正方針: `broadcast::Sender<String>` → `broadcast::Sender<UpdateMessage>` に変更し、JSON化をWebSocket送信直前に移動
  - 理由: 型安全性向上。現状は任意文字列をbroadcastできてしまう

- [ ] [Medium] AppState のフィールドをprivate化してコンストラクタを追加
  - ファイル: `src/server.rs`, `src/main.rs`, `tests/integration_test.rs`
  - 影響範囲: AppState生成箇所すべて（main.rs, 統合テスト内の3つのAppState直接生成箇所）
  - 修正方針: pub フィールド → private + `AppState::new()` コンストラクタ
  - 理由: 不変条件の強制。バリデーションなしに生成可能な現状を改善

- [ ] [Low] read_and_render のエラー型を分離
  - ファイル: `src/server.rs`
  - 影響範囲: index_handler, api_content_handler, handle_socket, notify_update
  - 修正方針: `(String, String)` → `Result<(String, String), RenderError>` に変更し、エラーとコンテンツを型で区別
  - 理由: 現状はエラーメッセージもcontent文字列として返しており、正常コンテンツとの区別ができない

- [ ] [Low] watcher JoinHandle の監視
  - ファイル: `src/watcher.rs:102`
  - 影響範囲: watch_file関数
  - 修正方針: `tokio::spawn` の返り値を保持し、パニック時にログ出力する仕組みを追加
  - 理由: 現状JoinHandleがdropされ、tokioタスクのパニックが検知されない
