# TOC pending navigation 境界回帰テスト設計

**作成日**: 2026-04-28
**対象 backlog**: `docs/todo/BACKLOG.md` の `TOC pending navigation の境界回帰テスト強化`

## 目的

TOC pending navigation の未固定境界を E2E で回帰検知できるようにする。

現在の `src/template/assets/js/sidebar.js` は、目次クリック直後に `pendingTocNavigationId` を立て、`TOC_NAVIGATION_GRACE_MS = 400` の間はクリック先の active を優先する。クリック先見出しが activation line から `TOC_NAVIGATION_SLACK_PX = 24` 以内にある場合は pending を維持し、帯外に出た場合は通常の viewport 判定へ戻る。

既存の `tests/e2e/text_selection_defer.spec.ts` は、目次クリック直後の active、下端見出し、日本語 ID、逆方向スクロール、burst 更新時の点滅を検証している。一方、backlog に残る以下の 3 観点は直接固定されていない。

- 400ms grace 内の連続 TOC クリックで、最後のクリック先へ収束すること。
- `TOC_NAVIGATION_SLACK_PX` の内側と外側で pending 維持と通常判定復帰が分かれること。
- pending 中の小揺らしで `#toc a.active` がクリック先以外へ一瞬切り替わらないこと。

## 非ゴール

- TOC tracking の設計を変更しない。
- `sidebar.js` を予防的にリファクタしない。
- `tests/e2e/helpers.ts` への共通ヘルパー抽出は行わない。
- Playwright 設定や E2E fixture 全体の構成は変更しない。
- テスト用の新しい production API 露出を追加しない。
- Rust 側の unit / integration test は追加しない。

## 方針

第一方針は、`tests/e2e/text_selection_defer.spec.ts` に黒箱 E2E テストを追加することとする。

テストは既存の `loadDenseHeadingFixture`、`clickTocLink`、`activeTocLabel` を使う。`getPendingTocNavigationId` などの内部関数を直接呼び出せるようにはしない。ユーザー操作、scroll 位置、`#toc a.active` の DOM 状態から pending navigation の契約を検証する。

`src/template/assets/js/sidebar.js` は、追加テストが失敗して現行実装の不備が確認できた場合だけ最小修正する。修正する場合も、対象は `pendingTocNavigationId` の更新・解除条件に限定し、URL restore、DOM 構造、CSP、HTML sanitization には触れない。

## テスト設計

### 1. 連続 TOC クリック

テスト名は `目次クリックの猶予中に別の目次をクリックしたら最後のクリック先へ収束する` とする。

`loadDenseHeadingFixture` で Alpha / Beta を含むページを作る。最初に Beta の少し手前へスクロールし、通常判定では `Alpha` が active になる状態を確認する。その後、400ms grace 内であることをテスト側で暗黙にしないため、同一 `page.evaluate` 内で `alpha.click()`、続けて `beta.click()` を実行する。

期待値は、最終 active が `Beta` になり、`window.scrollY` が `betaTop - activationOffset` 近辺へ収束することとする。scrollY の検証はブラウザの丸めや scroll-margin の差を考慮し、厳密一致ではなく小さな許容幅を持たせる。

### 2. slack 境界

テスト名は `目次クリック後のslack内スクロールではpending activeを維持し、slack外では通常判定へ戻る` とする。

`clickTocLink(page, 'beta')` 後、`activationOffset` が正値であることを確認し、Beta 見出しの viewport top が `activationOffset + 22` 相当になるよう `window.scrollTo` する。`22px` は `TOC_NAVIGATION_SLACK_PX - 2` に相当する。この状態では pending が維持され、active は `Beta` のままであることを確認する。

次に Beta 見出しの viewport top が `activationOffset + 26` 相当になるよう `window.scrollTo` する。`26px` は `TOC_NAVIGATION_SLACK_PX + 2` に相当する。この状態では pending が解除され、通常の viewport 判定へ戻る。Beta は activation line より下にあるため、直前の見出しである `Alpha` が active になることを確認する。

