# BACKLOG.md Deep Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/todo/BACKLOG.md` を深掘り監査結果に合わせて更新し、今後の実行候補として使いやすい短い backlog にする。

**Architecture:** docs-only 変更として `docs/todo/BACKLOG.md` だけを更新する。実装済み項目は `Done` へ移し、TOC pending navigation 関連の細かい 3 項目は 1 項目へ統合し、現在も根拠がある P2/P3 項目だけを未完了リストに残す。

**Tech Stack:** Markdown, ripgrep, git

**Design reference:** `docs/superpowers/specs/2026-04-28-backlog-deep-audit-design.md`

---

## ファイル構成

- Modify: `docs/todo/BACKLOG.md`
  - 実装済み P1 項目を未完了リストから外す。
  - `E2E を verify.sh に統合するか検討` を opt-in 統合済みとして `Done` に移す。
  - TOC pending navigation 関連 3 項目を 1 項目へ統合する。
  - P2/P3 項目のファイル拡張子、現状説明、根拠を現在のコードに合わせる。
- Read only: `docs/superpowers/specs/2026-04-28-backlog-deep-audit-design.md`
  - 受け入れ基準と検証方針を確認する。
- Unchanged: `src/**`, `tests/**`, `verify.sh`, `README.md`
  - 今回は backlog 文書だけを更新し、コード・テスト・検証スクリプト・README は変更しない。

---

### Task 1: 監査根拠を再確認する

**Files:**
- Inspect: `docs/todo/BACKLOG.md`
- Inspect: `docs/superpowers/specs/2026-04-28-backlog-deep-audit-design.md`
- Inspect: `tests/e2e/memo_jump.spec.ts`
- Inspect: `tests/e2e/text_selection_defer.spec.ts`
- Inspect: `src/template/assets/js/content.js`
- Inspect: `src/template/assets/js/sidebar.js`
- Inspect: `src/server/files/catalog.rs`
- Inspect: `README.md`

- [ ] **Step 1: 作業ツリーが計画実行前に想定通りか確認する**

Run:

```bash
git status --short --branch
```

Expected: `docs/superpowers/plans/2026-04-28-backlog-deep-audit.md` 以外の未コミット差分がない。別の差分がある場合は、ユーザー変更として触らず、`BACKLOG.md` 更新に干渉しないことを確認する。

- [ ] **Step 2: DOM cleanup が実装済みであることを確認する**

Run:

```bash
rg -n "container\\.lastChild|lastChild\\.remove\\(|lineHint\\.remove\\(\\)" tests/e2e/memo_jump.spec.ts
```

Expected: `lineHint.remove()` が複数表示され、`container.lastChild` と `lastChild.remove()` は表示されない。

- [ ] **Step 3: verify E2E opt-in 統合の根拠を確認する**

Run:

```bash
git show --stat --oneline 4241390
```

Expected: `chore: verifyにE2E opt-inを追加 (#100)` として `verify.sh` と関連 docs/spec/plan が変更されている。

- [ ] **Step 4: P2/P3 に残す根拠が現在も存在することを確認する**

Run:

```bash
rg -n "Codex review #4136142343|document\\.createElement\\('span'\\)|TOC_NAVIGATION_SLACK_PX|markPendingTocNavigation|collect::<Vec<_>>|\\.join\\(\"/\"\\)|websocket\\.rs|renderer\\.rs|template\\.rs|files\\.rs" tests/e2e/memo_jump.spec.ts tests/e2e/text_selection_defer.spec.ts src/template/assets/js/content.js src/template/assets/js/sidebar.js src/server/files/catalog.rs README.md
```

Expected:

- `Codex review #4136142343` が `tests/e2e/memo_jump.spec.ts` に表示される。
- `TOC_NAVIGATION_SLACK_PX` と `markPendingTocNavigation` が `src/template/assets/js/sidebar.js` に表示される。
- `collect::<Vec<_>>` と `.join("/")` が `src/server/files/catalog.rs` に表示される。
- `websocket.rs`、`renderer.rs`、`template.rs`、`files.rs` が `README.md` のアーキテクチャ図に表示される。
- `tests/e2e/memo_jump.spec.ts` に `document.createElement('span')` の直接テストは表示されない。コマンド全体の exit code が 0 でも、これは `src/template/assets/js/content.js` 側の span 生成や他パターンを拾うため、目視で判断する。

---

### Task 2: `BACKLOG.md` を監査結果に合わせて置き換える

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: `docs/todo/BACKLOG.md` 全体を次の内容に置き換える**

Use `apply_patch` to replace the file content with:

