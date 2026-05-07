# Content Renderer Boundary Design

## Goal

`src/template/assets/js/content.js` の `updateContent` 周辺に、サーバー生成済み HTML を DOM へ反映する明示的な境界を作る。

今回の主目的は巨大なブラウザ JS 全体を分割することではない。最初に `innerHTML` の信頼境界、update payload 契約、no-op cache を小さな `content-renderer` 境界へ寄せる。これにより、後続の検索、ナビゲーション、controller 化を安全に進めやすくする。

## Non-Goals

- ES modules、bundler、production JS の TypeScript 化は導入しない。
- 検索、ナビゲーション、メモ、TOC tracking の大規模分割は今回行わない。
- サーバー API、WebSocket payload、HTML sanitizer、CSP ポリシーの意味は変えない。
- クライアント側 sanitizer は追加しない。

## Current Context

現状の `content.js` は約 1300 行あり、次の責務を同じファイルに持つ。

- live status、document chrome、reading progress の更新
- Markdown 内部リンク、メモ引用リンク、行番号 hash の解決
- heading/code block のコピー UI 追加
- document search と directory search
- `updateContent` による `#content` / `#toc` の DOM 反映
- スクロール復元、TOC tracking、検索同期、メモ UI、WebSocket 適用済み記録

直近の deglobalization 作業で production の `window` 露出は抑えられている。一方で、`updateContent` は `innerHTML` 代入、payload 契約検証、描画後副作用をまとめて扱っているため、信頼境界がファイル構造としては読みにくい。

## Architecture

新規ファイル `src/template/assets/js/content-renderer.js` を追加する。既存の no-build inline JS 方式は維持し、`src/template/assets/inline_script.rs` で `content.js` より前に読み込む。

`content-renderer.js` は、サーバー生成済み HTML を DOM へ反映する境界だけを担当する。`content.js` の `updateContent` は当面オーケストレーターとして残し、スクロール復元、TOC tracking、検索同期、メモ UI、WebSocket 適用済み記録の呼び出し順を維持する。

### Script Order

`inline_script.rs` の読み込み順は次を想定する。

```text
bootstrap.js
selection.js
content-renderer.js
content.js
memo.js
fetch.js
websocket.js
sidebar.js
startMarkdownViewApp()
```

`content-renderer.js` は `appContext` と同じ IIFE スコープに置かれるが、production の `window` へは露出しない。

## Content Renderer API

### `validateUpdatePayload(data)`

WebSocket/API から届いた update payload を検証する。

- `null`、配列、非 object は空 object と同じ扱いに寄せる。
- `content` と `toc` が文字列かを判定する。
- 戻り値は `{ safeData, missing, hasContractViolation }` とする。

この関数は HTML の安全性を検証しない。HTML の sanitization はサーバー側 renderer の責務であり、この関数は browser update payload の形だけを検証する。

### `logUpdatePayloadContractViolation(validation)`

契約違反時に既存と同等の warn を出す。

- ログ文言は既存 E2E が期待する `updateContent` の契約違反メッセージを維持する。
- 長大な HTML 本体はログに出さない。
- 出力するメタデータは `missing`、`file`、`contentLength`、`tocLength` に限定する。

### `normalizeTocHtml(html)`

現在 `content.js` にある TOC HTML 正規化を移す。`>\s+<` の差分を潰して、TOC の不要な再描画を避ける。

### `applySanitizedContentHtml(ctx, contentEl, content)`

`content` が文字列で、`ctx.state.lastAppliedContent` と異なる場合だけ `contentEl.innerHTML = content` を行う。反映した場合は `ctx.state.lastAppliedContent` を更新する。

この関数名は、入力 HTML がサーバー側で sanitize 済みである契約を明示するために `Sanitized` を含める。

### `applySanitizedTocHtml(tocEl, toc)`

`toc` が文字列で、正規化後に現在の `tocEl.innerHTML` と異なる場合だけ `tocEl.innerHTML = toc` を行う。

### `applyValidatedUpdateHtml(ctx, targets, validation)`

`content` と `toc` の DOM 反映をまとめる薄い関数にする。

想定する `targets` は `{ contentEl, tocEl }`。戻り値は `{ contentChanged, tocChanged }` とする。`updateContent` は初期実装では戻り値を副作用分岐に使わないが、後続の `createContentController(ctx, deps)` 化で副作用を条件分岐しやすくするために返す。

## `updateContent` Integration

`content.js` の `updateContent` は次の形に寄せる。

