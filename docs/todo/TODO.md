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

## PR #4 レビュー Suggestion (レビュー日: 2026-02-25)

### Low Priority

- [ ] `AppMode` にバリデーション付きコンストラクタを追加
  - ファイル: `src/server.rs`
  - 理由: `AppMode::SingleFile` に存在しないパスや非.mdを設定可能な現状を改善

- [ ] `ResolveFileError` に `std::error::Error` を実装
  - ファイル: `src/server.rs`
  - 理由: 標準のエラーインターフェースに準拠し、`anyhow` との互換性を向上

- [ ] `render_page` の6引数を構造体パラメータに変更
  - ファイル: `src/template.rs`, 呼び出し箇所全て
  - 理由: 引数の順序ミスリスク軽減。`RenderPageParams` 構造体を導入

- [ ] `list_markdown_files` のクエリ指定時スキップ
  - ファイル: `src/server.rs` (`resolve_target_file`)
  - 理由: `query_file` が Some の場合、ファイル一覧の走査は不要（パフォーマンス改善）

- [x] 空ディレクトリ時の404テスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: .mdファイルが1つもないディレクトリで404が返ることのテストが未整備
  - 対応: `test_ディレクトリモード_空ディレクトリで404を返す` を追加

- [ ] 単一ファイルモードの `/api/content` で `file` フィールド不在テスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: 単一ファイルモードのAPIレスポンスに `file` フィールドが含まれないことの検証

- [ ] `resolve_file` docコメントに「正規化済みパスを返す」を追記
  - ファイル: `src/server.rs`
  - 理由: 戻り値がcanonicalize済みであることがドキュメントに明記されていない

- [ ] `notify_update` docコメントにディレクトリモード動作を追記
  - ファイル: `src/server.rs`
  - 理由: ディレクトリモード時の `file` フィールド付与動作がドキュメントに未記載

## PR #4 レビュー Round 2 Suggestion (レビュー日: 2026-02-25)

### Low Priority

- [ ] Watcher canonicalize サイレントフォールバックにログ追加
  - ファイル: `src/watcher.rs:188-189`
  - 理由: `canonicalize().unwrap_or_else` がサイレント。ログを追加して監視対象外イベントの追跡性を向上

- [x] JS `selectFile` のfetch失敗時にユーザーへの視覚的フィードバック追加
  - ファイル: `src/template.rs` (JS部分)
  - 理由: `console.error` のみでユーザーには通知されない。バナー表示等を検討
  - 対応: `file-fetch-error-banner` を追加し、fetch失敗時に表示・成功時に自動非表示化

- [ ] シンボリックリンクディレクトリのファイル一覧テスト追加
  - ファイル: `tests/integration_test.rs` or `src/server.rs`テスト
  - 理由: `list_markdown_files` のsymlink containmentチェック動作が未テスト

- [ ] `UpdateMessage.file` フィールドのシリアライズテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: `skip_serializing_if` による条件付きシリアライズの動作検証

- [ ] 単一ファイルモードで `?file=` クエリパラメータ指定時の動作テスト
  - ファイル: `tests/integration_test.rs`
  - 理由: クエリが無視されることの明示的テストが未整備

- [ ] アクティブファイルマーカーのHTMLテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: ファイル一覧で現在選択中のファイルに `class="active"` が付与されることの検証

- [x] デフォルトファイルフォールバックテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: README.mdがない場合にアルファベット順最初のファイルが選ばれることの検証
  - 対応: `test_ディレクトリモード_readmeなし時はアルファベット順最初のファイルがデフォルト` を追加

- [ ] `relative_path_of` にfile_pathの事前条件docコメント追加
  - ファイル: `src/server.rs`
  - 理由: file_pathがcanonicalize済みであることの前提が明記されていない

- [ ] `is_hidden_relative` にfail-safe動作のdocコメント追加
  - ファイル: `src/watcher.rs`
  - 理由: 判定失敗時に安全側で `true` を返す動作の明示

- [ ] `list_markdown_files` にソート順のdocコメント追加
  - ファイル: `src/server.rs`
  - 理由: アルファベット順ソートであることがdocコメントに未記載

- [ ] innerHTML セキュリティコメントにエスケープ経路を追記
  - ファイル: `src/template.rs`
  - 理由: `updateContent()` のコメントに具体的なエスケープパス（renderer.rs→server.rs→template.rs）を記載

