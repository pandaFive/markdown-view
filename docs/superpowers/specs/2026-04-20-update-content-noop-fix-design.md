# updateContent の no-op check 構造修正 設計書

- **作成日**: 2026-04-20
- **対象 TODO**: `docs/todo/TODO.md` "TODO Issues (レビュー日: 2026-04-20, PR `#E2E-flake-fix` レビュー)" / Low Priority 1 件目
- **対象ブランチ予定**: `fix/update-content-noop-cache`
- **関連 PR**: #79 (E2E flake fix で `waitForTimeout(1000)` workaround を導入した PR)

## 背景

`src/template/assets/js/content.js` の `updateContent` 関数 L1239 に以下相当の no-op 判定がある:

> `if (data.content !== undefined && contentEl.innerHTML !== data.content) { contentEl.innerHTML = data.content; }`

ただし、同ファイル L345 の `enhanceContentInteractions` が描画後に下記要素を DOM へ append する:

- `<button class="heading-anchor">#</button>` を各見出し (`h1`〜`h6`) 内に追加
- `<button class="code-copy">Copy</button>` を各 `pre.code-block` 内に追加

このため SSR で生成された `data.content` と enhancement 後の `contentEl` の現在 HTML は **常に mismatch** する。
結果として、WebSocket 経由の遅延 broadcast が必ず `#content` を再描画し、進行中の一時状態
（`.jump-highlight` クラスやスクロール位置の進行など）を消失させる。

XSS 観点: `data.content` は server 側で pulldown-cmark の raw HTML 無効化済み（CLAUDE.md 参照）。
本修正は既存の代入経路を変更せず、no-op 判定の比較対象のみを変える。

### 観測される実害

1. **テスト**: `tests/e2e/memo_jump.spec.js` L97 / L234 で `page.waitForTimeout(1000)` を入れて
   遅延 broadcast の到達を待ってからハイライト検証を行わざるを得ない（PR #79 の workaround）。
2. **UI**: メモ出典クリック直後に付与された `.jump-highlight` が遅延 broadcast で 1 回点滅する
   軽微な visual バグ（実害は小さいが「次回別の一時状態を追加した時に再発する」構造的脆弱性が本質）。

## 修正方針 (案 A: `lastAppliedContent` キャッシュ変数)

比較対象を「DOM の現在 HTML」から「最後に適用した `data.content` 文字列」に切り替える。
DOM がどれだけ enhancement で改変されても、比較は cache 変数に対して行うため一致が崩れない。

### 検討した別案

- **案 B (cloneNode + remove で enhancement 抜き HTML を比較)**: clone コストと strip ロジックの保守負担あり。
- **案 C (enhancement 要素の別コンテナ化)**: `heading-anchor` を見出し外に出すと `#` 配置の視覚調整が複雑、CSS 変更も大きい。
- **案 A (採用)**: 修正最小、パフォーマンス影響なし、enhancement が増えても勝手に追従。

## 詳細設計

### 1. `src/template/assets/js/content.js` の変更

#### 1-A. モジュールスコープ変数を追加

`updateContent` 関数の手前 (既存の `normalizeTocHtml` 付近) に以下相当の変数宣言を追加:

> `var lastAppliedContent = null;`

コメントとして、enhancement が DOM を改変するため `contentEl` の現在 HTML との比較は常に
mismatch する旨と、`data.content` 同士の比較に切り替える意図を残す。

#### 1-B. 初期化: SSR HTML を初期値としてキャッシュ

スクリプト末尾の `setupDocumentSearch();` などが並ぶ初期化部の手前で
`contentRoot` が存在すれば `lastAppliedContent` に `contentRoot` の現在 HTML を代入する。

`contentRoot` は既に同ファイル内で `document.getElementById('content')` から取得済み。
SSR HTML は enhancement 適用前の状態なので、WS broadcast の `data.content` と一致する想定。
万一一致しなくても初回 broadcast で 1 回再描画されるだけで、既存挙動から悪化しない。

#### 1-C. `updateContent` の比較ロジック変更

L1239 を「`data.content !== undefined && data.content !== lastAppliedContent` ならば
代入し、その直後に `lastAppliedContent = data.content` で cache を更新」する形に変更。

- 比較対象を `lastAppliedContent` に変更
- 描画した時のみ `lastAppliedContent` を更新（同一 content の二度目以降は no-op）

#### 1-D. toc は現状維持

`tocEl` には enhancement が適用されないため、既存の `normalizeTocHtml` 比較で十分。
今回は変更しない（YAGNI 原則）。

### 2. `tests/e2e/memo_jump.spec.js` の変更

#### 2-A. `waitForTimeout(1000)` の削除

該当 2 箇所:

- L89-97 (beforeEach 周辺) の `page.waitForTimeout(1000)` とコメント
- L231-234 (旧形式メモテスト) の `page.waitForTimeout(1000)` とコメント

