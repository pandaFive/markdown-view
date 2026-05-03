# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium が TODO.md から捌けてから着手する候補。
未完了項目はリスク低減効果を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）の発見コンテキスト。

## P1: リスク低減・検証基盤

- [ ] Host middleware 化後の低優先 follow-up を整理して追加検証する
  - ファイル: `src/server/routes.rs`, `src/server/guards.rs`, `tests/integration_test.rs`, `docs/superpowers/specs/2026-05-02-host-middleware-guard-design.md`
  - 現状: PR #120 で Host 検証を router middleware へ集約し、主要 route の不正 Host 拒否、security headers、WS Host/Origin 経路の分離、大容量 PUT body の順序を固定した。一方、許可 Host の全 route smoke、malformed/missing/empty Host の middleware 統合テスト、WS Origin 拒否の error message assert、middleware warn ログへの URI path 追加、test helper 内 `axum::serve(...).unwrap()` の panic 観測性、CHANGELOG 相当の運用ドキュメント化は未対応
  - 対応: 追加する価値が高い順に、許可 Host 明示ループ、malformed/missing/empty Host の middleware 経路 403、WS Origin 拒否 message assert、warn ログへの `request.uri().path()` 追加を検討する。`axum::serve(...).unwrap()` は test helper の失敗文脈が分かる `expect(...)` へ寄せる。WS Host 拒否 message 変更は PR 本文には明記済みなので、必要になった時点で README か CHANGELOG 相当へ移す
  - 由来: PR #120 再レビュー follow-up (2026-05-02)

- [ ] Windows メモ原子保存のエラー処理と retry 条件を細分化する
  - ファイル: `src/server/files/memo_fs.rs`
  - 現状: Windows の `MoveFileExW` 呼び出しは `spawn_blocking` 経由だが、`JoinError` は `ErrorKind::Other` に潰している。また tmp 作成 retry は `AlreadyExists` のみを対象にしており、Windows の共有違反・削除保留・ウイルス対策ソフトによる一時ロックを retry しない
  - 対応: `JoinError::is_panic()` / `is_cancelled()` を分けて `tracing::error!` に残す。Windows では `raw_os_error()` で sharing violation / delete pending 相当を判定し、短い retry 対象に含める。Windows CI または `cargo check --target x86_64-pc-windows-gnu` が通る環境で検証する
  - 由来: メモ原子保存 PR 3rd レビュー (2026-04-30)

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索のキャンセル境界と allocation 削減を検討する
  - ファイル: `src/server/files/search.rs`, `src/template/assets/js/content.js`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数の打ち切りも明示されている。一方、連続検索時に古い検索処理をキャンセルする仕組みはなく、`SearchResultItem` の `before/current/after` はマッチごとに `String` を確保する
  - 対応: クライアント検索世代とサーバ側処理の対応、古い検索結果の破棄、`Cow<str>` 化や検索ブロック処理の allocation 削減を、計測結果に基づいて検討する
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)

- [ ] CLAUDE.md のアーキテクチャ記述を現在の実装構成に揃える
  - ファイル: `CLAUDE.md`
  - 現状: CLAUDE.md は `server/files.rs` / `watcher.rs` / `template/mod.rs` を単一ファイル前提で記載しているが、実装は `src/server/files/{catalog,content,memo,memo_fs,memo_sidecar,resolve,search,test_support,tests}.rs`、`src/watcher/{runtime,strategy,error}.rs`、`src/server/{log_path,watch}.rs`、`src/renderer/{state,security,line,highlight,render}.rs`、`src/template/assets/{css,js}/` まで細分化済み。さらに「見出しパースが 2 回」と書かれているが `search` 経由で 3 回目が走る
  - 対応: モジュール構成図と 2 回パースの記述を実装に追従。「CSS/JS 完全埋め込み」の文言は維持しつつ内部構造（`include_str!` 経由のサブモジュール化）を補足
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] プロダクト定義を Markdown previewer から Markdown workspace へ明文化する
  - ファイル: `README.md`, `CLAUDE.md`, `Cargo.toml`, `docs/todo/TODO.md`
  - 現状: `markdown-view` はメモ、引用、横断検索、ファイルツリー、読書補助 UI を含む Markdown 専用 workspace として育っているが、説明文には「軽量・高速 Markdown プレビューア」など previewer 寄りの表現が残る。そのため外部レビューでメモ機能が scope creep と誤読されやすい
  - 対応: README/Cargo description/開発ガイドの文言を「Markdown workspace」前提へ揃え、メモ・引用・検索を中核機能として位置付ける。純プレビュー化や `--no-memo` は現時点の非目標として明記する
  - 由来: Unix 哲学レビュー再検討 (2026-04-30)