```markdown
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

- [ ] TOC pending navigation の境界回帰テスト強化
  - ファイル: `tests/e2e/text_selection_defer.spec.ts`
  - 内容:
    - grace 400ms 以内に `clickTocLink('alpha')` → `clickTocLink('beta')` と連続クリックしたとき、最終 active と scrollY が 2 番目のリンク先に正しく収束することを検証する
    - `TOC_NAVIGATION_SLACK_PX` の境界として、`SLACK - 2 = 22px` で pending 維持、`SLACK + 2 = 26px` で通常判定復帰を検証する
    - `MutationObserver` で `#toc a.active` の `class` 遷移を監視し、小揺らし中に Beta 以外へ切り替わらないことを検証する
  - 理由: 3 項目はいずれも `src/template/assets/js/sidebar.js` の pending TOC navigation に対する局所回帰検知。個別 backlog のままだと粒度が細かすぎるため、同一テスト群の強化としてまとめる
  - 由来: PR #77 レビュー (2026-04-20)

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

- [x] E2E テストの DOM クリーンアップ戦略見直し
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: `augmentHashWithTrailingLineHint` 系テストの DOM cleanup を、`container.lastChild` 推測ではなく、追加した `lineHint` ノードを直接 `remove()` する方式へ変更
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
```

---

### Task 3: 文書検証を実行する

**Files:**
- Verify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 主要項目が期待通り残っていることを確認する**

Run:

```bash
rg -n "現時点で未完了項目なし|TOC pending navigation|augmentHashWithTrailingLineHint|Codex review ID|E2E 共通ヘルパー|render_markdown|catalog.rs|README|opt-in 統合" docs/todo/BACKLOG.md
```

Expected: P1 の空状態、P2/P3 の残存項目、Done の opt-in 統合が表示される。

- [ ] **Step 2: 古い未完了 P1 記述が残っていないことを確認する**

Run:

```bash
rg -n "container\\.lastChild|lastChild\\.remove\\(|memo_jump\\.spec\\.js|text_selection_defer\\.spec\\.js|L347|L359 付近|L245-L407|現状 `tsc --noEmit` のみ" docs/todo/BACKLOG.md
```

Expected: no output, exit code 1.

- [ ] **Step 3: チェックボックスと見出し構造を確認する**

Run:

```bash
rg -n "^- \\[ \\]|^- \\[x\\]|^## P1|^## P2|^## P3|^## Done" docs/todo/BACKLOG.md
```

Expected:

- `## P1`, `## P2`, `## P3`, `## Done` が表示される。
- 未完了 `[ ]` は P2 に 4 件、P3 に 4 件、合計 8 件。
- 完了 `[x]` は Done に 8 件。

- [ ] **Step 4: docs-only 差分であることを確認する**

Run:

```bash
git diff --stat
```

Expected: `docs/todo/BACKLOG.md` だけが変更されている。計画ファイルを同じ作業単位で未コミットにしている場合は、`docs/superpowers/plans/2026-04-28-backlog-deep-audit.md` も表示されてよい。

- [ ] **Step 5: 変更内容を目視確認する**

Run:

```bash
git diff -- docs/todo/BACKLOG.md
```

Expected:

- `E2E テストの DOM クリーンアップ戦略見直し` が P1 から `Done` に移っている。
- `E2E を verify.sh に統合するか検討` が `E2E を verify.sh に opt-in 統合` として `Done` に移っている。
- TOC pending navigation 関連 3 項目が 1 項目に統合され、3 つの検証観点が残っている。
- P2/P3 の残存項目が現在の `.ts` ファイル名や現行構成を参照している。
- 外部レビュー由来の記述が現在の脆弱性として断定されていない。

---

### Task 4: コミットする

**Files:**
- Commit: `docs/todo/BACKLOG.md`

- [ ] **Step 1: `BACKLOG.md` だけをステージする**

Run:

```bash
git add docs/todo/BACKLOG.md
```

Expected: `docs/todo/BACKLOG.md` がステージされる。

- [ ] **Step 2: ステージ内容を確認する**

Run:

```bash
git diff --cached --stat
```

Expected: `docs/todo/BACKLOG.md` だけが表示される。

- [ ] **Step 3: コミットする**

Run:

```bash
git commit -m "docs: BACKLOGを深掘り監査結果で整理"
```

Expected: docs-only commit が作成される。

## セキュリティ確認

この変更は backlog 整理であり、セキュリティ境界そのものを強化しない。外部レビュー由来の記述は未検証入力として扱い、現在の脆弱性として断定しない。検証基盤や回帰検知に関わる項目を適切に残すことで、将来のセキュリティ関連回帰を見落としにくくする。

## ロールバック

`docs/todo/BACKLOG.md` の更新コミットを revert すれば元に戻せる。コード、テスト、`verify.sh` は変更しないため、ロールバックの影響範囲は backlog 文書に限定される。
