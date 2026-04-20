# E2E flake 全件解消 設計

- 作成日: 2026-04-20
- 対象: `docs/todo/TODO.md` Medium「Flaky E2E テスト 3件の安定化」+ 観測された関連 flake 全件
- 方針: 共通根本原因（worker 並列実行による fixture race）を 1行設定で潰し、残る真の flake (L310) を `expect.poll` で個別修正

## 背景

`./verify.sh` に E2E を組み込む前提で flake 許容しないと TODO.md に明記。デバッグの結果、現状の Playwright デフォルト並列実行では 5回中 5回いずれかのテストが失敗する（5/5 で `memo_sync.spec.js:48`、4/5 で `text_selection_defer.spec.js:180` ほか）。

`workers=1` で 5回実行すると 390件中失敗わずか 2件（`markdown_links.spec.js:310` のみ、CDP 非同期 warning 由来）。

## 根本原因

### 主因: 共有 fixture への並列書き込み race

Playwright デフォルトでは `workers = ncpu/2`、複数 spec ファイルが並列実行される。すべての E2E テストが共通の固定 fixture ディレクトリ `tests/fixtures/e2e/{README.md, notes.md}` に書き込むため、別 spec の `beforeEach` が直前テストの fixture を上書きし、live update broadcast 経由で別ページの content を巻き戻す。

観測例（`text_selection_defer.spec.js:180`）:
- 期待 `#content` に `'Notes body'`（元の notes.md）
- 実受信 `'Notes# / Paragraph 1 / Paragraph 2 / ...30+件'`
- 受信内容は `markdown_links.spec.js:41/L62/L253` が書き込んだ「Notes + 40段落 + Beta セクション」と一致 → 別 spec が同 fixture に書いた残骸

該当する flake テスト（5回実行で観測）:
| テスト | 5回中失敗 | 症状 |
|--------|-----------|------|
| `memo_sync.spec.js:48` | 5/5 | peer の memo-editor に前テストの memo 残存 |
| `text_selection_defer.spec.js:180` | 4/5 | notes.md に他 spec の段落データ |
| `markdown_links.spec.js:253` | 2/5 | 同上、`日本語見出し` 取得失敗 |
| `markdown_links.spec.js:41` | 1/5 | 同上、`Target section body` 取得失敗 |
| `markdown_links.spec.js:92` | 1/5 (30s timeout) | 同上 |
| `text_selection_defer.spec.js:311` | 1/5 | toc に `Alpha` 含まれず |

### 副因: CDP イベント非同期遅延

`markdown_links.spec.js:310` は serial 実行でも 5回中 1-2回失敗。`page.on('console')` で受け取る warning が assertion 実行時にまだ Node 側へ到達していない。`window.scrollY === 0` などの DOM 観測 assertion はパスしているのに warning 配列だけ空。

## 修正

### 修正 1: `playwright.config.js` に `workers: 1` 追加

```js
module.exports = defineConfig({
  testDir: './tests/e2e',
  timeout: 30000,
  workers: 1,
  use: { ... },
  ...
});
```

- 効果: 並列起因の 6 件以上の flake が一括解消
- 副作用: 全 E2E 実行時間が 20秒 → 80-120秒（並列 → 直列）
- 妥当性: `./verify.sh` に組み込む前提では決定論性 >> 速度。個人利用ツールで CI 並列負荷もない

### 修正 2: `tests/e2e/markdown_links.spec.js:341` を `expect.poll` 化

```js
// 変更前
expect(warnings.some((msg) => msg.indexOf('リンク先の見出しが見つかりません') !== -1)).toBe(true);

// 変更後
await expect.poll(() => warnings.some((msg) => msg.indexOf('リンク先の見出しが見つかりません') !== -1)).toBe(true);
```

- 効果: CDP イベント到達を最大 5秒 polling で待ち、serial 実行で残る唯一の flake を解消
- 同じ warning 検証は L152, L274, L307 にも存在するが現状で flake 観測なし。YAGNI で本箇所のみ修正

### 修正 3: `docs/todo/TODO.md` の Medium 項目を削除

「Flaky E2E テスト 3件の安定化」エントリと冒頭サマリーの Medium 言及を削除する（解決済み）。

## 検証

1. `npx playwright test` を 10回連続実行して全 PASS を確認
2. `./verify.sh` PASS
3. 直列化により実行時間が約 4 倍になることを確認、ユーザに体感差を共有

## スコープ外（将来課題）

- **`fullyParallel` 化やワーカー別 fixture 隔離**: 並列実行を維持したまま race を防ぐ抜本策。各テストごとに一時 fixture ディレクトリを生成し、対応する markdown-view サーバを別ポートで起動する大規模リファクタが必要。本 PR では実施せず Low TODO に記録
- **`markdown_links.spec.js:219` の 30s timeout**: 今回 5回中再現せず根本原因未確定。将来再観測時に対応
- **`markdown_links.spec.js:310` 以外の warning polling 検証**: 同パターン (L152, L274, L307) は現状 flake 観測なしで予防修正は YAGNI

## 非対象

- PR #76, #77 レビュー由来の Low TODO 6件はそのまま残置
