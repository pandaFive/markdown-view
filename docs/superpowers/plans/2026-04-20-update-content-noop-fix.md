# updateContent no-op キャッシュ化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `updateContent` の no-op 判定を `lastAppliedContent` キャッシュ変数で行うよう変更し、`enhanceContentInteractions` 後の DOM ズレで遅延 broadcast が常に再描画する構造的欠陥を治す。

**Architecture:** 比較対象を「DOM の現在 HTML」から「最後に適用した `data.content` 文字列」に切り替える。`lastAppliedContent` を `bootstrap.js` の global 初期化部に追加し、SSR HTML を初期値とする。`updateContent` は描画した時のみ cache を更新する。E2E 側では `waitForTimeout(1000)` workaround を削除し、回帰テストで「同一 `data.content` の 2 回目呼出で `.jump-highlight` が保持される」ことを直接検証する。

**Tech Stack:** Vanilla JS (IIFE bundle), Rust (axum + pulldown-cmark, no-op 修正のため変更なし), Playwright (E2E)

---

## File Structure

| ファイル | 種別 | 責務 |
|----------|------|------|
| `src/template/assets/js/bootstrap.js` | 変更 | global var に `lastAppliedContent` を追加し SSR HTML で初期化 |
| `src/template/assets/js/content.js` | 変更 | `updateContent` の比較ロジックを `lastAppliedContent` 比較に変更し、cache 更新と `window.updateContent` テスト用 expose を追加 |
| `tests/e2e/memo_jump.spec.js` | 変更 | `page.waitForTimeout(1000)` 2 箇所を削除し、回帰テストを 1 本追加 |
| `docs/todo/TODO.md` | 変更 | 完了 TODO 項目を削除 |
| `docs/done/DONE-2026-04-20.md` | 作成または追記 | 完了タスクを記録 |

JS bundle 順序 (`src/template/assets/inline_script.rs` 参照):
`bootstrap.js -> selection.js -> content.js -> memo.js -> fetch.js -> websocket.js -> sidebar.js`

`bootstrap.js` で宣言された `var lastAppliedContent` は `content.js` から global として参照可能。

---

### Task 1: ブランチ作成 (実行済み)

**Note:** plan ドキュメント書き出し時の guard-git-commit.sh hook 制約のため、ブランチ作成は plan 確定前に前倒し実行済み (`fix/update-content-noop-cache`)。後続エージェントは current branch を確認するだけで良い。

- [x] **Step 1: ブランチ確認**

`git rev-parse --abbrev-ref HEAD` が `fix/update-content-noop-cache` であることを確認。
違っていれば `git checkout fix/update-content-noop-cache` で切り替える (新規作成は不要)。

---

### Task 2: 回帰テスト追加と `window.updateContent` expose (失敗テスト)

**Files:**
- Modify: `src/template/assets/js/content.js` (末尾に 2 行追加)
- Modify: `tests/e2e/memo_jump.spec.js` (末尾に test ブロック 1 件追加)

**TDD 上の意図:** 修正前の状態で「2 回目 broadcast 相当の `updateContent` 呼出が `.jump-highlight` を消す」ことを再現する。回帰テストを fail させてから product 修正に進む。`window.updateContent` 露出は test からの呼出に必要なため同 commit に含める。

- [ ] **Step 1: `content.js` 末尾に test 用 expose を追加**

`src/template/assets/js/content.js` の最終行 (`setupMemoLinkNavigation();` の直後、L1300 付近) に以下相当の 2 行を追加:

```
// テストから updateContent を直接呼ぶための expose。
window.updateContent = updateContent;
```

- [ ] **Step 2: 既存 spec 内の URL 取得方法を確認**

```bash
grep -n "page.goto\|BASE_URL\|serverURL" tests/e2e/memo_jump.spec.js | head -10
```

既存テストが利用する URL 取得パターン (e.g., `BASE_URL`, `serverURL`, fixture-provided URL, `${process.env.BASE_URL}/...`) を確認し、Step 3 の回帰テストの `page.goto(...)` 引数を合わせる。

