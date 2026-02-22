# TODO Issues (レビュー日: 2026-02-22)

## Low Priority

- [x] テンプレートの単体テスト追加
  - ファイル: `src/template.rs`
  - 理由: `render_page` のXSSエスケープ、ダークモード切替等のテストが未整備
  - 対応: `tests/renderer_test.rs` にタイトルエスケープ、ダークモード、基本構造テストを追加

- [x] ファイルサイズ上限のテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: `MAX_FILE_SIZE` 超過時の動作テストが未整備
  - 対応: `test_ファイルサイズ上限超過で413を返す` に更新

- [ ] 未知言語コードブロックのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: syntectが認識しない言語指定時のフォールバック動作のテストが未整備

- [ ] テーブルalignment対応
  - ファイル: `src/renderer.rs`（`Event::Start(Tag::Table(alignments))`付近）
  - 理由: テーブルのセル揃え（left/center/right）が未実装

- [ ] 順序付きリストのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: `<ol>` のレンダリング動作テストが未整備

- [x] WebSocket Origin検証のエッジケーステスト追加
  - ファイル: `src/server.rs`
  - 理由: Origin未送信、不正スキーム、ポート不一致等の境界テストが未整備
  - 対応: `server::tests` にユニットテスト12件追加

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

- [ ] `eprintln!` を構造化ロギング（`log` or `tracing`）に置換
  - ファイル: 全ソースファイル
  - 理由: ログレベルの区別やフィルタリングができない

- [ ] `html_escape` を単一パス実装に最適化
  - ファイル: `src/renderer.rs:507-513`
  - 理由: 現在5回の`.replace()`チェーンで中間Stringが5回割り当てられる

- [ ] ダークテーマCSS変数の重複排除
  - ファイル: `src/template.rs:72-104`
  - 修正方針: Rustの`const`でCSS変数を定義し2箇所に`format!`で埋め込む
  - 理由: `[data-theme="dark"]`と`@media (prefers-color-scheme: dark)`で同じ変数が重複

- [ ] `slugify` の日本語・CJK文字テスト追加
  - ファイル: `tests/renderer_test.rs` or `tests/toc_test.rs`
  - 理由: 日本語見出しのスラッグ生成が未テスト（ターゲットユーザーは日本語利用）

- [ ] non-UTF8ファイル読み込み時の500レスポンステスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: バイナリファイルを`.md`として開いた場合の動作が未テスト

- [ ] フルパイプラインXSSテスト（render_markdown→render_page）
  - ファイル: `tests/renderer_test.rs`
  - 理由: 個別関数のXSSテストはあるがパイプライン全体のEnd-to-Endテストがない

## 巨大な修正（要別途対応）

- [x] [High] read_and_render のエラー型を Result に変更
  - ファイル: `src/server.rs`
  - 対応: `ReadMarkdownError` を戻り値型に使用。TooLarge→413、Io→500 を返すように修正

- [ ] [High] ファイル監視ランタイムエラーの伝播
  - ファイル: `src/watcher.rs`
  - 影響範囲: debouncer callback, mpsc channel, tokio spawn task
  - 修正方針: mpscチャネルの型を `Result<(), String>` に変更し、エラー時にログ出力＋WebSocket通知を検討
  - 理由: 初期化後の監視エラーでライブリロードが静かに停止する

- [ ] [Medium] renderer/tocの見出し抽出ロジック統合（DRY違反）
  - ファイル: `src/renderer.rs`, `src/toc.rs`
  - 影響範囲: render_markdown, generate_toc, extract_headings
  - 修正方針: 共通の見出し抽出関数を作成し、rendererとtocで共有。パーサーオプションも統一
  - 理由: 同じMarkdownを2回パースし、見出しID生成ロジックが2箇所に重複している

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

- [ ] [Medium] `render_markdown` のGod Function分割
  - ファイル: `src/renderer.rs`
  - 影響範囲: render_markdown（390行、14個の可変状態変数）
  - 修正方針: RenderState構造体の導入、コードブロック/画像レンダリングの関数抽出、push_htmlヘルパーの追加
  - 理由: 可読性・保守性向上。新しいMarkdown要素追加時の状態管理バグリスクを低減

- [x] [Medium] `is_target_file` のアトミックセーブ対応
  - ファイル: `src/watcher.rs`
  - 対応: canonicalize失敗時にファイル名＋親ディレクトリ比較のフォールバックを実装済み

- [ ] [Medium] CSP の `'unsafe-inline'` をnonce/hashに置換
  - ファイル: `src/server.rs`, `src/template.rs`
  - 修正方針: リクエストごとにnonce生成し、CSPヘッダーとscript/styleタグに埋め込む
  - 理由: unsafe-inlineはXSS防御を弱める。ローカルツールとしてはリスク低だが防御深化として有効

- [x] [Low] watcher JoinHandle の監視
  - ファイル: `src/watcher.rs`
  - 対応: JoinHandleを保持し、パニック検知・ログ出力する監視タスクを追加済み

- [ ] [Low] read_and_render → UpdateMessage 直接返却
  - ファイル: `src/server.rs`
  - 修正方針: `Result<(String, String), _>` → `Result<UpdateMessage, _>` に変更
  - 理由: タプルの位置引数による取り違えリスクを排除

- [ ] [High] 監視スレッドのグレースフルシャットダウン機構
  - ファイル: `src/watcher.rs`
  - 影響範囲: std::thread::park()ループ、debouncer lifetime
  - 修正方針: AtomicBool + unpark、またはmpsc channelでシャットダウンシグナルを送信
  - 理由: 現在スレッドは永久にparkし、プロセス終了まで解放されない

- [ ] [Medium] notify callbackエラーのチャネル伝播
  - ファイル: `src/watcher.rs`
  - 影響範囲: mpsc channel型、tokio受信タスク
  - 修正方針: `mpsc::channel(32)` の型を `Result<(), String>` に変更し、エラー時にクライアントへWebSocket通知
  - 理由: 初期化後の監視エラーがeprintlnのみで報告され、クライアントに伝播しない

- [ ] [Medium] `is_target_file` のユニットテスト追加
  - ファイル: `src/watcher.rs`
  - 理由: 正規化成功ケース、失敗フォールバック（ファイル名+親ディレクトリ比較）、異なるディレクトリの同名ファイル等のテストが未整備

- [ ] [Medium] WebSocket Origin ポート不一致時のテスト追加
  - ファイル: `src/server.rs` テスト
  - 理由: `is_allowed_ws_origin` でOriginのポートがHostと一致しない場合のテストが不足

- [ ] [Medium] `read_markdown_with_limit` の境界値テスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: ちょうど10MB、10MB+1バイト等の境界値テストが未整備

- [ ] [Medium] UpdateMessage にファクトリメソッド追加
  - ファイル: `src/template.rs`
  - 修正方針: `UpdateMessage::new(content, toc)` + `UpdateMessage::error(msg)` を追加
  - 理由: エラーJSONの生成が `serde_json::json!` のアドホック構築で一貫性がない

- [ ] [Medium] ReadMarkdownError に IntoResponse 実装
  - ファイル: `src/server.rs`
  - 修正方針: `impl IntoResponse for ReadMarkdownError` で TooLarge→413, Io→500 をカプセル化
  - 理由: ハンドラーでのmatch分岐を減らし、ステータスコードマッピングを一箇所に集約
