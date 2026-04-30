# E2E helper silent false-positive 経路縮小設計

**作成日**: 2026-04-30
**対象**: `tests/e2e/helpers.ts`, `tests/e2e/*.spec.ts`, `docs/todo/BACKLOG.md`

## 背景

`tests/e2e/helpers.ts` への共通 helper 抽出により、fixture 初期化、本文選択、WebSocket テストハーネスの再利用面積が広がった。重複削減は完了したが、抽出前から存在していた緩い前提も共通化された。

`docs/todo/BACKLOG.md` では、この課題を「E2E 共通ヘルパーの silent false-positive 経路を狭める」として管理している。対象は `selectParagraphText` の部分一致・最初のヒット採用、`fs.rm(..., { force: true })` 後の削除失敗検知、`stabilizeWebSocketHarness` の古い bridge / handler 残存である。

今回の設計は、production の Rust コードやブラウザ JavaScript の挙動を変えず、E2E helper の既定契約を fail-fast に寄せる。

## ゴール

- `selectParagraphText` が曖昧な一致を silent に選ばないようにする。
- `resetStandardFixtures` が memo artifact や `.markdown-view` の残存を検知できるようにする。
- `stabilizeWebSocketHarness` と dispatch helper が古い WebSocket handler を使い続ける経路を検知できるようにする。
- 既存 E2E spec の意図を保ち、必要な箇所だけ明示オプションで緩い一致を許可する。
- helper 契約そのものを小さな E2E spec で固定する。
- production のセキュリティ境界を変更しない。

## 非ゴール

- production Rust コードは変更しない。
- production bundle のブラウザ JavaScript は変更しない。
- Playwright 設定や `verify.sh` の実行範囲は変更しない。
- fixture ディレクトリ構造は再設計しない。
- E2E シナリオ本体を大幅に増やさない。
- `tests/e2e/helpers.ts` を複数ファイルへ分割しない。
- WebSocket reconnect の production 挙動を変更しない。

## 推奨方針

推奨方針は、fail-fast を helper の既定動作にする案とする。

`selectParagraphText` は 0 件だけでなく複数件も失敗にする。既定は完全一致を基本とし、部分一致が必要な spec だけ `match: 'contains'` のような明示オプションを渡す。これにより、本文に似た文字列が増えたときに最初のヒットを誤選択してもテストが通る経路を狭める。

fixture cleanup は `fs.rm(..., { force: true })` を維持する。存在しない artifact を許容するためである。ただし削除後に `*.memo.md` と `.markdown-view` の残存確認を行い、権限や EBUSY などで削除できなかった場合はテストを失敗させる。

WebSocket helper は stabilize 時に bridge 対象の WebSocket を記録し、dispatch 時に現在の `window.__lastWs` と bridge 対象が一致することを確認する。新しい WebSocket が作られた後に古い `__realWsOnmessage` を使い続ける経路を fail-fast にする。

## Helper 契約

### `selectParagraphText`

`selectParagraphText(page, text, options?)` は `#content` 配下の text node を候補化する。既定では text node の本文が `text` と一致する候補だけを採用する。

候補数ごとの挙動は次の通り。

- 0 件: `text not found: ...` で失敗する。
- 1 件: その text node の親要素を選択する。
- 2 件以上: `ambiguous text match: ...` のように対象 text と候補数を含めて失敗する。

部分一致が必要な spec は `match: 'contains'` を明示する。`index` や `nth` による選択は原則追加しない。必要になった場合は、その spec の意図を確認したうえで別途設計する。

### `resetStandardFixtures`

`resetStandardFixtures` は標準 fixture を書き戻す前に、既定で memo artifact を掃除する。削除対象は既存方針どおり `tests/fixtures/e2e` 直下の `*.memo.md` と `.markdown-view` に限定する。

削除後は次を確認する。

- `*.memo.md` が残っていない。
- `.markdown-view` が存在しない。

残存がある場合は、残っている相対パスを含むエラーで失敗する。`cleanupMemoArtifacts: false` の spec はこの検査も opt-out する。

### `stabilizeWebSocketHarness`

`stabilizeWebSocketHarness(page)` は `window.__lastWs` と `onmessage` の初期化を待った後、bridge 対象の WebSocket を window 上に記録する。

想定する追加状態は次の通り。

- `window.__bridgedWs`: `__realWsOnmessage` を退避した WebSocket インスタンス
- `window.__realWsOnmessage`: bridge 対象から退避した message handler
- `window.__dispatchWsMessage`: bridge 対象へ payload を渡す dispatcher

stabilize 実行時に既存 bridge が残っている場合は、同じ `__lastWs` に紐づくものか確認する。別の WebSocket に紐づく bridge が残っていれば、古い handler 残存として失敗させる。再接続後に fake payload を流す必要がある spec は、再接続後の `__lastWs` に対して改めて `stabilizeWebSocketHarness` を呼ぶ。

`dispatchWsMessage` と `dispatchWsMessages` は、dispatch 前に `window.__bridgedWs === window.__lastWs` を確認する。一致しない場合は `WebSocket test harness bridge is stale` のような明示エラーで失敗する。