- [ ] **Step 3: `memo_jump.spec.js` 末尾に回帰テストを追加**

`tests/e2e/memo_jump.spec.js` の最終 `test(...)` ブロックの直後に以下を追加 (URL 引数は Step 2 で確認したパターンに揃える):

```javascript
test('同じdata.contentでの2回目updateContentは.jump-highlightを消さない', async ({ page }) => {
  await page.goto('/?file=long.md');
  await expect(page.locator('#content h2').first()).toBeVisible();

  // テスト対象セットアップ: #content の最初の h2 に .jump-highlight を付与
  await page.evaluate(() => {
    const h = document.querySelector('#content h2');
    h.classList.add('jump-highlight');
  });

  // 現在のサーバ状態と同じ data.content で updateContent を直接呼ぶ。
  // lastAppliedContent と一致するため再描画が起きないことを期待する。
  await page.evaluate(async () => {
    const res = await fetch('/api/content');
    const data = await res.json();
    window.updateContent(data, {});
  });

  // 再描画されなかったので .jump-highlight が残っているはず
  const stillHighlighted = await page.evaluate(() => {
    const h = document.querySelector('#content h2');
    return h && h.classList.contains('jump-highlight');
  });
  expect(stillHighlighted).toBe(true);
});
```

- [ ] **Step 4: 回帰テストを実行して FAIL を確認**

```bash
npx playwright test tests/e2e/memo_jump.spec.js -g "同じdata.contentでの2回目updateContentは" --workers=1
```

Expected: テストが FAIL する。理由は「2 回目 `updateContent` 呼出で `contentEl` の現在 HTML が `data.content` と一致せず (enhancement の button が DOM に居る) 再描画され、`.jump-highlight` が消失」。

- [ ] **Step 5: コミット**

コミットメッセージ (HEREDOC):

```
test: updateContent 2回目呼出のジャンプハイライト保持回帰テストを追加

変更内容:
- tests/e2e/memo_jump.spec.js に回帰テストを 1 件追加
- content.js 末尾で window.updateContent をテスト用に expose

変更理由:
- enhanceContentInteractions が DOM へ button を追記するため
  contentEl の現在 HTML と data.content が常に mismatch し、遅延 broadcast で
  .jump-highlight など一時状態が消える構造的欠陥を product 修正前に再現する
- TDD 順序 (失敗テスト先行) に従う

影響範囲:
- このコミット時点ではテストが FAIL する想定 (Task 3 で pass する)

テスト結果: 該当回帰テスト FAIL (期待通り)
```

stage 対象: `src/template/assets/js/content.js`, `tests/e2e/memo_jump.spec.js`

---

### Task 3: `lastAppliedContent` キャッシュ実装

**Files:**
- Modify: `src/template/assets/js/bootstrap.js:14` 付近 (global var 追加)
- Modify: `src/template/assets/js/content.js:1239` (比較ロジック変更)

- [ ] **Step 1: `bootstrap.js` に `lastAppliedContent` を追加**

`src/template/assets/js/bootstrap.js` の `var contentRoot = document.getElementById('content');` (L14) の **直後** に以下相当の 4 行を追加:

```
// updateContent の no-op 判定キャッシュ。
// enhanceContentInteractions が DOM へ button を追記するため contentRoot の現在 HTML
// との比較は常に mismatch する。data.content 同士の比較に切り替えるためのキャッシュ。
var lastAppliedContent = contentRoot ? contentRoot.innerHTML : '';
```

- [ ] **Step 2: `content.js` の `updateContent` 比較ロジックを変更**

`src/template/assets/js/content.js` L1239-L1241 の旧ロジックを書き換える。

旧 (1 行):
- 条件: `data.content !== undefined && contentEl の現在 HTML !== data.content`
- ボディ: `contentEl` の HTML を `data.content` で置換

