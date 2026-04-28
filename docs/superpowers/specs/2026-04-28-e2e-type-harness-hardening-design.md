# E2E 型宣言・WebSocket ハーネス整理設計

**作成日**: 2026-04-28
**対象**: `tests/e2e`, `tsconfig.json`, `docs/todo/BACKLOG.md`

## 目的

E2E テストのブラウザ側グローバル型と WebSocket テストハーネスを共通化し、strict flag 追加に耐える型基盤へ寄せる。

現在は `declare global` が複数 spec に散在し、`window.updateContent` の型も spec 間で揺れている。`TestWebSocket` の差し替えも `document_search.spec.ts` と `text_selection_defer.spec.ts` に重複しており、片方だけが `setTimeout` の短縮処理を持つなど drift しやすい。

今回の変更は E2E テスト基盤に限定し、production の Rust コードやブラウザ JS の動作は変えない。

## 非ゴール

- DOM cleanup 戦略の見直しはしない。
- 汎用 `tests/e2e/helpers.ts` 抽出はしない。
- TOC ナビゲーション回帰テストは追加しない。
- `verify.sh` への E2E 実行統合はしない。
- Rust 本体、production bundle、WebSocket の Origin/Host 検証は変更しない。

## 対象バックログ項目

完了対象:

- E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約
- `updateContent` 型宣言の統一
- `tsconfig.json` に strict flag 追加
- `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出
- `as unknown as` double-cast の説明コメント追加

対象外として残す項目:

- E2E テストの DOM クリーンアップ戦略見直し
- E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出
- E2E を `verify.sh` に統合するか検討

## 型宣言設計

`tests/e2e/globals.d.ts` を追加し、E2E spec で使う ambient 宣言の SSoT にする。各 spec の `declare global` は削除し、必要な型は共通宣言から参照する。

共通型は次の粒度にする。

- `UpdateContentPayload`: `content: string` と `toc: string` を必須にする。
- `UpdateContentOptions`: 現状の `Record<string, unknown>` を維持する。
- `Window.updateContent`: `(data: UpdateContentPayload, opts?: UpdateContentOptions) => void`
- `Window.scheduleBufferedLiveUpdate`: `(data: UpdateContentPayload) => void`
- E2E hook: `__MV_E2E__?: boolean`, `__lastWs?`, `__realWsOnmessage?`, `__dispatchWsMessage?` などをまとめる。

`UpdateContentPayload` は正常系のサーバ契約を表す型として狭く保つ。契約違反テストで `{}`、`null`、`undefined` などを渡す箇所だけ `as unknown as UpdateContentPayload` を使い、正常系型を広げない。

`exactOptionalPropertyTypes` 追加後も `toc?: string` と `toc: undefined` を混同しないよう、欠落ケースは `Partial<UpdateContentPayload>` または明示 cast に限定する。

## WebSocket ハーネス設計

`tests/e2e/browser/test-websocket.ts` を追加し、ブラウザへ注入する WebSocket 差し替えロジックを 1 箇所に集約する。

基本 API:

```ts
installTestWebSocketHarness(options?: { shorten30sTimeouts?: boolean }): void
```

責務:

- `window.WebSocket` を `NativeWebSocket` 継承クラスに差し替える。
- 最後に生成された socket を `window.__lastWs` に保存する。
- `shorten30sTimeouts` が `true` の場合だけ、`setTimeout(30000)` を `50` に短縮する。WebSocket reconnect だけでなく、選択中更新の30秒フォールバックも対象になる。

`document_search.spec.ts` は `shorten30sTimeouts` なしで使う。`text_selection_defer.spec.ts` は既存の30秒待機短縮を維持するため `shorten30sTimeouts: true` で使う。

Playwright の `page.addInitScript({ path })` で TypeScript ファイルをそのまま読み込めない場合は、実装計画で Node 側 helper へ切り替える。ただし設計上の固定点は、WebSocket 差し替えロジックを SSoT 化し、spec ごとのインライン重複をなくすこととする。

`stabilizeWebSocketHarness(page)` は今回は大規模 helper 抽出の対象にしない。各 spec に残す場合でも、`__realWsOnmessage` に保存する直前へ日本語コメントを追加し、`__dispatchWsMessage` が `MessageEvent` ではなく `{ data: string }` を直接渡すテスト契約であることを明記する。

## strict flag 設計

`tsconfig.json` に次を追加する。

```json
"noUncheckedIndexedAccess": true,
"exactOptionalPropertyTypes": true
```

追加 flag によって `tests/e2e` 内で型エラーが出た場合は、今回の計画内で修正する。修正方針は次の通り。

- `Record` アクセスは missing key を想定し、存在確認または明示的な失敗に寄せる。
- optional property は `undefined` 代入で欠落を表現せず、プロパティ自体を省く。
- 契約違反を意図するテストだけ明示 cast を残す。
- 挙動変更や大規模 helper 抽出が必要になった場合は、別計画へ分離する。

## データフロー

1. Playwright spec が `page.addInitScript` で E2E hook と WebSocket ハーネスを注入する。
2. ブラウザ側で作成された最後の WebSocket が `window.__lastWs` に保存される。
3. spec の `stabilizeWebSocketHarness(page)` が本物の `onmessage` を `window.__realWsOnmessage` に退避する。
4. テストが必要に応じて `window.__dispatchWsMessage(payload)` から JSON 文字列の `{ data }` を渡す。
5. `window.updateContent` と `scheduleBufferedLiveUpdate` は `UpdateContentPayload` を正常系契約として受け取る。

## エラー処理

`__lastWs` や `__realWsOnmessage` は初期化前に存在しない可能性があるため、ambient 型では optional にする。利用側は `page.waitForFunction` や存在チェックを通してから使う。

strict flag 追加で検出された missing key や optional property の曖昧さは、型を緩めて黙殺せず、テスト前提の確認または明示的なエラーに寄せる。

## セキュリティ考慮

このハーネスは E2E 実行時に `page.addInitScript` で注入されるテストコードであり、production bundle には含めない。`window.updateContent` の E2E 限定 expose 条件も変更しない。

外部レビュー由来の backlog 記述は実装済み事実や現在の脆弱性として扱わない。今回の整理はテスト基盤の型安全性と保守性を上げるものであり、Host/Origin 検証、HTML sanitization、CSP、path validation などの production セキュリティ境界は強化しない。

## 受け入れ基準

- `tests/e2e/globals.d.ts` に ambient 宣言が集約されている。
- 対象 spec から重複した `declare global` が削除されている。
- `window.updateContent` と `scheduleBufferedLiveUpdate` の payload 型が共通化されている。
- WebSocket 差し替えロジックが `tests/e2e/browser/test-websocket.ts` または同等の単一 helper に集約されている。
- `shorten30sTimeouts` 相当の既存挙動が `text_selection_defer.spec.ts` で維持されている。
- `tsconfig.json` に `noUncheckedIndexedAccess` と `exactOptionalPropertyTypes` が追加されている。
- `as unknown as` double-cast の直前に、日本語の契約説明コメントがある。
- 対象 backlog 項目が完了状態へ更新されている。

## 検証

必須:

```bash
npm run typecheck
npx playwright test tests/e2e/document_search.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/update_content_exposure.spec.ts tests/e2e/markdown_links.spec.ts
```

可能なら実行:

```bash
./verify.sh
```

## 影響範囲

変更対象:

- `tests/e2e/globals.d.ts`
- `tests/e2e/browser/test-websocket.ts` または同等の helper
- `tests/e2e/document_search.spec.ts`
- `tests/e2e/text_selection_defer.spec.ts`
- `tests/e2e/memo_jump.spec.ts`
- `tests/e2e/update_content_exposure.spec.ts`
- `tests/e2e/markdown_links.spec.ts`
- `tsconfig.json`
- `docs/todo/BACKLOG.md`

依存影響:

- `npm run typecheck` は `tests/e2e/**/*.ts` 全体を見るため、対象外 spec の型エラーが顕在化する可能性がある。
- Playwright の init script 読み込み方式により、WebSocket helper の実装形態が変わる可能性がある。

## ロールバック

docs-only の設計段階は、この設計書コミットを revert すれば戻せる。

実装後に戻す場合は、追加した `globals.d.ts` と WebSocket ハーネス、各 spec の import・宣言削除、`tsconfig.json` の strict flag、`BACKLOG.md` のチェック変更を revert する。production コードには触れないため、動作面のロールバック範囲は E2E テスト基盤に閉じる。
