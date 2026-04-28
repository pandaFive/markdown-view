# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium が TODO.md から捌けてから着手する候補。
未完了項目はリスク低減効果を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）の発見コンテキスト。

## P1: リスク低減・検証基盤

現時点で未完了項目なし。

## P2: 保守性・局所回帰検知

- [ ] `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling の直接回帰テスト
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: `document.createElement('span')` で `L15` を内包したノードを `link.nextSibling` に置き、`augmentHashWithTrailingLineHint(link, '#section-b')` が `#section-b:L15` を返すことを直接検証する
  - 理由: `src/template/assets/js/content.js` のドックコメントは、renderer がソース行トラッキング用に text を `<span>` でラップするケースに対応すると説明している。実装は `TEXT_NODE` と `ELEMENT_NODE` の双方で `textContent` を見るが、現行テストは text node 経路中心で、ELEMENT_NODE sibling の設計意図を直接固定していない
  - 由来: PR #76 レビュー (2026-04-20)

- [ ] E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出
  - ファイル: `tests/e2e/{memo_quote,memo_sync,memo_jump,markdown_links,text_selection_defer,document_search}.spec.ts`
  - 内容: `resetFixtures`、`selectParagraphText`、`stabilizeWebSocketHarness`、`requireUpdateContent` 系など、複数 spec に残る近いヘルパーを共通モジュールへ抽出する
  - 理由: DRY 違反、片方を修正して片方を忘れるリスク。`tests/e2e/browser/test-websocket.ts` と `tests/e2e/globals.d.ts` は既に整備済みだが、spec-local helper の重複は残っている
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `memo_jump.spec.ts` の Codex review ID コメント削除
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: `Codex review #4136142343` という外部 review system の ID 参照を除去し、回帰保護の対象である false-positive パターンの説明に置き換える
  - 理由: ID は Codex 側でアーカイブされると参照不能。テストコメントは外部 ID ではなく、壊したくない仕様と入力パターンを説明する方が保守しやすい
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20、既存 PR #76 持ち込み課題)

## P3: 長期改善・低緊急

- [ ] インラインブラウザJS の TS 化
  - ファイル: `src/template/assets/js/{bootstrap,content,fetch,memo,selection,sidebar,websocket}.js`
  - 内容: Rust の `include_str!` でコンパイル時に埋め込まれる JS を TS で記述し、事前 tsc でビルドして `.js` 出力を `include_str!` 対象にする
  - 理由: ブラウザ側 JS は現在無型。ただし Rust ビルドパイプラインへの Node 依存追加が必要で、「Rust 単体ビルド」の明快さが崩れる
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `render_markdown` の責務分割（大規模）
  - ファイル: `src/renderer/mod.rs`
  - 現状: pulldown-cmark の `Event` ループと状態管理（heading / code block / table / image / link の各フェーズ）が 1 関数に同居。ファイル全体も大きい
  - 対応方針: フェーズ別ハンドラを `RenderState` の impl メソッドとして抽出し、メイン関数はイベントディスパッチのみに寄せる
  - 注意: 大規模リファクタ。既存テスト（`renderer_test.rs`, `toc_test.rs`）が振る舞い等価性を担保するため、先にテストカバレッジを確認
  - 理由: renderer の保守重心は既に認識済みで、今後の Markdown 拡張時に局所変更しやすくする
  - 由来: PR #59 探索 (2026-04-18)

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

- [x] TOC pending navigation の境界回帰テスト強化
  - ファイル: `tests/e2e/text_selection_defer.spec.ts`
  - 内容:
    - grace 400ms 以内の連続 TOC クリックで最後のクリック先に収束することを検証
    - `TOC_NAVIGATION_SLACK_PX` の内側 / 外側で pending 維持と通常判定復帰が分かれることを検証
    - `MutationObserver` で小揺らし中にクリック先以外へ active が切り替わらないことを検証
  - 完了根拠: `16dc04f test: TOC pending navigation境界を固定`
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