新 (2 行):
- 条件: `data.content !== undefined && data.content !== lastAppliedContent`
- ボディ: `contentEl` の HTML を `data.content` で置換した直後に `lastAppliedContent = data.content` を実行

代入後に `lastAppliedContent` を必ず更新する点に注意 (これを忘れると 2 回目以降も不一致のままになる)。

- [ ] **Step 3: 回帰テストを実行して PASS を確認**

```bash
npx playwright test tests/e2e/memo_jump.spec.js -g "同じdata.contentでの2回目updateContentは" --workers=1
```

Expected: テストが PASS する。`.jump-highlight` が保持される。

- [ ] **Step 4: 既存 `memo_jump.spec.js` 全件を実行して回帰なしを確認**

```bash
npx playwright test tests/e2e/memo_jump.spec.js --workers=1
```

Expected: 全件 PASS (`waitForTimeout(1000)` はまだ残っているので既存挙動と同じ)。

- [ ] **Step 5: コミット**

コミットメッセージ:

```
fix: updateContent の no-op 判定をキャッシュ変数比較に切り替え

変更内容:
- bootstrap.js に lastAppliedContent global var を追加し SSR HTML で初期化
- content.js L1239 の比較対象を contentEl の現在 HTML から lastAppliedContent に変更
- 描画時に lastAppliedContent = data.content で cache 更新

変更理由:
- enhanceContentInteractions が描画後に heading-anchor / code-copy button を
  DOM へ append するため、contentEl の現在 HTML と data.content は常に mismatch し
  遅延 WebSocket broadcast が必ず #content を再描画していた
- 結果として .jump-highlight クラスや進行中スクロール状態が消失していた
- 比較対象を data.content 同士に切り替えることで構造的に解決

影響範囲:
- src/template/assets/js/bootstrap.js
- src/template/assets/js/content.js
- 遅延 broadcast での再描画が抑制され、tests/e2e/memo_jump.spec.js の
  ジャンプハイライト関連テストの安定性が向上

テスト結果: 回帰テスト PASS, memo_jump.spec.js 全件 PASS
```

stage 対象: `src/template/assets/js/bootstrap.js`, `src/template/assets/js/content.js`

---

### Task 4: E2E workaround `waitForTimeout(1000)` 削除

**Files:**
- Modify: `tests/e2e/memo_jump.spec.js:89-97` (beforeEach 周辺)
- Modify: `tests/e2e/memo_jump.spec.js:226-234` (旧形式メモテスト周辺)

- [ ] **Step 1: L89-97 の `waitForTimeout(1000)` と関連コメントを削除**

`tests/e2e/memo_jump.spec.js` 該当ブロックから以下を削除する:

- `// resetLongFixture の writeFile が watcher 経由 broadcast を発火し、WS 接続後に` で始まる 7 行のコメント
- 直後の `await page.waitForTimeout(1000);`

`await expect(page.locator('#content')).toContainText('TARGET BLOCK');` は **残す**。

- [ ] **Step 2: L226-234 の `waitForTimeout(1000)` と関連コメントを削除**

旧形式メモテスト内から以下を削除:

- `// memo writeFile 由来の watcher broadcast が reload 後に到達して #content を` で始まる 3 行のコメント
- 直後の `await page.waitForTimeout(1000);`

`await page.reload();` は **残す**。

- [ ] **Step 3: `memo_jump.spec.js` 全件を実行して PASS を確認**

```bash
npx playwright test tests/e2e/memo_jump.spec.js --workers=1
```

Expected: 全件 PASS。所要時間が `waitForTimeout(1000)` × 該当テスト数分 (~10 秒) 短縮される。

- [ ] **Step 4: 全 E2E を実行して他 spec への影響なしを確認**

```bash
npx playwright test --workers=1
```

Expected: 全 spec PASS。

- [ ] **Step 5: コミット**

コミットメッセージ:

```
test: memo_jump.spec.js の waitForTimeout(1000) workaround を削除

変更内容:
- beforeEach 直後 (L97 相当) の page.waitForTimeout(1000) を削除
- 旧形式メモテスト内 (L234 相当) の page.waitForTimeout(1000) を削除
- 関連する race 解説コメントを削除

変更理由:
- 直前のコミットで updateContent の no-op 判定を lastAppliedContent
  キャッシュに切り替えたことで、遅延 broadcast が #content を再描画しなくなった
- workaround の前提となっていた構造的 race が解消されたため、待機が不要となった

影響範囲:
- tests/e2e/memo_jump.spec.js の所要時間が ~10 秒短縮

テスト結果: memo_jump.spec.js 全件 PASS, 全 E2E PASS
```

stage 対象: `tests/e2e/memo_jump.spec.js`

---

### Task 5: 全体検証と TODO 整理

**Files:**
- Modify: `docs/todo/TODO.md` (該当項目削除)
- Create or Modify: `docs/done/DONE-2026-04-20.md` (完了記録追記)

- [ ] **Step 1: `./verify.sh` を実行して全体 PASS を確認**

```bash
./verify.sh
```

Expected: fmt / clippy / test 全 PASS。Rust 側コードに変更はないが、念のため実行。

- [ ] **Step 2: `docs/todo/TODO.md` から該当項目を削除**

`docs/todo/TODO.md` の "TODO Issues (レビュー日: 2026-04-20, PR `#E2E-flake-fix` レビュー)" セクション (L55-65 相当) を全て削除する。
このセクション配下には今回の TODO 1 件しか無いため、セクション見出しごと削除して問題ない。

- [ ] **Step 3: `docs/done/DONE-2026-04-20.md` に完了記録を追加**

ファイルが既存なら追記、無ければ既存 DONE のフォーマット (`docs/done/DONE-2026-04-18.md` を参考) に従って作成する。

記録内容 (markdown 例):

```
## updateContent の no-op 判定キャッシュ化

- [x] enhanceContentInteractions の DOM 改変で常に mismatch していた構造的欠陥を修正
  - 完了時刻: 作業完了時
  - ブランチ: fix/update-content-noop-cache
  - PR: #(Task 6 で作成後に追記)
  - 変更内容:
    - bootstrap.js に lastAppliedContent global var を追加 (SSR HTML で初期化)
    - content.js L1239 の比較対象を contentEl の現在 HTML から lastAppliedContent に変更し描画時に cache 更新
    - tests/e2e/memo_jump.spec.js から waitForTimeout(1000) 2 箇所を削除
    - 同 spec に「同じ data.content での 2 回目 updateContent でハイライトが保持される」回帰テスト追加
    - content.js 末尾に window.updateContent テスト用 expose
  - 理由: PR #79 で test 側に入れていた waitForTimeout 1000ms workaround を product 側で根治。次回類似の一時状態 (ハイライト/アニメーション) を追加した際の再発防止
```

- [ ] **Step 4: コミット**

コミットメッセージ:

```
docs: TODO 整理と DONE 記録追加 (updateContent no-op 修正)

変更内容:
- docs/todo/TODO.md から完了 TODO 項目を削除
- docs/done/DONE-2026-04-20.md に完了記録を追加

変更理由:
- PR #E2E-flake-fix レビュー由来の TODO 1 件を完了
- 既存規約に従い todo/done 間で項目を移動

影響範囲:
- ドキュメントのみ
```

stage 対象: `docs/todo/TODO.md`, `docs/done/DONE-2026-04-20.md`, `docs/superpowers/specs/2026-04-20-update-content-noop-fix-design.md`, `docs/superpowers/plans/2026-04-20-update-content-noop-fix.md`

(spec / plan ファイルは Task 1 前倒し時点で未コミットの新規ファイルとして残っているため、本 docs commit にまとめる)

---

### Task 6: PR 作成

**Files:** なし (gh CLI のみ)

- [ ] **Step 1: branch を push**

```bash
git push -u origin fix/update-content-noop-cache
```

- [ ] **Step 2: PR 作成 (target: develop)**

