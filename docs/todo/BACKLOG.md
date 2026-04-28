# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium が TODO.md から捌けてから着手する候補。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）の発見コンテキスト。

## Low Priority

- [ ] `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling のユニットテスト
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 行番号: L359 付近（既存 augmentHashWithTrailingLineHint テスト群と併設）
  - 理由: `src/template/assets/js/content.js` L181-L182 のドックコメント『renderer がソース行トラッキング用に text を `<span>` でラップするケースに対応』という設計意図を固定する直接テストが欠如している。現状は L205 の「旧形式メモ」E2E で実レンダ経由の TEXT_NODE パスのみカバー。`document.createElement('span')` で `L15` を内包したノードを sibling に置いて `textContent` 経路が生きることを明示的に検証する
  - 由来: PR #76 レビュー (2026-04-20)

- [ ] E2E テストの DOM クリーンアップ戦略見直し
  - ファイル: `tests/e2e/memo_jump.spec.js`（全 augmentHashWithTrailingLineHint 系テスト）
  - 行番号: L245-L407 の `try/finally` ブロック
  - 内容: 現状は `container.lastChild && lastChild.nodeType === TEXT_NODE` で末尾を削除しているが、並列で別ノードが挿入された場合に想定外ノードを削除する脆さがある。`afterEach` で `memo-preview` innerHTML のスナップショット復元に寄せると安全
  - 理由: PR #76 レビュー（pr-test-analyzer）で指摘された全テスト共通の懸念。本 PR 単独の課題ではなくテスト基盤改善
  - 由来: PR #76 レビュー (2026-04-20)

- [ ] 猶予期間中の連続 TOC クリックでの挙動検証
  - ファイル: `tests/e2e/text_selection_defer.spec.js`
  - 内容: `markPendingTocNavigation` は無条件に id と時刻を上書きする（sidebar.js L180-186）。grace 400ms 以内に `clickTocLink('alpha')` → `clickTocLink('beta')` と連続クリックしたとき、最終 active と scrollY が 2 番目のリンク先に正しく収束することを検証するテストが欠落
  - 理由: pending 上書き仕様が壊れた場合（条件付き更新などに書き換え）の回帰検知
  - 由来: PR #77 レビュー (2026-04-20)

- [ ] `TOC_NAVIGATION_SLACK_PX` 境界の回帰テスト
  - ファイル: `tests/e2e/text_selection_defer.spec.js` L347-L359 近辺
  - 内容: 現 L347 の小揺らし検証は `+6px` ハードコード。`SLACK - 2 = 22px` で pending 維持、`SLACK + 2 = 26px` で通常判定復帰を 2 ポイントで検証すれば SLACK 定数縮小の回帰を検出できる
  - 理由: 定数変更時のテスト反映漏れ検知
  - 由来: PR #77 レビュー (2026-04-20)

- [ ] L347 を active 遷移フラッシュ厳密検証に強化
  - ファイル: `tests/e2e/text_selection_defer.spec.js` L347-L359
  - 内容: 現在の `expect.poll(...).toBe('Beta')` は「最終的に Beta なら通る」。一瞬 `Alpha` に遷移して戻るケースを見逃す。`MutationObserver` で `#toc a.active` の `class` 遷移を監視し、Beta 以外への切り替わりが 0 回であることを主張するように強化
  - 理由: フラッシュ系の視覚バグは poll で見逃されるため、より厳密な回帰検知を整備する
  - 由来: PR #77 レビュー (2026-04-20)

- [x] `window.updateContent` を E2E モード限定 expose に変更
  - ファイル: `src/template/assets/js/content.js` L1303 (現状 `window.updateContent = updateContent;`)
  - 内容: Playwright 実行時のみ expose する形 (`if (window.__MV_E2E__ === true) { window.updateContent = updateContent; }`) に変更。truthy 非 boolean では expose しない。E2E 側は `page.addInitScript(() => { window.__MV_E2E__ = true; })` で有効化
  - 理由: 個人 markdown viewer (127.0.0.1 限定) なので実害はないが、テスト hook が production HTML に常時露出している。将来 OSS 化 / 公開ホスティングに転じた際にサニタイズ層をバイパスして任意 HTML payload を流す呼び出しが可能になる
  - 由来: PR #80 レビュー (2026-04-20)

