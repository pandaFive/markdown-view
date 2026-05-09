# Content Controller Boundary Design

## Goal

`src/template/assets/js/content.js` を、本文領域の全機能を抱える巨大ファイルから、本文ライフサイクルを束ねる controller と小さな機能モジュール群へ再境界化する。

主な目的は次の通り。

- 検索、ディレクトリ検索、リンク解決、アンカー/履歴ナビゲーション、描画後 enhancement を別々に理解・検証できる構造にする。
- `content-renderer.js` に切り出し済みの HTML 反映境界を維持し、`innerHTML` の信頼境界を広げない。
- `appContext` を共有状態の置き場として残しつつ、各機能は `createX(ctx, deps)` 形式の明示的な入口を持つ。
- production `window` への API 露出を増やさず、E2E hook は `window.__MV_E2E__ === true` の場合だけ公開する。

## Non-goals

- ES modules、bundler、production TypeScript、追加ビルド手順の導入。
- UI 仕様、検索 API、WebSocket protocol、renderer の sanitize 方針の変更。
- メモ、検索、リンク、TOC、WebSocket 更新機能の削減。
- `inline_script.rs` の IIFE 連結モデルや CSP hash 自動生成の変更。

## Current State

`content.js` は 1300 行超で、次の責務を同じファイル内に持っている。

- live status、文書 stats、読書進捗、タイトル同期。
- 相対 Markdown リンク、`?file=...#hash`、同一ファイル hash、行番号 hash、旧形式メモ citation の解決。
- heading anchor と code copy の描画後 enhancement。
- 文書内検索、検索 highlight、検索結果 list 描画。
- ディレクトリ横断検索、`/api/search` fetch、generation 管理、検索結果からのファイル遷移。
- WebSocket / fetch 経由の pending update 適用と `updateContent`。

一方で `content-renderer.js` はすでに `validateUpdatePayload`、`applySanitizedContentHtml`、`applySanitizedTocHtml` を持ち、サーバーで sanitize 済み HTML を `#content` / `#toc` へ反映する境界として機能している。この境界は維持する。

## Architecture

`content.js` の実体を次のモジュール群へ分割する。

### `document-search.js`

現在表示中の本文だけを対象にした検索を担当する。

責務:

- 本文 DOM から検索対象 block と text node を収集する。
- query を HTML として扱わず、text node 分割と `mark.document-search-match` 生成で highlight する。
- 現在 match の移動、検索 summary、文書内検索結果 list の描画を行う。
- `ctx.search.documentMatches` と `ctx.search.currentDocumentIndex` を更新する。

非責務:

- `/api/search` を呼ばない。
- ファイル切替を行わない。
- `#content` 全体の `innerHTML` 置換を行わない。

### `directory-search.js`

ディレクトリ横断検索を担当する。

責務:

- `/api/search?q=...` を呼び、HTTP error と JSON parse error を既存のユーザー文言に変換する。
- `ctx.search.documentFetchGeneration` による古い結果の破棄を維持する。
- query 不一致の応答を破棄する。
- loading、error、truncated、skipped files、current result index を管理する。
- 検索結果 list のうち、ディレクトリ検索固有の path と result 選択を描画する。

非責務:

- `selectFile` を直接呼ばない。
- 本文 highlight の内部実装を知らない。

依存:

- `deps.openFileSearchResult(file, options)` で controller へファイル遷移を依頼する。
- `deps.renderDocumentSearchResultContext(...)` で検索結果本文を DOM API で描画する。
- `deps.setCurrentDocumentSearchMatch(...)` / `deps.updateDocumentSearchSummary()` で現在ファイル内 highlight と summary に反映する。

### `content-navigation.js`

本文とメモ preview の内部リンク、hash、履歴復元を担当する。

責務:

- 相対 `.md` リンクを現在ファイル基準で解決する。
- `?file=foo.md#hash` と単一ファイルモードの同一 path + hash を解決する。
- `#heading:L5-L7`、`#L5-L7`、旧形式メモ citation の trailing `L5-L7` 補完を維持する。
- 同一ファイル内の hash navigation と `history.pushState` / `replaceState` を扱う。
- hash miss 時の warn、先頭戻し、hash clear の既存挙動を維持する。

非責務:

- 別ファイルへの遷移を直接実行しない。
- TOC tracking の内部状態を直接持たない。

依存:

- `deps.selectFile(file, pushHistory, options)` で別ファイル遷移を依頼する。
- `deps.markPendingTocNavigation(id)`、`deps.clearPendingTocNavigation()`、`deps.restoreActiveTocHeading(id)` で TOC と連携する。

### `content-enhancements.js`

本文 DOM 反映後の副作用を担当する。

責務:

- 文書 stats、読書進捗、タイトル同期。
- heading anchor の追加と heading link copy。
- code block copy button の追加。
- TOC filter setup。

非責務:

- 検索状態を持たない。
- ナビゲーション履歴を変更しない。
- `content-renderer.js` の HTML 反映境界を横取りしない。

### `content-controller.js`

本文領域の統合層。

責務:

- `createContentController(ctx, deps)` を公開する。
- `setup()` で content link navigation、memo link navigation、document search、scroll progress などを初期化する。
- `updateContent(data, options)` の外部契約を維持する。
- WebSocket の pending update を適用する。
- 描画後の順序を統制する。
- 検索再同期、TOC tracking、WebSocket applied update 通知を連携する。

返す API:

```js
{
  setup: Function,
  updateContent: Function,
  applyPendingUpdate: Function,
  restoreNavigationFromLocation: Function,
  openDocumentSearch: Function,
  moveDocumentSearch: Function,
  applyDocumentSearchQuery: Function,
  clearDocumentSearchQuery: Function,
  renderDirectorySearchUi: Function,
  scheduleDirectorySearch: Function,
  augmentHashWithTrailingLineHint: Function,
  setLiveStatus: Function,
  updateDocumentStats: Function,
  updateReadingProgress: Function,
  syncDocumentChrome: Function,
  enhanceContentInteractions: Function,
  setupTocFilter: Function
}
```

既存のトップレベル関数呼び出しは、段階的に `appContext.content.updateContent(...)` のような controller 経由へ移す。production `window` には controller API を公開しない。

## Data Flow

起動時:

```text
startMarkdownViewApp()
  -> createContentController(appContext, deps)
  -> appContext.content = contentController
  -> contentController.setup()
  -> memo / websocket / sidebar / selection setup
```

本文更新時:

```text
WebSocket / fetch
  -> contentController.updateContent(data, options)
  -> content-renderer validates payload
  -> content-renderer applies sanitized content and toc HTML
  -> navigation applies anchor/hash behavior
  -> enhancements update copy buttons, stats, title, progress, toc filter
  -> search reapplies current query
  -> websocket remembers applied live update
```

ディレクトリ検索結果を開く時:

```text
directory-search
  -> deps.openFileSearchResult(file, options)
  -> contentController delegates to selectFile
  -> fetch path updates content
  -> document-search reapplies highlight in selected file
  -> directory-search restores selected result index when generation/query still match
```

## Error Handling

- update payload の `content` / `toc` 欠落または型不正は、`content-renderer.js` で warn する。
- controller は契約違反時に `{ ok: false, contractViolation: true }` を返し、本文/TOC は更新しない。WebSocket 経路では成功状態へ戻さず error 表示にする。
- ディレクトリ検索の HTTP error、JSON parse error、fetch error は `directory-search.js` に閉じ込め、既存の `getFileFetchErrorMessage` 経由の表示を維持する。
- 古い generation、query 不一致、ファイル切替後に対応しない検索結果は破棄する。
- hash miss、リンクパス decode failure、同一ファイル内見出し未発見は `content-navigation.js` で warn し、既存の先頭戻しまたは hash clear 挙動を維持する。

## Security Considerations

- `#content` と `#toc` の HTML 置換は `content-renderer.js` に閉じ込める。
- `document-search.js` と `directory-search.js` は検索 query や検索結果文字列を `innerHTML` へ挿入しない。`textContent`、`createTextNode`、`appendChild` を使う。
- 検索 highlight は既存どおり text node を分割して `mark` 要素へ置換し、HTML 文字列を組み立てない。
- リンク解決は同一 origin、同一 path、相対 `.md`、`?file=` の `.md` 制限を維持する。
- `//` と外部 scheme は内部 navigation 対象にしない。
- production `window` へ `updateContent`、`openDocumentSearch`、controller、helper、`appContext` を公開しない。
- E2E hook は `window.__MV_E2E__ === true` の場合だけ `window.markdownViewTestHooks` に公開する。
- retrieved text、検索結果、Markdown 本文、メモ本文は untrusted input として扱い、DOM API 経由で表示する。

## Testing Strategy

既存回帰対象:

- `tests/e2e/document_search.spec.ts`
- `tests/e2e/markdown_links.spec.ts`
- `tests/e2e/memo_jump.spec.ts`
- `tests/e2e/update_content_exposure.spec.ts`
- `tests/e2e/memo_sync.spec.ts`
- `tests/e2e/text_selection_defer.spec.ts`

追加または補強する観点:

- production `window` に `updateContent`、`openDocumentSearch`、`appContext`、content controller API が露出していない。
- E2E mode のみ `markdownViewTestHooks` が必要な hook を公開する。
- 検索 query に HTML 風文字列を入れても、検索結果 list と highlight が HTML として解釈されない。
- ディレクトリ検索中にファイル切替が起きた場合、古い generation の結果が現在状態へ適用されない。
- `?file=...#hash`、同一ファイル hash、行番号 hash、旧形式メモ citation の navigation が維持される。

最終 verification:

```bash
./verify.sh
npm run test:e2e
```

必要に応じて、実装中は対象 E2E を絞って反復する。

## Acceptance Criteria

- `content.js` の主要責務が `document-search.js`、`directory-search.js`、`content-navigation.js`、`content-enhancements.js`、`content-controller.js` に分割されている。
- `content-controller.js` が本文領域の外部 API を束ね、他モジュールは controller または依存注入経由で連携している。
- `innerHTML` による sanitized content / toc 反映境界は `content-renderer.js` に残っている。
- 検索 query、検索結果、リンク由来文字列を HTML として挿入する新規経路がない。
- production `window` への新規 API 露出がない。
- 既存の検索、ディレクトリ検索、Markdown link navigation、memo citation jump、WebSocket/fetch update の挙動が維持されている。
- 対象 E2E と最終 verification が通る。

## Rollback Path

この変更は browser JS asset の分割が中心で、Rust server contract や Markdown renderer contract は変えない。問題が起きた場合は、該当 PR の revert で `inline_script.rs` の include 順と JS ファイル群を元に戻せる。

段階実装中に一部モジュールだけ問題が出た場合は、controller の公開 API を維持したまま、対象責務を一時的に `content-controller.js` 内へ戻す。`content-renderer.js` の sanitize 境界と production `window` 非露出の受け入れ条件は rollback 後も維持する。

## Work Estimates

人間実装見積もり: 1.5-2.5 日。既存 E2E の読み解き、手動ブラウザ確認、レビュー対応を含む。

Codex/AI 支援見積もり: 4-7 時間。モジュール分割自体は機械的に進めやすいが、検索・履歴・メモ citation・WebSocket 更新の回帰確認に時間を使う。