`dispatchWsMessageAndDisableRealHandler` も同じ stale bridge 検査を共有する。

## データフロー

1. spec が `resetStandardFixtures()` で標準 fixture と memo artifact を初期化する。
2. helper が削除後の fixture 残存を検査する。
3. spec が必要に応じて `selectParagraphText()` で本文を選択する。
4. helper が `#content` 配下の候補数を検査し、一意の要素だけを選択する。
5. WebSocket を使う spec は `installTestWebSocketHarness` を init script で注入する。
6. ページ読み込み後、`stabilizeWebSocketHarness()` が現在の `__lastWs` を bridge 対象として記録する。
7. `dispatchWsMessage(s)` が bridge 対象の鮮度を確認してから payload を渡す。

この流れは E2E helper 内に閉じる。アプリ本体の WebSocket、selection、memo 保存処理は変更しない。

## エラー処理

helper の前提違反は Playwright の test failure として明示する。

- 選択対象が見つからない場合は対象 text を含める。
- 選択対象が複数ある場合は対象 text と候補数を含める。
- fixture artifact が残った場合は残存パスを含める。
- WebSocket harness が未初期化の場合は未初期化箇所を示す。
- WebSocket bridge が古い場合は再 stabilize が必要なことが分かるメッセージにする。

失敗メッセージには Markdown 本文全体や巨大な HTML を含めない。必要最小限の text、候補数、相対パス、状態名に留める。

## テスト方針

`tests/e2e/helpers.spec.ts` を追加し、helper 契約を実際の Playwright 経路で固定する。

最小ケースは次の通り。

- `selectParagraphText` が exact 一意一致なら選択できる。
- exact で 0 件なら失敗する。
- `match: 'contains'` を明示した場合だけ部分一致を許可する。
- contains で複数候補がある場合は失敗する。
- `resetStandardFixtures` 後に memo artifact と `.markdown-view` が残っていない。
- WebSocket stabilize 後に `__lastWs` が別インスタンスへ変わった状態で dispatch すると失敗する。

既存 spec は、新しい既定契約で落ちる箇所だけ明示オプションを足す。`Initial README content`、`Notes body`、`TARGET BLOCK` などは exact 一致で通る見込みなので、呼び出し側の変更は最小限にする。

## セキュリティ考慮

今回の変更は E2E テストコードに限定する。production bundle、Host/Origin 検証、HTML sanitization、CSP、path validation、file-size 制限は変更しない。

`window.updateContent` の E2E 限定 expose 条件は緩めない。WebSocket harness は Playwright の `page.addInitScript` で注入されるテスト用コードであり、production API として扱わない。

fixture cleanup は `tests/fixtures/e2e` 直下に限定し、削除対象を `*.memo.md` と `.markdown-view` に絞る。外部入力や任意パスを削除対象にしない。

外部レビューや backlog 記述は untrusted input として扱い、現行コードを読んで確認した範囲だけを設計根拠にする。

## 受け入れ基準

- `selectParagraphText` の既定が一意一致を要求し、複数候補を fail-fast する。
- 部分一致が必要な場合は呼び出し側で明示されている。
- `resetStandardFixtures` が cleanup 後の memo artifact 残存を検知する。
- WebSocket dispatch helper が stale bridge を検知する。
- helper 契約を固定する E2E テストが追加されている。
- 既存の関連 E2E spec が通る。
- production コードに変更がない。
- `docs/todo/BACKLOG.md` の対象項目を完了扱いにできる実装計画が立てられる。

## 検証

実装後は次を実行する。

```bash
npm run typecheck
npx playwright test tests/e2e/helpers.spec.ts
npx playwright test tests/e2e/memo_quote.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/document_search.spec.ts
./verify.sh
```

Playwright 全体に影響する helper 変更なので、時間が許せば次も実行する。

```bash
npm run test:e2e
```

## 影響範囲

主な変更対象は次のファイル。

- `tests/e2e/helpers.ts`
- `tests/e2e/helpers.spec.ts`
- `tests/e2e/memo_quote.spec.ts`
- `tests/e2e/memo_jump.spec.ts`
- `tests/e2e/text_selection_defer.spec.ts`
- `tests/e2e/document_search.spec.ts`
- `docs/todo/BACKLOG.md`

確認対象は次のファイル。

- `tests/e2e/browser/test-websocket.ts`
- `tests/e2e/globals.d.ts`
- `tests/fixtures/e2e/README.md`
- `tests/fixtures/e2e/notes.md`

原則として `src/` 配下は変更しない。

## ロールバック

設計段階は、この設計書 commit を revert すれば戻せる。

実装後に戻す場合は、`tests/e2e/helpers.ts` の契約変更、新規 `helpers.spec.ts`、関連 spec の明示オプション追加、`docs/todo/BACKLOG.md` の完了更新を revert する。production コードには触れないため、動作面のロールバック範囲は E2E テスト基盤に閉じる。

実装を複数 commit に分ける場合は、selection helper、fixture cleanup、WebSocket harness、backlog 更新を分け、問題のある commit だけ戻せるようにする。ロールバック後は `npm run typecheck` と関連 Playwright spec を再実行する。