- [ ] E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` に散在する `declare global { interface Window { ... } }` + ブラウザバンドル関数/変数の declare
  - 内容: 共通 ambient 宣言を `tests/e2e/globals.d.ts` に一本化。各 spec の `declare global` を削除。`tsconfig.json` の `include` で拾う
  - 理由: `Window.__lastWs` / `__realWsOnmessage` が text_selection_defer と document_search で byte 一致しているが、片方を変更すると TS2717 で破綻するリスクを根治。`selectFile` / `updateContent` 宣言の spec 間不整合も解消
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `updateContent` 型宣言の統一
  - ファイル: `tests/e2e/memo_jump.spec.ts` (Window.updateContent プロパティ型), `tests/e2e/document_search.spec.ts` (top-level function 型)
  - 内容: 同じランタイム binding に対し 2 通りの型宣言が存在。`opts` が memo_jump では required、document_search では optional と不整合。どちらかに統一
  - 理由: 同一 binding を 2 型で捕捉しているため、片方の型が誤っても検出不能。ペイロード union (`{ refresh: true }` / `file?: string` 等) も未表現
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `as unknown as` double-cast の説明コメント追加
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` の `stabilizeWebSocketHarness` 内
  - 内容: `window.__realWsOnmessage = window.__lastWs.onmessage! as unknown as (ev: { data: string }) => void;` の直前に、`__dispatchWsMessage` が MessageEvent を生成せず `{ data: string }` を直接渡すため契約を狭めている旨の日本語コメント
  - 理由: 2 箇所の strict エスケープハッチが無説明。`MessageEvent` contravariance の問題を説明しないと将来の保守者が削除しかねない
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `memo_jump.spec.ts:303` の Codex review ID 削除
  - 内容: 外部 review system の ID 参照を除去し、回帰保護の対象である false-positive パターンの説明に置き換える
  - 理由: ID は Codex 側でアーカイブされると参照不能、典型的な rot-prone comment
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20、既存 PR #76 持ち込み課題)

- [ ] `tsconfig.json` に strict flag 追加
  - 内容: strict の上位に両 flag を有効化
  - 理由:
    - `noUncheckedIndexedAccess`: `__clickObservations[href]` 等の Record アクセスに `undefined` 可能性を強制 → missing key のバグを発見
    - `exactOptionalPropertyTypes`: `toc?: string` と `toc: undefined` の区別を厳格化 → `content.js` 側の `data.toc !== undefined` チェックと整合
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出
  - ファイル: `tests/e2e/{memo_quote,memo_sync,memo_jump,markdown_links,text_selection_defer}.spec.ts` に重複するヘルパー
  - 内容: 複数 spec で同一実装されているヘルパー関数を共通モジュールに抽出
  - 理由: DRY 違反、片方を修正して片方を忘れるリスク。TS 化の副産物として可視化されたが、E2E TS 移行スコープ外として延期
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` の `page.addInitScript` 内 TestWebSocket 定義
  - 内容: 共有ブラウザハーネスモジュールとして切り出し、`page.addInitScript(path)` で読み込む
  - 理由: 2 spec で TestWebSocket 定義が重複、片方に `setTimeout` override が付く等の drift が発生している
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] E2E を `verify.sh` に統合するか検討
  - ファイル: `verify.sh`
  - 内容: 現状 `tsc --noEmit` のみで `npm run test:e2e` は手動実行。verify.sh で Rust server 立ち上げ→ playwright 実行まで含めるか
  - 理由: E2E を CI で回していない現状、type check のみが SSoT。実行コストと速度のトレードオフ要検討
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] インラインブラウザJS の TS 化
  - ファイル: 7 ファイル (bootstrap, content, fetch, memo, selection, sidebar, websocket)
  - 内容: Rust の `include_str!` でコンパイル時に埋め込まれる JS を TS で記述し、事前 tsc でビルドして `.js` 出力を `include_str!` 対象にする
  - 理由: ブラウザ側 JS は現在無型。ただし Rust ビルドパイプラインへの Node 依存追加が必要で、「Rust 単体ビルド」の明快さが崩れる
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `render_markdown` の責務分割（大規模）
  - ファイル: `src/renderer/mod.rs` L184-540（約 357 行）
  - 現状: pulldown-cmark の `Event` ループと状態管理（heading / code block / table / image / link の各フェーズ）が 1 関数に同居。ファイル全体 951 行
  - 対応方針: フェーズ別ハンドラを `RenderState` の impl メソッドとして抽出、メイン関数はイベントディスパッチのみにする
  - 注意: 大規模リファクタ。既存テスト（`renderer_test.rs`, `toc_test.rs`）が振る舞い等価性を担保するため、先にテストカバレッジを確認
  - 理由: CLAUDE.md にも「見出しパースが 2 回実行される既知トレードオフ」が記載されており、renderer の保守重心は既に認識済み
  - 由来: PR #59 探索 (2026-04-18)

- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
  - ファイル: `src/server/files/catalog.rs` L127-128
  - 現状: 相対パス構築で `collect::<Vec<_>>()` してから `join("/")`。上限 1000 件だが呼出あたり Vec アロケーションが発生
  - 対応: イテレータ駆動で直接 String を構築する（`itertools::Itertools::join()` もしくは手書き fold）
  - 理由: マイクロ最適化。計測前に効果確認推奨
  - 由来: PR #59 探索 (2026-04-18)

- [ ] README のアーキテクチャ図を実装構成に揃える
  - ファイル: `README.md` L121-125 周辺
  - 現状: `websocket.rs` / `renderer.rs` / `template.rs` / `files.rs` が単一ファイル前提で記載。実装は `src/server/session.rs`、`src/renderer/`（ディレクトリ）、`src/template/`（ディレクトリ）、`src/server/files/`（サブモジュール分割）
  - 対応: CLAUDE.md の「アーキテクチャ」節と同じ粒度で README を更新
  - 理由: ドキュメント rot。新規コントリビュータが実装構造を誤解する
  - 由来: PR #59 探索 (2026-04-18)
