# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善・完了済みの履歴を置く。
未完了項目は重要度と将来影響度を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）以降の発見コンテキスト。

最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## P1: リスク低減・契約明文化

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索のキャンセル境界と allocation 削減を検討する
  - ファイル: `src/server/files/search.rs`, `src/template/assets/js/directory-search.js`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数の打ち切りも明示されている。一方、連続検索時に古い検索処理をキャンセルする仕組みはなく、`SearchResultItem` の `before/current/after` はマッチごとに `String` を確保する
  - 対応: クライアント検索世代とサーバ側処理の対応、古い検索結果の破棄、`Cow<str>` 化や検索ブロック処理の allocation 削減を、計測結果に基づいて検討する
  - 判断: 検索負荷制御は実装済みで、残件は効率化と古い結果の扱いなので BACKLOG P2 に残す
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)

- [ ] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - ファイル: `src/server/state.rs` L18-24/L112/L132
  - 現状: `CanonicalPath::try_from_path` で `canonicalize` した直後に `is_file()`/`is_dir()` で判定するが、両者の間に rename/unlink される race window がある。実害は起動時の `AppMode::new_*` のみで影響は小さい
  - 対応: `metadata` を一度取得してから `is_file`/`is_dir` を判定し、race window を縮める。`AppModeBuildError` のメッセージも metadata 起点に整理
  - 判断: path safety に関係するが起動時限定で影響が小さいため BACKLOG P2 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `log_path::canonicalize_status` の毎回 syscall を削減する
  - ファイル: `src/server/log_path.rs` L52-65
  - 現状: ログ出力ごとに `path` と `base` を canonicalize する。warn/error 時のみ呼ばれるが、ログ storm 状況下では I/O が増える
  - 対応: `base` の canonicalize 結果を起動時に一度だけ算出してキャッシュし、ログ経路では path 側のみ canonicalize する。または `OnceLock` で base を保持
  - 判断: ログ storm 時の効率化であり、現行の安全性を弱めていないため BACKLOG P2 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

## P3: 長期改善・低緊急

- [ ] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
  - ファイル: `src/server/guards.rs`
  - 現状: PR #123 で Host middleware 後段に到達した Host 系 `WsOriginRejection` を `error!` ログとして観測できるようにした。個人向け localhost ツールとしてはログで十分だが、本格運用や継続監視を想定するなら、発生回数をメトリクスやカウンタとして扱う余地がある
  - 対応: 実運用で bypass 兆候を集計する必要が出た場合のみ、軽量なカウンタや structured logging 連携を検討する。現時点では依存追加やメトリクス基盤導入は YAGNI とする
  - 判断: 既に error ログがあり、メトリクス基盤は実運用要求が出てからでよいため BACKLOG P3 に残す
  - 由来: PR #123 レビュー follow-up (2026-05-04)

- [ ] サイドバーの "Documents" 文字列を i18n または日本語化
  - ファイル: `src/server/routes.rs` L33-41 (`sidebar_directory_name`)
  - 現状: `unwrap_or("Documents")` で英語固定。日本語 UI でも同名が出る
  - 対応: 日本語デフォルト（"ドキュメント"）にするか、ディレクトリ名取得失敗時のフォールバック挙動をコメントで明示
  - 判断: UI 文言の局所改善であり、安全性や後続設計への影響は小さいため BACKLOG P3 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] インラインブラウザJS の TS 化
  - ファイル: `src/template/assets/js/{bootstrap,content,fetch,memo,selection,sidebar,websocket}.js`
  - 内容: Rust の `include_str!` でコンパイル時に埋め込まれる JS を TS で記述し、事前 tsc でビルドして `.js` 出力を `include_str!` 対象にする
  - 理由: ブラウザ側 JS は現在無型。ただし Rust ビルドパイプラインへの Node 依存追加が必要で、「Rust 単体ビルド」の明快さが崩れる
  - 判断: 型安全性の長期改善だが、Node 依存追加の設計判断が必要なため BACKLOG P3 に残す
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
  - ファイル: `src/server/files/catalog.rs`
  - 現状: 相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使っている。上限 1000 件だが呼出あたり Vec アロケーションが発生する
  - 対応: 計測または必要性確認のうえ、イテレータ駆動で直接 String を構築する（`itertools::Itertools::join()` もしくは手書き fold）
  - 判断: マイクロ最適化であり、実装前に効果確認が必要なため BACKLOG P3 に残す
  - 由来: PR #59 探索 (2026-04-18)

## Done

- [x] `data-memo-file` 属性を None 時にスキップする
  - ファイル: `src/template/page.rs`
  - 内容: `MemoResponse.file()` が `None` の初期 HTML では `data-memo-file` 属性を出力せず、`Some(file)` の場合だけ既存の `html_attr` / `html_escape` 経由で属性を出力する契約へ揃えた。現行 JS は `data-memo-file` を参照していないため、bootstrap.js 側の新規読み取り契約は追加しない方針にした。
  - 完了根拠: `cargo test template::page::tests::test_メモfileがnoneの場合data_memo_file属性を出力しない`, `cargo test template::page::tests::test_メモuiが描画される`, `cargo test template::page`, `cargo test --all-targets --all-features`, `./verify.sh`
  - 由来: アーキテクチャレビュー (2026-04-30)