### 巨大な修正（要別途対応）

- [ ] [Medium] `AppMode` にバリデーション付きコンストラクタ追加
  - ファイル: `src/server.rs`, `src/main.rs`, `tests/integration_test.rs`
  - 影響範囲: 全AppState生成箇所
  - 修正方針: `AppMode::new_single_file(path)` / `AppMode::new_directory(path)` で存在確認・.md検証
  - 理由: 型設計アナライザで5.5/10の評価。不変条件の強制が不十分

- [ ] [Low] `ResolveFileError`/`ReadMarkdownError` に `std::error::Error` 実装
  - ファイル: `src/server.rs`
  - 修正方針: `impl std::error::Error for ResolveFileError {}` + `impl std::error::Error for ReadMarkdownError {}`
  - 理由: 標準エラーインターフェース準拠。anyhowとの互換性向上

## PR #4 レビュー Round 3 (レビュー日: 2026-02-25)

### Important（巨大な修正のためTODO）

- [x] [Important] JS `selectFile` のfetch失敗時にユーザーへの視覚的フィードバック追加
  - ファイル: `src/template.rs` (JS部分)
  - 修正方針: エラーバナー表示（disconnect bannerと同様のパターン）
  - 理由: `console.error` のみでユーザーには通知されない
  - 対応: エラーバナー表示/非表示処理を実装し、`selectFile` の成功・失敗フローに統合

- [ ] [Important] JS WebSocket `data.error` 受信時にユーザーへの視覚的フィードバック追加
  - ファイル: `src/template.rs` (JS部分)
  - 修正方針: エラーバナー/トースト表示
  - 理由: `console.error` のみでユーザーには通知されない

### Low Priority

- [ ] `list_markdown_files` のシンボリックリンクサイクル検出テスト追加
  - ファイル: `src/server.rs` テスト
  - 理由: サイクル検出ロジックの動作検証（テスト環境でのsymlink loop作成が必要）

- [ ] `notify_update` のディレクトリモード相対パス失敗時スキップテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: ベースディレクトリ外のファイル変更時にブロードキャストがスキップされることの検証

- [ ] `list_markdown_files_recursive` の通常ディレクトリcanonicalize失敗時テスト追加
  - ファイル: `src/server.rs` テスト
  - 理由: ハードリンクやマウントポイントでcanonicalizeが失敗するケースの検証

- [ ] `resolve_file` のdocコメントに「canonicalize済みの絶対パスを返す」を追記
  - ファイル: `src/server.rs`
  - 理由: 戻り値の保証がドキュメントに明記されていない

- [ ] `notify_update` のdocコメントにディレクトリモード相対パス失敗時のスキップ動作を追記
  - ファイル: `src/server.rs`
  - 理由: 新たに追加されたスキップ動作がdocコメントに反映されていない（※コード内コメントには記載済み）

## PR #4 レビュー Round 4 (レビュー日: 2026-02-25)

### 修正済み

- [x] [Critical] watcher `canonicalize` フォールバックがセキュリティ境界をバイパス
  - ファイル: `src/watcher.rs` (`watch_directory`, `watch_single_file`)
  - 対応: `unwrap_or_else(|_| event.path.clone())` → 失敗時はログ出力してイベントをスキップ
- [x] [Important] `list_markdown_files_recursive` に再帰深度制限がない
  - ファイル: `src/server.rs`
  - 対応: `MAX_DIR_DEPTH = 32` の定数を追加し、超過時はログ出力してスキップ

### Low Priority

- [ ] `is_target_file` canonicalize失敗フォールバックのセキュリティ検証
  - ファイル: `src/watcher.rs`
  - 理由: ファイル名+親ディレクトリ比較のフォールバックは異なるディレクトリの同名ファイルで誤検知リスクあり

## PR #4 レビュー Round 5 (レビュー日: 2026-02-25)

### 修正済み

- [x] [Critical] watcher `is_hidden_relative` がcanonicalize前に実行され隠しディレクトリのsymlink bypass可能
  - ファイル: `src/watcher.rs` (`watch_directory`)
  - 対応: `is_hidden_relative` をcanonicalize + starts_with チェックの後に移動
- [x] [Critical] lagged client のエラーJSON送信結果を `let _` で破棄、デッドコネクションで無限ループ
  - ファイル: `src/server.rs:439`
  - 対応: 送信失敗時はログ出力 + `break`