- [ ] 未知言語コードブロックの silent fallback に警告ログを追加
  - ファイル: `src/renderer/highlight.rs` L14-50, `tests/renderer_test.rs` L346
  - 現状: `find_syntax_by_token().or_else(find_syntax_by_extension())?` が None を返すと `plain_code_block_html` で `class="language-{lang}"` だけ付与する fallback が走るが、ユーザーに「ハイライトが効いていない」ことを知らせる経路がない。`tests/renderer_test.rs:346` `test_未知言語コードブロックはフォールバック描画される` で仕様固定済み
  - 対応: 初回フォールバック時に `tracing::debug!` 程度のログを 1 回だけ出す（同じ言語名の繰り返しは抑制）。CLI 起動時に「対応シンタックス一覧」コマンドで利用可能言語を確認できるドキュメント追加も検討
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] CSP/syntax_theme_css フォールバック CSS の副作用設計判断を doc 化
  - ファイル: `src/renderer/mod.rs` L91-108, `src/template/assets.rs` L42-61
  - 現状: `syntax_theme_css` 失敗時に `highlight_disabled_notice_css()`（`body::before` グローバル CSS）を返し、`combined_css` に連結される。CSP ハッシュは fallback ベースで再計算されるため整合性は保たれるが、Markdown 側で `body::before` を期待する CSS が無いという暗黙前提がドキュメントに無い
  - 対応: `body::before` 衝突を許容しない旨を doc コメントに明記。または fallback CSS のセレクタを `.markdown-view-fallback-notice` 等の局所スコープに変更する
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] assets バンドルの sentinel 衝突回避テストを追加
  - ファイル: `src/template/assets/css_bundle.rs` L19, `src/template/assets/inline_script.rs` L17-22
  - 現状: `TEMPLATE.replace("__DARK_THEME_VARS__", ...)` / `replace("__MAX_FILE_SIZE_MB__", ...)` のプレースホルダーは sentinel 衝突に脆弱。`include_str!` した CSS/JS 内に同文字列が無いことを保証するテストが無い
  - 対応: `#[cfg(test)] mod tests` で「include 対象ソースに sentinel 文字列が含まれない」アサートを追加。`MAX_FILE_SIZE / 1024 / 1024` の整数除算で 11MB → 10MB 表示の丸め事故が起きないかも境界テスト
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] watcher の `try_send` で `WatchEvent::Error` を `FileChanged` と同列に破棄しない
  - ファイル: `src/watcher/runtime.rs` L19/L72/L111-127
  - 現状: `mpsc::channel(WATCHER_MESSAGE_BUFFER=32)` が満杯時、`FileChanged` も `Error` も同じ `try_send` 経路で破棄される。`WatchError::Init` / `ThreadPanic` を破棄するとフォアグラウンドが「監視が止まった理由」を失う
  - 対応: イベント種別で優先度を分け、`Error` 系は `blocking_send` に切り替えるか、別チャネルに分離する。または `try_send` 失敗時に `tracing::error!` で SLA を上げる
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `SanitizedHtml` から `innerHTML` までの信頼境界を設計メモ化する
  - ファイル: `src/renderer/mod.rs`, `src/template/assets/js/content.js`, `src/template/assets/js/memo.js`, `README.md`
  - 現状: Rust 側は `SanitizedHtml` newtype、raw HTML 破棄、URL policy、CSP hash で XSS 境界を作っている。一方ブラウザ側は `contentEl.innerHTML = safeData.content` / `memoPreviewEl.innerHTML = data.html` を使うため、境界の正しさは「サーバー生成 HTML だけが入る」という暗黙契約に依存している
  - 対応: renderer の信頼境界、HTTP/WS JSON の `content`/`toc`/`html` フィールド、JS 側の `innerHTML` 使用許可条件を短い設計メモにまとめる。E2E hook やテスト用 expose が production 経路で任意 HTML を流し込まないことも確認項目に含める
  - 由来: Unix 哲学レビュー (2026-04-30)

## P3: 長期改善・低緊急

