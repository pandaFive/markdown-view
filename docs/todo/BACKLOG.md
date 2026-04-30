# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium が TODO.md から捌けてから着手する候補。
未完了項目はリスク低減効果を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）の発見コンテキスト。

## P1: リスク低減・検証基盤

現時点で未完了項目なし。

## P2: 保守性・局所回帰検知

現時点で未完了項目なし。

## P3: 長期改善・低緊急

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