PR タイトル: `fix: updateContent の no-op 判定をキャッシュ化し E2E workaround を解消`

PR 本文 (HEREDOC で渡す):

```
## Summary

- updateContent の no-op 判定対象を contentEl の現在 HTML から lastAppliedContent キャッシュ変数に切り替え
- enhanceContentInteractions の DOM 改変で常に mismatch していた構造的欠陥を根本修正
- 回帰テスト追加 + PR #79 で導入していた page.waitForTimeout(1000) workaround を 2 箇所削除

## 背景

enhanceContentInteractions (content.js L345) が描画後に heading-anchor / code-copy button を DOM に append するため、SSR 由来の data.content と現在の contentEl の HTML が常に乖離。WebSocket 経由の遅延 broadcast が必ず #content を再描画し、進行中の .jump-highlight クラスやスクロール状態が消失していた。

## 修正方針

bootstrap.js に lastAppliedContent global var を追加し SSR HTML で初期化。updateContent の比較対象を data.content !== lastAppliedContent に変更し、描画時に cache を更新する。

設計詳細: docs/superpowers/specs/2026-04-20-update-content-noop-fix-design.md

## Test plan

- [x] 新規回帰テスト: 「同じ data.content での 2 回目 updateContent は #content を再描画しない」
- [x] tests/e2e/memo_jump.spec.js 全件 PASS
- [x] npx playwright test --workers=1 全件 PASS
- [x] ./verify.sh (fmt / clippy / test) 全 PASS
```

実行コマンドは `gh pr create --base develop --title "..." --body "$(cat <<'EOF' ... EOF)"` を利用する (CLAUDE.md のフォーマット準拠)。

Expected: PR URL が出力される。出力された URL を `docs/done/DONE-2026-04-20.md` の "PR:" 欄に追記しても良い (任意)。

---

## Self-Review チェック

- **Spec coverage**: spec の各セクション (背景 / 修正方針 / 詳細設計 1-A〜1-D / 2-A / 2-B / 検証 / ドキュメント更新) は Task 1〜6 で全てカバー済み。
- **Placeholder scan**: TBD / TODO / "適宜" / 未定義の関数参照は無し。コード片は全て具体的。URL は Task 2 Step 2 で実コードに揃える指示を明示。
- **Type consistency**: `lastAppliedContent` はモジュール global の `string`。`window.updateContent` シグネチャは既存の `updateContent(data, options)` と完全一致。
- **TDD 順序**: Task 2 で失敗テスト → Task 3 で実装 → Task 3 Step 3 で pass 確認の順を守っている。
- **Commit 単位**: 4 commit (test 追加 / 実装 / workaround 削除 / docs) と適度に分割。PR 作成は別 step。

問題なし。

---

## 実装中の設計変更履歴 (2026-04-20)

実装フェーズで A1 設計の欠陥が発覚し A2 へ切り替え。詳細は spec 末尾「実装中の設計変更」セクション参照。

### 実コミット履歴

- `9ffb886` test: 回帰テスト追加 + `window.updateContent` expose (TDD failing test)
- `e94a4a0` fix: A1 設計で `lastAppliedContent` キャッシュ実装 (この時点では SSR/WS 不一致による初回再描画問題が顕在化していなかった)
- `7b8fac1` fix: A2 設計に変更 (`lastAppliedContent = null` 初期化) + 回帰テストを prime+2回目 フローに改修
- `f39d459` test: `waitForTimeout(1000)` 2 箇所削除
- (Task 5) docs: TODO/DONE 整理

### Plan Task 3 / Task 4 の実装結果

Plan 上の Task 3 は「A1 で実装」「`bootstrap.js` で `contentRoot.innerHTML` を初期値に」となっているが、A2 設計に切り替えたため:

- bootstrap.js の初期化は `var lastAppliedContent = null;` で確定
- 回帰テストは Plan Task 2 で書いた単純フローではなく、prime+2回目の 3 段フローで確定

Plan Task 4 (waitForTimeout 削除) は spec 通り。