- [ ] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - ファイル: `src/server/state.rs` L18-24/L112/L132
  - 現状: `CanonicalPath::try_from_path` で `canonicalize` した直後に `is_file()`/`is_dir()` で判定するが、両者の間に rename/unlink される race window がある。実害は起動時の `AppMode::new_*` のみで影響は小さい
  - 対応: `metadata` を一度取得してから `is_file`/`is_dir` を判定し、race window を縮める。`AppModeBuildError` のメッセージも metadata 起点に整理
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `log_path::canonicalize_status` の毎回 syscall を削減する
  - ファイル: `src/server/log_path.rs` L52-65
  - 現状: ログ出力ごとに `path` と `base` を canonicalize する。warn/error 時のみ呼ばれるが、ログ storm 状況下では I/O が増える
  - 対応: `base` の canonicalize 結果を起動時に一度だけ算出してキャッシュし、ログ経路では path 側のみ canonicalize する。または `OnceLock` で base を保持
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `/api/search` のクエリ長ガードを routes.rs 側に追加する
  - ファイル: `src/server/routes.rs` L298-318, `src/server/files/search.rs`
  - 現状: クエリ `q` を長さチェックせずに `search_directory` に渡す。極端に長い `q`（例: 1MB）が tracing にそのまま流れると無視できないコストになる
  - 対応: 1KB 程度の長さガードを `routes.rs` 側に追加し、超過時は 400 を返す。`search.rs` 内部にも防御を残す（depth in defense）
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `read_route_memo` の二重サイズチェックを単一化する
  - ファイル: `src/server/files/memo.rs` L455-481, `src/server/files/content.rs` L298-311
  - 現状: `fs.read_with_limit` が `MAX_FILE_SIZE+1` で `take` し超過時に `MemoReadError::TooLarge` を返すのに、`memo.rs:476-481` が読み込み完了後に `bytes.len() as u64 > MAX_FILE_SIZE` を再度チェックしている
  - 対応: `read_with_limit` の契約を doc コメントで明示し、呼び出し側の重複チェックを削除
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `BroadcastMessage::Update` 系のシリアライズ失敗時の fallback JSON を整備する
  - ファイル: `src/server/messages.rs` L46-56, `src/server/session.rs` L80-93
  - 現状: `serde_json::to_string(update)` の失敗は実質不可能だが、`session.rs` 側でエラー処理を持つ。Update メッセージ用の最小サイズ fallback (`{"content":"","toc":""}` 等) を返す `to_json_or_empty` 経路が無い
  - 対応: `BroadcastMessage::Update` の `to_json` に明示 fallback を追加。観測性として `tracing::error!` を残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `data-memo-file` 属性を None 時にスキップする
  - ファイル: `src/template/page.rs` L43/L47, `src/template/message.rs` L9-11
  - 現状: `params.memo.file().unwrap_or_default()` で常に `data-memo-file=""`（空文字）を出力する。`UpdateMessage` の `#[serde(skip_serializing_if = "Option::is_none")]` と非対称
  - 対応: `data-memo-file` も None 時に属性ごとスキップする経路に変更し、bootstrap.js 側を「属性無し ⇒ memo 無し」と扱うよう揃える
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `render_markdown` と `extract_headings` の早期 return 非対称を解消する
  - ファイル: `src/renderer/mod.rs` L57-63/L150
  - 現状: `render_markdown` は `input.is_empty()` で空 `SanitizedHtml` を返すが、`extract_headings` には対応する早期 return がない（`generate_toc` 側で空文字に落とすので結果は同じ）
  - 対応: `extract_headings` 側にも同様の早期 return を入れて API ペアの一貫性を揃える
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `CanonicalPathError` などの内部利用型を `pub(crate)` に絞る
  - ファイル: `src/server.rs` L13-19
  - 現状: `CanonicalPath` のみ `pub(crate)` で他は `pub` だが、`CanonicalPathError` も外部から触る経路がない
  - 対応: 公開不要な型を `pub(crate)` に絞り、lib API surface を最小化
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] サイドバーの "Documents" 文字列を i18n または日本語化
  - ファイル: `src/server/routes.rs` L33-41 (`sidebar_directory_name`)
  - 現状: `unwrap_or("Documents")` で英語固定。日本語 UI でも同名が出る
  - 対応: 日本語デフォルト（"ドキュメント"）にするか、ディレクトリ名取得失敗時のフォールバック挙動をコメントで明示
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `tokio::select!` の cancel-safe 性をコメントで明記する
  - ファイル: `src/server/session.rs` L54-132
  - 現状: `socket.recv()` と `rx.recv()` を `tokio::select!` で競わせているが、両者が cancel safe である根拠コメントが無い。将来の改修で cancel-unsafe な future を入れる事故リスク
  - 対応: 各 branch の future が cancel safe であることを doc コメントで明記し、新規 branch 追加時のチェックリストを残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `RouteTargetKind::include_file_list` を match 完全列挙に変更する
  - ファイル: `src/server/files/resolve.rs` L104-106
  - 現状: `matches!(self.kind, RouteTargetKind::Page)` で Page のみ true。新 variant 追加時に file_list を含めるかが暗黙判断になる
  - 対応: `match self.kind { Page => true, ApiContent | ApiMemo => false }` に変更し、新 variant 追加時に必ずコンパイルエラーで気付くようにする
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `panic::catch_unwind` の init 経路で `init_tx` 残存時に `ThreadPanic` を init 結果として送出
  - ファイル: `src/watcher/runtime.rs` L149-215/L243-247
  - 現状: debouncer 構築前に panic が起きた場合 `init_tx` が Some のまま `catch_unwind` を抜け、`await_watcher_init` が `Err(_)` 経路に落ちて「予期せず終了しました」とだけ表示される。`panic_detail` は受信前に終了するため使われない
  - 対応: panic 経路で `init_tx` がまだ Some なら `WatchError::thread_panic(...)` を init 結果として送る
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] インラインブラウザJS の TS 化
  - ファイル: `src/template/assets/js/{bootstrap,content,fetch,memo,selection,sidebar,websocket}.js`
  - 内容: Rust の `include_str!` でコンパイル時に埋め込まれる JS を TS で記述し、事前 tsc でビルドして `.js` 出力を `include_str!` 対象にする
  - 理由: ブラウザ側 JS は現在無型。ただし Rust ビルドパイプラインへの Node 依存追加が必要で、「Rust 単体ビルド」の明快さが崩れる
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
  - ファイル: `src/server/files/catalog.rs`
  - 現状: 相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使っている。上限 1000 件だが呼出あたり Vec アロケーションが発生する
  - 対応: 計測または必要性確認のうえ、イテレータ駆動で直接 String を構築する（`itertools::Itertools::join()` もしくは手書き fold）
  - 理由: マイクロ最適化。実装前に効果確認推奨
  - 由来: PR #59 探索 (2026-04-18)

