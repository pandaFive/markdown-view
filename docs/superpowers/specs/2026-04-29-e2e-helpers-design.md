# E2E 共通ヘルパー抽出設計

**作成日**: 2026-04-29
**対象**: `tests/e2e/helpers.ts`, `tests/e2e/*.spec.ts`, `docs/todo/BACKLOG.md`

## 目的

E2E spec に散在している近い helper を `tests/e2e/helpers.ts` に抽出し、DRY 違反と片側修正漏れのリスクを下げる。

現在は `resetFixtures`、`selectParagraphText`、`stabilizeWebSocketHarness`、`requireUpdateContent` 相当の処理が複数 spec に残っている。`tests/e2e/browser/test-websocket.ts` と `tests/e2e/globals.d.ts` は既に整理済みだが、spec-local helper の重複はまだ残っている。

今回の変更は E2E テスト基盤の保守性改善に限定し、production の Rust コードやブラウザ JS の挙動は変えない。

## 非ゴール

- アプリ本体の挙動は変更しない。
- Playwright 設定や `verify.sh` の実行範囲は変更しない。
- 新しい E2E シナリオは追加しない。
- spec 固有の fixture HTML、長文 fixture、assertion までは共通化しない。
- `tests/e2e/helpers/` のような複数ファイル階層は作らない。
- `tests/e2e/browser/test-websocket.ts` の WebSocket 差し替え責務は増やさない。

## 対象バックログ項目

完了対象:

- E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出

主な対象 spec:

- `tests/e2e/memo_quote.spec.ts`
- `tests/e2e/memo_sync.spec.ts`
- `tests/e2e/memo_jump.spec.ts`
- `tests/e2e/markdown_links.spec.ts`
- `tests/e2e/text_selection_defer.spec.ts`
- `tests/e2e/document_search.spec.ts`

## ヘルパー設計

`tests/e2e/helpers.ts` は名前付き関数 export のみで構成する。namespace object や class は使わず、既存 spec の素朴な関数呼び出しに近い形を維持する。

想定する helper は次の粒度にする。

- fixture: `resetStandardFixtures`
- selection: `selectParagraphText`, `clearSelection`
- browser harness: `stabilizeWebSocketHarness`, `dispatchWsMessage`, `updateContent`
- TOC: `activeTocLabel`, `activeTocLabelOrEmpty`, `clickTocLink`, `waitForTocTrackingFrame`, `startTocActiveChangeRecorder`, `stopTocActiveChangeRecorder`
- common UI: `openMemoTab`, `openFileTab`, `selectFile`, `saveMemo`

`resetStandardFixtures` は `README.md` と `notes.md` の標準内容を書き戻し、必要な spec では memo sidecar と `.markdown-view` も削除できる option を持たせる。`memo_jump.spec.ts` の `long.md` 生成のようなテスト固有 fixture は spec 側に残す。

`updateContent(page, payload, opts?)` は各 spec の `page.evaluate` 内に重複している `requireUpdateContent` を隠蔽する。`window.updateContent` が E2E 用に expose されていない場合は、helper 側で明示エラーにする。

`dispatchWsMessage(page, payload)` は `stabilizeWebSocketHarness(page)` 後の `window.__dispatchWsMessage` を使う薄い wrapper にする。`tests/e2e/browser/test-websocket.ts` は WebSocket 差し替えの SSoT として維持し、message dispatch のテスト都合だけを `helpers.ts` が受け持つ。

## データフロー

1. spec が `helpers.ts` から必要な関数だけ import する。
2. fixture helper が `tests/fixtures/e2e` の標準 fixture を初期化する。
3. browser harness helper は Playwright の `page.evaluate` 経由で、E2E expose 済みの browser global を検査してから呼び出す。
4. TOC と selection helper は DOM 操作を `page.evaluate` 内に閉じ、呼び出し元 spec にはテスト意図と assertion を残す。
5. spec 固有の fixture content、長文 document、検索 fixture HTML は各 spec に残す。

## エラー処理

helper は前提が満たされない場合に fail fast する。

- 対象テキストが見つからない場合は `text not found: ...` で失敗する。
- TOC link が見つからない場合は対象 id を含むエラーで失敗する。
- `window.updateContent` が expose されていない場合は E2E expose 不足を明示する。
- WebSocket harness が未初期化の場合は dispatcher または handler の未初期化を明示する。

型を広げて失敗を遅らせず、`tests/e2e/globals.d.ts` の既存契約に沿って利用側で存在確認する。

## セキュリティ考慮

今回の helper は E2E テストコードに限定し、production bundle には含めない。`window.updateContent` の E2E 限定 expose 条件を緩めず、helper は expose 済みかどうかを検査するだけにする。

fixture として HTML 文字列や Markdown を扱う場合も、外部入力を信頼する設計にはしない。Host/Origin 検証、HTML sanitization、CSP、path validation、file-size 制限などの production セキュリティ境界は変更しない。

## 受け入れ基準

- `tests/e2e/helpers.ts` が追加され、重複していた helper が名前付き export として集約されている。
- 対象 spec は必要な helper だけを import し、テスト意図に近い fixture content と assertion は spec 側に残っている。
- `resetFixtures`、`selectParagraphText`、`stabilizeWebSocketHarness`、`requireUpdateContent` 相当の重複が削減されている。
- `openMemoTab`、`openFileTab`、`selectFile`、`saveMemo`、TOC active recorder 系も、再利用できる範囲で共通化されている。
- E2E expose 条件や WebSocket harness の production 非混入方針が維持されている。
- 対象 backlog 項目が完了状態へ更新されている。

## 検証

必須:

```bash
npm run typecheck
npx playwright test tests/e2e/memo_quote.spec.ts tests/e2e/memo_sync.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/markdown_links.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/document_search.spec.ts
```

可能なら実行:

```bash
./verify.sh
```

## 影響範囲

変更対象:

- `tests/e2e/helpers.ts`
- `tests/e2e/memo_quote.spec.ts`
- `tests/e2e/memo_sync.spec.ts`
- `tests/e2e/memo_jump.spec.ts`
- `tests/e2e/markdown_links.spec.ts`
- `tests/e2e/text_selection_defer.spec.ts`
- `tests/e2e/document_search.spec.ts`
- `docs/todo/BACKLOG.md`

依存するが原則変更しないファイル:

- `tests/e2e/globals.d.ts`
- `tests/e2e/browser/test-websocket.ts`
- `tests/fixtures/e2e/README.md`
- `tests/fixtures/e2e/notes.md`

## ロールバック

docs-only の設計段階は、この設計書コミットを revert すれば戻せる。

実装後に戻す場合は、追加した `tests/e2e/helpers.ts`、各 spec の import と helper 呼び出し変更、`BACKLOG.md` の完了更新を revert する。production コードには触れないため、動作面のロールバック範囲は E2E テスト基盤に閉じる。