- [x] `BroadcastMessage::Update` 系のシリアライズ失敗時の fallback JSON を整備する
  - ファイル: `src/server/messages.rs`
  - 内容: `BroadcastMessage::Update` の JSON 生成に明示 fallback を追加し、シリアライズ失敗時も最小構造の update JSON を返す契約に整理した。fallback は本文を含めず、`file` はディレクトリモードの routing に必要な場合だけ保持する。
  - 完了根拠: `cargo test server::messages`, `cargo test --all-targets --all-features`, `./verify.sh`
  - 由来: アーキテクチャレビュー (2026-04-30)

- [x] `read_route_memo` の二重サイズチェックを単一化する
  - ファイル: `src/server/files/memo.rs`, `src/server/files/memo_fs.rs`
  - 内容: メモ読込サイズ上限の契約を `read_with_limit` 側へ集約し、呼び出し側の post-read 重複チェックを削除した。サイズ上限は metadata と `read_with_limit` による実読込上限で維持し、read 時に `MemoReadError::TooLarge` へ落ちる経路も API レベルで 413 に変換されることを固定した。
  - 完了根拠: `cargo test server::files::tests::memo_route::test_load_route_memo_read_with_limit_too_largeは413を返す`, `cargo test --all-targets --all-features`, `./verify.sh`
  - 由来: アーキテクチャレビュー (2026-04-30)

- [x] `tokio::select!` の cancel-safe 性をコメントで明記する
  - ファイル: `src/server/session.rs`
  - 内容: WebSocket セッションの `tokio::select!` について、現在の branch が cancel-safe な future だけで構成される前提をコメントで明記した。挙動変更はなく、将来の branch 追加時の確認観点を残した。
  - 完了根拠: `cargo test --all-targets --all-features`, `./verify.sh`
  - 由来: アーキテクチャレビュー (2026-04-30)

- [x] 未知言語コードブロックの silent fallback に debug 観測ログを追加
  - ファイル: `src/renderer/highlight.rs`, `tests/renderer_test.rs`
  - 内容: 未知言語の syntax lookup 失敗時に、同一 language につき初回だけ `tracing::debug!` を出すようにした。HTML fallback 出力は維持し、ログに出す language は制御文字を escape し、長大入力は UTF-8 境界で切り詰める。重複抑制は固定長 fingerprint と 256 件上限で、未知言語名の長大文字列を保持しない。
  - 完了根拠: `cargo test --lib renderer::highlight -- --nocapture`, `cargo test --test renderer_test test_未知言語コードブロックはフォールバック描画される -- --nocapture`, `cargo test --all-targets --all-features`, `./verify.sh`
  - 由来: アーキテクチャレビュー (2026-04-30)

- [x] README のアーキテクチャ図を実装構成に揃える
  - ファイル: `README.md`
  - 内容: `src/server/session.rs`、`src/server/files/`、`src/renderer/`、`src/template/`、`watcher/` を含む現行構成へアーキテクチャ図を更新した。
  - 完了根拠: README のアーキテクチャ図
  - 由来: PR #59 探索 (2026-04-18)

- [x] Superpowers spec/plan の長期保存方針を整理する
  - ファイル: `docs/superpowers/README.md`
  - 内容: `specs/` は長期参照する設計判断、`plans/` は実装前計画として扱う方針を明文化した。完了済み plan は参照価値がある場合に残し、実行ログとして rot する場合は要点化する基準を追加した。
  - 完了根拠: `docs/superpowers/README.md` の保存方針

- [x] CLAUDE.md のアーキテクチャ記述を現在の実装構成に揃える
  - ファイル: `CLAUDE.md`
  - 内容: `server/service.rs`、`watcher/`、`renderer/`、`template/assets/` を含む現行構成へ更新し、見出し情報共有の説明を `render_document` 起点に修正した。
  - 完了根拠: `CLAUDE.md` のアーキテクチャ図と設計判断

- [x] プロダクト定義を Markdown previewer から Markdown workspace へ明文化する
  - ファイル: `README.md`, `CLAUDE.md`, `Cargo.toml`
  - 内容: メモ、引用、横断検索、ファイルツリー、ライブ更新を Markdown workspace の中核機能として説明した。純プレビュー化や `--no-memo` は今回も非目標として扱い、既存セキュリティ説明は弱めていない。
  - 完了根拠: README 冒頭、特徴一覧、Cargo description、CLAUDE.md 概要

- [x] `render_markdown` と `extract_headings` の早期 return 非対称を解消する
  - ファイル: `src/renderer/mod.rs`, `tests/renderer_test.rs`
  - 内容: `extract_headings("")` を明示的な早期 return にし、空入力が空配列を返す契約をテストで固定した。
  - 完了根拠: `test_extract_headingsは空入力で空配列を返す`

- [x] `CanonicalPathError` などの内部利用型を `pub(crate)` に絞る
  - ファイル: `src/server.rs`
  - 内容: `CanonicalPathError` の re-export を crate 内へ絞った。`AppModeBuildError` は public constructor の戻り値に含まれるため public のまま残した。
  - 完了根拠: `cargo test --all-targets --all-features`

- [x] `RouteTargetKind::include_file_list` を match 完全列挙に変更する
  - ファイル: `src/server/files/resolve.rs`
  - 内容: `Page` / `ApiContent` / `ApiMemo` を `match` で完全列挙し、新 variant 追加時に file list 要否を見直す構造にした。
  - 完了根拠: `cargo test --all-targets --all-features`

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