- [ ] README のアーキテクチャ図を実装構成に揃える
  - ファイル: `README.md`
  - 現状: `websocket.rs` / `renderer.rs` / `template.rs` / `files.rs` が単一ファイル前提で記載されている。実装は `src/server/session.rs`、`src/renderer/`、`src/template/`、`src/server/files/` に分割済み
  - 対応: 現在の実装構成と同じ粒度で README のアーキテクチャ図を更新する
  - 理由: ドキュメント rot。新規コントリビュータが実装構造を誤解する
  - 由来: PR #59 探索 (2026-04-18)

## Done

- [x] `notify_update` receiver=0 エラー観測性改善
  - ファイル: `src/server/broadcast.rs`, `src/server/files/content.rs`
  - 内容: WebSocket 受信者が 0 の場合でも、ファイル変更イベント由来の検証・読込前エラーを warn ログへ残す経路を追加した。正常更新では従来通り本文読込と描画を避ける
  - 完了根拠: `6de7137 fix: notify_updateの受信者なしエラーをログ化 (#114)`、現行の `server::broadcast::tests::*受信者ゼロ*` 系テスト
  - 由来: TODO.md High Priority

- [x] `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling の直接回帰テスト
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: `document.createElement('span')` で `L15` を内包したノードを `link.nextSibling` に置き、`augmentHashWithTrailingLineHint(link, '#section-b')` が `#section-b:L15` を返すことを直接検証した
  - 完了根拠: `7185d24 test: ELEMENT_NODE sibling の hash 補完を固定 (#102)`、現行 `tests/e2e/memo_jump.spec.ts` の `augmentHashWithTrailingLineHint は ELEMENT_NODE sibling の textContent から行番号を補完する`
  - 由来: PR #76 レビュー (2026-04-20)

- [x] TOC pending navigation 小揺らしテストの grace 内外分離
  - ファイル: `tests/e2e/text_selection_defer.spec.ts`
  - 内容: slack 内では pending active を維持し、slack 外では通常判定へ戻る境界を分離して検証した
  - 完了根拠: `ff0fbe8 test: TOC pending navigation境界を固定 (#103)`、現行 `tests/e2e/text_selection_defer.spec.ts` の `目次クリック後のslack内スクロールではpending activeを維持し、slack外では通常判定へ戻る`
  - 由来: TOC pending navigation PR 再レビュー (2026-04-28)