このテストでは `TOC_NAVIGATION_SLACK_PX` の値を production から読み取らない。backlog に明記された現行契約である 24px の内側 / 外側を E2E として固定する。

### 3. 小揺らし中の active 遷移監視

テスト名は `目次クリック直後の小揺らし中にactiveがBeta以外へ遷移しない` とする。

既存の `同一見出しのburst更新でも目次activeが点滅しない` と同じ `MutationObserver` 方式で `#toc a.active` の `class` 変化を記録する。`clickTocLink(page, 'beta')` 後、grace 内に `+6`, `-4`, `+3` 程度の小さな `scrollTo` を挟み、scroll イベントと `requestAnimationFrame` が処理される短い待ち時間を置く。

期待値は、記録された active ラベルが `Beta` 以外を含まないこととする。実装では `Beta` クリック後に observer を開始し、初期記録として `Beta` が含まれることも確認する。重複した `Beta` 記録は許容するが、`Alpha` や空文字への一瞬の切り替わりは失敗にする。

## 実装修正が必要な場合

追加テストが失敗した場合だけ、`src/template/assets/js/sidebar.js` を最小修正する。

想定する修正範囲は以下に限定する。

- `markPendingTocNavigation` が連続クリック時に最後の id と期限で上書きされること。
- `getPendingTocNavigationId` が slack 内では pending id を返し、slack 外では pending を clear すること。
- `setActiveTocLink` が余計な中間 active を作らないこと。

既存の `tryDecodeHash`、`popstate` ガード、line-range restore、file tree、tab 切り替え処理は変更しない。

## 受け入れ基準

- 400ms grace 内の `alpha` -> `beta` 連続 TOC クリックで、最終 active が `Beta` になり、scrollY も `Beta` 位置へ収束する。
- pending 中に Beta 見出し位置から `SLACK - 2 = 22px` 相当ずれても `Beta` active が維持される。
- pending 中に `SLACK + 2 = 26px` 相当ずれたら通常判定へ戻り、実 viewport に合う `Alpha` active へ切り替わる。
- `MutationObserver` で `#toc a.active` の `class` 変化を記録し、小揺らし中に `Beta` 以外へ切り替わらないことを検証する。
- 新しい production API 露出を追加しない。
- 既存の TOC / WebSocket / selection defer E2E の意図を壊さない。

## 検証

実装フェーズでは、対象 E2E を優先して実行する。

```bash
npm run test:e2e -- text_selection_defer.spec.ts
```

TypeScript 型検査用の npm script が存在する場合は併せて実行する。最後に必要に応じて `./verify.sh` を実行する。

docs-only の本設計書は、以下で最低限の文書検証を行う。

```bash
rg -n "TOC_NAVIGATION_SLACK_PX|連続 TOC|MutationObserver|production API" docs/superpowers/specs/2026-04-28-toc-pending-navigation-boundary-tests-design.md
```

## セキュリティ考慮

この作業はブラウザ内 UI 状態の回帰テスト強化であり、Host / Origin validation、CSP、HTML sanitization、path validation には変更を加えない。

テストは既存 fixture と DOM 観測だけを使う。外部入力、検索結果、AI 生成コードを実行する経路は増やさない。`sidebar.js` を修正する場合も、テスト用のグローバル関数露出や production HTML への新しいデバッグ API 追加は行わない。

## 影響範囲

- 主変更対象: `tests/e2e/text_selection_defer.spec.ts`
- 条件付き変更対象: `src/template/assets/js/sidebar.js`
- 参照対象: `tests/e2e/browser/test-websocket.ts`, `tests/e2e/globals.d.ts`
- 影響しない領域: Rust server / renderer、memo API、file watching、CSP / Host / Origin guard

## ロールバック

テスト追加のみで完了した場合は、`tests/e2e/text_selection_defer.spec.ts` の追加差分を revert すれば戻せる。

`sidebar.js` の最小修正が必要になった場合も、追加テストと実装修正を同一スコープに保つ。挙動に問題が出た場合は、この作業のコミットを revert し、既存の TOC pending navigation 実装へ戻す。

設計書自体は docs-only なので、この spec 追加コミットを revert すれば元に戻せる。
