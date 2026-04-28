# updateContent の E2E 限定 expose 化

- 対象: `src/template/assets/js/content.js` / `tests/e2e/memo_jump.spec.ts` / `tests/e2e/document_search.spec.ts`
- 関連 backlog: `docs/todo/BACKLOG.md` の「`window.updateContent` を E2E モード限定 expose に変更」
- 優先理由: production HTML にテスト専用 hook が常時露出しているため、将来の公開運用や OSS 化に備えて公開面を縮小する

## 1. ゴール

`window.updateContent` を通常実行時には公開せず、E2E 実行時だけ明示フラグで公開する。

受け入れ条件:

- 通常ブラウザ実行では `typeof window.updateContent === 'undefined'`
- `window.__MV_E2E__ === true` を `content.js` 読み込み前に注入した場合だけ `window.updateContent` が関数として公開される
- WebSocket 更新、HTTP fetch 更新、buffered live update の正規経路は挙動を変えない
- `tests/e2e/memo_jump.spec.ts` と `tests/e2e/document_search.spec.ts` の既存 direct-call テストは通る
- production 非露出を明示する回帰テストを追加する

## 2. 非ゴール

- `updateContent` の入力契約や描画ロジックの変更
- WebSocket / HTTP 更新経路の再設計
- E2E 共通 helper 抽出
- `declare global` や `updateContent` 型宣言の統一
- `TestWebSocket` の共有化

上記は backlog に別項目として残っているため、本 spec では巻き取らない。

## 3. 採用方針

`src/template/assets/js/content.js` の末尾を、無条件 expose から E2E フラグ付き expose に変更する。

```js
if (window.__MV_E2E__ === true) {
  window.updateContent = updateContent;
}
```

E2E 側では、`window.updateContent` を直接呼ぶ spec だけが `page.goto()` より前に次の初期化を行う。

```ts
await page.addInitScript(() => {
  window.__MV_E2E__ = true;
});
```

対象 spec は現時点で `memo_jump.spec.ts` と `document_search.spec.ts`。`websocket.js` と `fetch.js` は同じブラウザスクリプト束内の `updateContent` 関数参照を使うため、`window` への公開有無には依存しない。

型変更は最小限に留める。必要な spec の `Window` interface に `__MV_E2E__?: boolean` を追加するが、`updateContent` の宣言形式統一は行わない。

## 4. 代替案

### 4.1 正規経路のみで E2E を書く

WebSocket または HTTP fetch 経路だけで状態を刺激し、`window.updateContent` の direct-call を完全にやめる案。

本番に近いが、文書検索やメモジャンプの細かい DOM 状態を作るための準備が重くなり、テスト速度と安定性に影響する。今回の目的は露出面の縮小であり、direct-call テスト全廃は過剰。

### 4.2 テスト hook 名前空間を導入する

`window.__MV_TEST_HOOKS__ = { updateContent }` のように名前空間化する案。

将来 hook が増える場合は整理しやすいが、production に名前空間を出すかどうかの gate が別途必要になる。現時点では `updateContent` だけが対象なので、`__MV_E2E__` gate の方が小さい。

## 5. セキュリティ考慮

この変更は XSS 対策そのものではなく、テスト専用 API の外部公開をやめる防御的な公開面縮小である。

既存の HTML サニタイズ、CSP、Host / Origin 検証、localhost-only 前提は変更しない。`updateContent` はサーバーが生成したサニタイズ済み HTML を反映する内部関数なので、production の `window` から消すことで、将来公開運用に寄せた場合の「サニタイズ済み経路を迂回して任意 HTML を流し込む direct-call 面」を減らす。

E2E フラグはブラウザ内のテスト実行時だけに立てる。アプリ本体は `__MV_E2E__` を生成しないため、通常配信 HTML だけでは hook は公開されない。

## 6. テスト方針

### 6.1 既存 E2E の維持

`memo_jump.spec.ts` と `document_search.spec.ts` の `beforeEach` で、`page.goto()` 前に `window.__MV_E2E__ = true` を注入する。これにより既存の `window.updateContent` direct-call テストを維持する。

### 6.2 production 非露出の回帰テスト

`__MV_E2E__` を注入しない独立 E2E テストを追加し、通常ロード後に `typeof window.updateContent === 'undefined'` を検証する。

このテストは、E2E 用フラグを常時立てる fixture や helper と混ざらない位置に置く。専用 spec に切り出すか、フラグ未使用の spec に追加する。

### 6.3 検証コマンド

実装後に最低限次を実行する。

- `npx tsc --noEmit`
- `npm run test:e2e`、または変更対象 spec と非露出 spec の Playwright 実行
- `./verify.sh`

`./verify.sh` が E2E 実行まで含まない場合は、E2E 結果を別枠で報告する。

## 7. 影響範囲

直接変更する想定:

- `src/template/assets/js/content.js`
- `tests/e2e/memo_jump.spec.ts`
- `tests/e2e/document_search.spec.ts`
- production 非露出を検証する E2E spec

依存影響:

- `src/template/assets/js/websocket.js`: 同一 bundle scope の `updateContent` を呼ぶため挙動不変
- `src/template/assets/js/fetch.js`: 同一 bundle scope の `updateContent` を呼ぶため挙動不変
- TypeScript ambient 宣言: `__MV_E2E__` だけ追加。既存の `updateContent` 宣言統一は行わない

## 8. リスクと対策

| リスク | 影響 | 対策 |
|--------|------|------|
| `page.addInitScript` が `page.goto()` より後に実行される | E2E で `window.updateContent` が未定義になる | direct-call spec の `beforeEach` で最初に注入する |
| 将来 direct-call を使う spec がフラグ注入を忘れる | 該当 E2E が `window.updateContent is not a function` で失敗する | 失敗が明示的なので、必要な spec だけに注入を追加する |
| production 非露出テストが E2E フラグ付き fixture と混ざる | 非露出検証が偽陰性になる | フラグを注入しない専用 spec か独立 test setup に置く |

## 9. ロールバック

`content.js` の条件付き expose を元の無条件 `window.updateContent = updateContent;` に戻す。あわせて E2E の `__MV_E2E__` 注入と production 非露出テストを削除する。

永続データやユーザー設定には影響しないため、ロールバックはコード差分の revert だけで完了する。