- [x] `memo_jump.spec.ts` の Codex review ID コメント削除
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: 外部 review system の ID 参照を残さず、回帰保護の対象である false-positive パターンの説明へ置き換え済み
  - 完了根拠: 現行 `tests/e2e/memo_jump.spec.ts` に `Codex review #4136142343` が存在せず、false-positive 系の仕様説明コメントがテスト内に残っている
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20、既存 PR #76 持ち込み課題)

- [x] E2E 共通ヘルパーの silent false-positive 経路を狭める
  - ファイル: `tests/e2e/helpers.ts`, `tests/e2e/helpers.spec.ts`, `tests/e2e/globals.d.ts`, `tests/e2e/memo_jump.spec.ts`
  - 内容: `selectParagraphText` を既定で完全一致かつ一意一致にし、部分一致を明示オプションへ移した。fixture cleanup は memo artifact と `.markdown-view` の削除後残存を検知し、WebSocket dispatch helper は stale bridge を fail-fast にした
  - 完了根拠: `npm run typecheck`、`npx playwright test tests/e2e/helpers.spec.ts tests/e2e/memo_quote.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/document_search.spec.ts tests/e2e/memo_sync.spec.ts tests/e2e/markdown_links.spec.ts`、`./verify.sh` が pass
  - 由来: E2E 共通ヘルパー抽出 PR レビュー (2026-04-29、pre-existing)

- [x] `render_markdown` 責務分割後の silent failure 観測性強化
  - ファイル: `src/renderer/{render,state,highlight}.rs`, `tests/renderer_test.rs`
  - 内容: `heading_line_attrs` / `code_block_line_attrs` / `finish_heading` の active state 前提を debug/test で検知する契約として固定し、未処理 Markdown event/tag の debug ログ経路と code block fallback HTML の escaped fallback を module test で保護した
  - 完了根拠: `cargo test --all-targets --all-features` と `./verify.sh` が pass
  - 由来: render_markdown 責務分割 PR レビュー (2026-04-29)

- [x] `render_markdown` の責務分割
  - ファイル: `src/renderer/{mod,render,state,line,security,highlight}.rs`, `tests/renderer_test.rs`
  - 確認対象: `src/renderer/toc.rs`
  - 内容: `render_markdown` の公開契約を維持したまま、イベントディスパッチ、状態管理、行番号属性、URL sanitize、コードハイライトを renderer 内部モジュールへ分割した。Post-review で未使用の内部 `RenderOptions` は削除し、行追跡とハイライトの既定経路へ一本化した
  - 完了根拠: 2026-04-29 実装時点で `render_markdown` 境界テスト追加、`cargo test --all-targets --all-features`、`./verify.sh` が pass と報告済み
  - 由来: PR #59 探索 (2026-04-18)

- [x] E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出
  - ファイル: `tests/e2e/{memo_quote,memo_sync,memo_jump,markdown_links,text_selection_defer,document_search}.spec.ts`
  - 内容: `resetFixtures`、`selectParagraphText`、`stabilizeWebSocketHarness`、`requireUpdateContent`、`currentScrollY`、`waitForTocTrackingFrame`、`startTocActiveChangeRecorder`、`stopTocActiveChangeRecorder` 系など、複数 spec に残る近いヘルパーを共通モジュールへ抽出した
  - 理由: DRY 違反、片方を修正して片方を忘れるリスク。`tests/e2e/browser/test-websocket.ts` と `tests/e2e/globals.d.ts` は既に整備済みだが、spec-local helper の重複が残っていた
  - 完了根拠: `0b37aaa test: E2E共通ヘルパーを追加`, `791e5eb test: E2E fixture helperを共通化`, `0f5851d test: TOCとWebSocket E2E helperを共通化`, `098bc0c test: WebSocket連続dispatch helperを追加`, `268ae3a test: document searchのupdateContent helperを共通化`, `e898260 test: memo jumpのE2E helperを共通化`
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [x] TOC pending navigation の境界回帰テスト強化
  - ファイル: `tests/e2e/text_selection_defer.spec.ts`
  - 内容:
    - grace 400ms 以内の連続 TOC クリックで最後のクリック先に収束することを検証
    - `TOC_NAVIGATION_SLACK_PX` の内側 / 外側で pending 維持と通常判定復帰が分かれることを検証
    - `MutationObserver` で小揺らし中にクリック先以外へ active が切り替わらないことを検証
  - 完了根拠: `c9f8b81 test: TOC active監視ヘルパーを追加`, `16dc04f test: TOC pending navigation境界を固定`
  - 由来: PR #77 レビュー (2026-04-20)