- [x] [Important] `notify_update` エラーJSON生成失敗が無言でreturn
  - ファイル: `src/server.rs:811`
  - 対応: ログ出力追加
- [x] [Important] `is_hidden_relative` のcanonicalize失敗が無言で吸収
  - ファイル: `src/watcher.rs:312-313`
  - 対応: matchに変更してログ出力追加
- [x] [Important] `list_markdown_files` ベースdir canonicalize失敗が無言
  - ファイル: `src/server.rs:514-516`
  - 対応: matchに変更してログ出力追加
- [x] [Important] 通常ディレクトリcanonicalize失敗が無言でサイクル検出スキップ
  - ファイル: `src/server.rs:595-599`
  - 対応: matchに変更してログ出力追加
- [x] [Important] `is_target_file` docコメントが不正確
  - ファイル: `src/watcher.rs:334`
  - 対応: 「ファイル名と親ディレクトリの両方で比較する」に修正
- [x] [Important] `notify_update` docコメントが「エラー表示される」と誤記
  - ファイル: `src/server.rs:775`
  - 対応: 「コンソールにエラーログが出力される（UI表示はなし）」に修正
- [x] [Important] JS try-catchが広すぎて非パースエラーも "parse error" とラベル
  - ファイル: `src/template.rs` (JS内 ws.onmessage)
  - 対応: try-catchをJSON.parseのみに限定、後続ロジックはcatch外に移動
- [x] [Important] ディレクトリ一覧で単一エントリのエラーが全体を中断
  - ファイル: `src/server.rs:538-539, 549`
  - 対応: `entry?` と `file_type()?` をmatchに変更、エラー時はスキップ+ログ

### Low Priority / Suggestions

- [ ] `notify_update` broadcastスキップのテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: ディレクトリモードで相対パス算出失敗時のスキップ動作が未テスト

- [ ] `MAX_DIR_DEPTH` 深度制限のテスト追加
  - ファイル: `src/server.rs` テスト
  - 理由: 33+階層のディレクトリ構造でのスキップ動作が未テスト

- [ ] `render_page` ディレクトリモード引数のユニットテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: `file_list`, `current_file` 引数のテストがNone,Noneのみ

- [ ] `list_markdown_files` docコメントに `MAX_DIR_DEPTH` 記載追加
  - ファイル: `src/server.rs`
  - 理由: docコメントに深度制限の記載がない

- [ ] CSP `img-src *` と `data:` スキームのコメント明確化
  - ファイル: `src/server.rs:109`
  - 理由: CSP Level 2+では `*` は `data:` にマッチしない。現状は安全（ブロック）だがコメントが曖昧

- [ ] `ResolveFileError` に `status_code()` メソッド追加
  - ファイル: `src/server.rs`
  - 理由: HTTPステータスマッピングが2箇所に散在し微妙に異なる

- [ ] `resolve_file` の403 vs 404の区別でパス列挙が可能（SEC-1）
  - ファイル: `src/server.rs:230-237`
  - 理由: Traversal/Hidden/NotMarkdown=403, NotFound=404 で隠しファイル存在が推測可能
  - 対応方針: 全エラーを404に統一することを検討（ローカルツールなのでリスクは低い）

- [ ] `base_for_filter` 変数名の明確化
  - ファイル: `src/watcher.rs:162`
  - 理由: canonicalize済みである不変条件が変数名に表現されていない

### 巨大な修正（要別途対応）

- [ ] [Medium] `AppMode` に `CanonicalPath` newtypeで不変条件を型で表現
  - ファイル: `src/server.rs`, `src/main.rs`, `src/watcher.rs`
  - 影響範囲: AppMode構築・パターンマッチ箇所すべて
  - 修正方針: `CanonicalPath(PathBuf)` newtypeを導入、`relative_path_of` の毎回canonicalizeを排除
  - 理由: type-design-analyzer 評価 4.0/10。繰り返しcanonicalize呼び出しの排除と型安全性向上

- [ ] [Medium] broadcast チャネルを `String` から型付きメッセージに変更
  - ファイル: `src/server.rs`, `src/template.rs`, `src/watcher.rs`
  - 影響範囲: AppState, handle_socket, notify_update
  - 修正方針: `BroadcastMessage` enumを導入（Update/Refresh/Error）、JSON化はWS送信直前に移動
  - 理由: type-design-analyzer指摘。String型では任意のJSON形状を送信可能で型安全性が不足