1. `validateUpdatePayload(data)` で payload の形を検証する。
2. 契約違反があれば `logUpdatePayloadContractViolation(validation)` を呼ぶ。
3. `pendingUpdate` と `pendingUpdateTimer` を既存通りクリアする。
4. scroll 位置、scroll mode、active TOC id を既存通り保存する。
5. `applyValidatedUpdateHtml(appContext, { contentEl, tocEl }, validation)` へ DOM 反映を委譲する。
6. `setupTocTracking()`、`suppressTocTrackingFor(120)`、`requestAnimationFrame(...)`、document stats、document chrome、content interactions、search sync、TOC filter、quote action 非表示を既存順で呼ぶ。
7. 契約違反がない場合だけ `appContext.websocket.rememberAppliedLiveUpdate(safeData)` を既存通り呼ぶ。

副作用順を変えないことを今回の重要な互換条件にする。

## Security Considerations

`content-renderer.js` は `innerHTML` 使用箇所を隠すための抽象ではなく、明示的な信頼境界として置く。

- `innerHTML` に渡せる値は、サーバー側 renderer が生成した sanitize 済み HTML だけとする。
- browser 側で外部入力やユーザー入力から HTML 文字列を組み立てて `applySanitizedContentHtml` に渡してはいけない。
- 契約違反ログには HTML 本体を含めない。ログ経由の情報漏えいと巨大ログ化を避ける。
- production の `window` へ `content-renderer` 内部関数を露出しない。
- `#content` / `#toc` への直接 `innerHTML` 代入を renderer 境界に集約し、将来の XSS レビュー対象を狭める。

## Tests And Verification

### E2E

既存 E2E を維持し、境界移動で不足が出る場合は同じ spec 内で補強する。

- `updateContent` の契約違反 warn が既存と同等に出る。
- 同一 `data.content` の 2 回目 `updateContent` は no-op cache により一時 DOM 状態を壊さない。
- `data.content` が変わると再描画される。
- production では `window.updateContent`、`window.markdownViewTestHooks`、`content-renderer` 内部関数が露出しない。
- E2E opt-in 時だけ `window.markdownViewTestHooks.updateContent` が使える。

必須の対象 E2E:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts tests/e2e/memo_jump.spec.ts
```

### Rust

`inline_script.rs` の include 追加で CSP/hash 生成が壊れないことを確認する。

```bash
cargo test --all-targets --all-features
```

### Static Checks

完了時に次を確認する。

```bash
rg "innerHTML\\s*=" src/template/assets/js
```

受け入れ基準は、`#content` / `#toc` への直接代入が `content-renderer.js` に閉じていること。検索結果や UI empty state のクリア用途など、別 DOM の意図的な代入が残る場合は個別に確認する。

最終検証は次を目標にする。

```bash
./verify.sh
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts tests/e2e/memo_jump.spec.ts
```

## TODO Update

実装時に `docs/todo/TODO.md` のブラウザ JS 項目を更新する。

- 今回完了する範囲: `content-renderer` 境界、payload 契約、`innerHTML` 反映集約。
- 後続に残す範囲: document search / directory search 分割、navigation / link-resolution 分割、`createContentController(ctx, deps)` 化。

これにより、TODO が「ブラウザ JS を分割する」という大項目のまま残るのではなく、完了済みの安全境界と次に切る責務を区別できる。

## Acceptance Criteria

- `src/template/assets/js/content-renderer.js` が追加される。
- `src/template/assets/inline_script.rs` が `content.js` より前に `content-renderer.js` を include する。
- `updateContent` の payload 検証、契約違反 warn、`#content` / `#toc` HTML 反映、TOC HTML 正規化が `content-renderer.js` に移る。
- `updateContent` の外部挙動、E2E hook、production 非露出契約が維持される。
- 契約違反 warn は HTML 本体を出さず、既存と同等のメタデータだけを出す。
- 同一 `data.content` の no-op cache が維持される。
- `docs/todo/TODO.md` のブラウザ JS 項目が、今回完了範囲と後続範囲に整理される。
- `./verify.sh` と対象 E2E の結果が completion report に記録される。

## Rollback

rollback は小さく保つ。

1. `inline_script.rs` から `content-renderer.js` の include を外す。
2. `content.js` に `normalizeTocHtml`、payload 検証、warn、`innerHTML` 代入を戻す。
3. `content-renderer.js` を削除する。

サーバー API、renderer、CSP ポリシーの意味を変えないため、rollback はブラウザ JS と include 順の範囲に閉じる。