- [x] E2E テストの DOM クリーンアップ戦略見直し
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: `augmentHashWithTrailingLineHint` 系テストの DOM cleanup を、末尾ノード推測ではなく、追加した `lineHint` ノードを直接 `remove()` する方式へ変更
  - 完了根拠: `1ee8916 test: E2E DOM cleanupを明示ノード削除に変更 (#99)`
  - 由来: PR #76 レビュー (2026-04-20)

- [x] E2E を `verify.sh` に opt-in 統合
  - ファイル: `verify.sh`
  - 内容: 通常の `./verify.sh` は軽量 checks を維持し、必要時に E2E を含めて実行できる opt-in 経路を追加
  - 完了根拠: `4241390 chore: verifyにE2E opt-inを追加 (#100)`
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [x] E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` に散在する `declare global { interface Window { ... } }` + ブラウザバンドル関数/変数の declare
  - 内容: 共通 ambient 宣言を `tests/e2e/globals.d.ts` に一本化。各 spec の `declare global` を削除。`tsconfig.json` の `include` で拾う
  - 理由: `Window.__lastWs` / `__realWsOnmessage` が text_selection_defer と document_search で byte 一致しているが、片方を変更すると TS2717 で破綻するリスクを根治。`selectFile` / `updateContent` 宣言の spec 間不整合も解消
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [x] `updateContent` 型宣言の統一
  - ファイル: `tests/e2e/memo_jump.spec.ts` (Window.updateContent プロパティ型), `tests/e2e/document_search.spec.ts` (top-level function 型)
  - 内容: 同じランタイム binding に対し 2 通りの型宣言が存在。`opts` が memo_jump では required、document_search では optional と不整合。どちらかに統一
  - 理由: 同一 binding を 2 型で捕捉しているため、片方の型が誤っても検出不能。ペイロード union (`{ refresh: true }` / `file?: string` 等) も未表現
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [x] `tsconfig.json` に strict flag 追加
  - 内容: strict の上位に両 flag を有効化
  - 理由:
    - `noUncheckedIndexedAccess`: `__clickObservations[href]` 等の Record アクセスに `undefined` 可能性を強制 → missing key のバグを発見
    - `exactOptionalPropertyTypes`: `toc?: string` と `toc: undefined` の区別を厳格化 → `content.js` 側の `data.toc !== undefined` チェックと整合
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [x] `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` の `page.addInitScript` 内 TestWebSocket 定義
  - 内容: 共有ブラウザハーネスモジュールとして切り出し、`page.addInitScript(path)` で読み込む
  - 理由: 2 spec で TestWebSocket 定義が重複、片方に `setTimeout` override が付く等の drift が発生している
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [x] `as unknown as` double-cast の説明コメント追加
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` の `stabilizeWebSocketHarness` 内
  - 内容: `window.__realWsOnmessage = window.__lastWs.onmessage! as unknown as (ev: { data: string }) => void;` の直前に、`__dispatchWsMessage` が MessageEvent を生成せず `{ data: string }` を直接渡すため契約を狭めている旨の日本語コメント
  - 理由: 2 箇所の strict エスケープハッチが無説明。`MessageEvent` contravariance の問題を説明しないと将来の保守者が削除しかねない
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [x] `window.updateContent` を E2E モード限定 expose に変更
  - ファイル: `src/template/assets/js/content.js`
  - 内容: Playwright 実行時のみ expose する形 (`if (window.__MV_E2E__ === true) { window.updateContent = updateContent; }`) に変更。truthy 非 boolean では expose しない。E2E 側は `page.addInitScript(() => { window.__MV_E2E__ = true; })` で有効化
  - 理由: 個人 markdown viewer (127.0.0.1 限定) なので実害はないが、テスト hook が production HTML に常時露出している。将来 OSS 化 / 公開ホスティングに転じた際にサニタイズ層をバイパスして任意 HTML payload を流す呼び出しが可能になる
  - 由来: PR #80 レビュー (2026-04-20)