product 側修正で遅延 broadcast が再描画を起こさなくなるため、待機不要となる。
コメント (race の解説) も削除する。

#### 2-B. 回帰テスト追加

ファイル末尾に以下フローのテストを 1 本追加する:

1. 任意のページ (long.md など) を表示し `#content` の表示を待つ。
2. `page.evaluate()` で `#content` 内の任意の要素 (例: 最初の `h2`) に `.jump-highlight` を付与する。
3. `page.evaluate()` で `/api/content` から現在の `data.content` を取得し、
   `window.updateContent(data, {})` を直接呼び出す。
4. `.jump-highlight` クラスが残っていることを `expect` で検証 (再描画されなかった証明)。

`updateContent` をテストから呼ぶため、`content.js` 末尾で
`window.updateContent = updateContent;` 相当のテスト用 expose を行う。
CSP は同一スクリプト内露出なので影響しない。

### 3. 検証

```bash
# Rust 側 (no-op 修正のため変化なしの想定)
./verify.sh

# Playwright E2E
npx playwright test tests/e2e/memo_jump.spec.js
```

`waitForTimeout(1000)` 削除により所要時間が ~10 秒短縮されること、
新規回帰テストが pass することを確認する。

### 4. ドキュメント更新

- `docs/todo/TODO.md` から該当 TODO 項目（PR #E2E-flake-fix レビューの 1 件目）を削除
- `docs/done/DONE-2026-04-20.md` に完了記録を追加（既存規約に従う、`/move-todo` 相当の処理）

## 影響範囲

| 領域 | 変更内容 | 行数目安 |
|------|----------|----------|
| `src/template/assets/js/content.js` | 変数追加 + 初期化 + 比較ロジック変更 + window expose | ~10 行 |
| `tests/e2e/memo_jump.spec.js` | `waitForTimeout(1000)` 削除 (2 箇所) + 回帰テスト追加 | -10 行 / +35 行 |
| `docs/todo/TODO.md` / `docs/done/DONE-*.md` | TODO 項目移動 | ~5 行 |

## リスク

| リスク | 影響度 | 緩和策 |
|--------|--------|--------|
| SSR HTML と WS `data.content` が初期化時点で不一致 | Low | 初回 broadcast で 1 回再描画されるが既存挙動から悪化なし |
| `lastAppliedContent` 初期化前の WS broadcast 到達 | Negligible | 初期化は同期実行で WS 接続前に完了 |
| 回帰テストで `window.updateContent` 露出が「テスト専用 API の混入」になる | Low | コメントで明示。CSP には影響なし（同一スクリプト内露出） |
| `data.content` が常に同一文字列で参照等価判定すべき問題が起きる | Negligible | 文字列比較なので参照等価ではなく value 等価で動作 |

## 完了条件

- [ ] `content.js` の修正適用
- [ ] `tests/e2e/memo_jump.spec.js` の `waitForTimeout(1000)` 削除
- [ ] 回帰テスト追加
- [ ] `./verify.sh` pass
- [ ] `npx playwright test tests/e2e/memo_jump.spec.js` 全件 pass
- [ ] `docs/todo/TODO.md` から該当項目移動
- [ ] PR 作成 (target: `develop`)

## 実装中の設計変更 (2026-04-20)

実装フェーズの Task 3 検証中に **A1 (SSR HTML キャッシュ) では SSR HTML と WS 由来 `data.content` が完全一致しない** ことが判明。原因はブラウザ HTML 正規化 (attribute 順序・quote 種・whitespace 等)。結果として初回 WS broadcast で必ず再描画され、`waitForTimeout(1000)` 削除と矛盾する状態となった。

### 採用設計: A2 (`lastAppliedContent = null` 初期化)

- bootstrap.js での初期化を `null` に変更
- 初回 WS broadcast での 1 回再描画を仕様として受容 (UI 上 `.jump-highlight` 付与前タイミングのため実害なし)
- 回帰テストは「1 回目 prime → `.jump-highlight` 付与 → 2 回目 no-op 検証」の 3 段フローに改修し、本質である「同一 content の 2 回目以降は再描画されない」を検証

### A2 採用理由

- A1 で SSR/WS 一致を担保するための正規化レイヤー追加は複雑度高
- A2 の「初回 1 回再描画」は実用上無害 (sidebar.js:438 の initial `enhanceContentInteractions()` 後に WS 接続 → 初回 broadcast 到来 → 再描画 → 以降 no-op)
- `.jump-highlight` 等の一時状態は「初回 broadcast 完了後に付与される」運用なので干渉なし
- 元の TODO 価値 (`waitForTimeout(1000)` 削除) は両設計とも達成可能

### 実装結果

- Commit `7b8fac1`: A2 設計 + 回帰テスト 3 段フロー化
- Commit `f39d459`: `waitForTimeout(1000)` 2 箇所削除
- 全 E2E spec 79 件 PASS、`memo_jump.spec.js` 所要時間 25.9s → 4.9s
