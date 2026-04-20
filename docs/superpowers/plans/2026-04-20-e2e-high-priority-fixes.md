# E2E High Priority Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/todo/TODO.md` の E2E High Priority 2件 (`markdown_links.spec.js:124`、`document_search.spec.js:389`) を修正し develop baseline の失敗をゼロにする。

**Architecture:** Bug #1 はプロダクトコードの warning 文言を他2箇所と用語統一（1行）。Bug #2 はプロダクトコードの挙動（auto-selection）が仕様として正しいため、テストの期待値を実挙動と兄弟テスト L437 の規約に合わせる（1行）。最後に TODO.md から完了項目を除去する。

**Tech Stack:** Playwright (E2E)、Vanilla JS (`src/template/assets/js/content.js`)、Rust CLI（本件では無変更）

**Spec:** `docs/superpowers/specs/2026-04-20-e2e-high-priority-fixes-design.md`

**ブランチ:** 実装は `fix/e2e-high-priority-test-failures` で実施（既に spec コミット済み）

---

## File Structure

本プランで変更するファイル:

- `src/template/assets/js/content.js` — 同一ファイル内 hash click handler の warning 文言を修正（Bug #1）
- `tests/e2e/document_search.spec.js` — ディレクトリモード検索一覧表示テストの summary 期待値を修正（Bug #2）
- `docs/todo/TODO.md` — High Priority 2件の完了に伴うセクション削除

---

### Task 1: Bug #1 — 同一ファイル内未解決 hash click の warning 文言統一

**Files:**
- Modify: `src/template/assets/js/content.js:402`
- Test: `tests/e2e/markdown_links.spec.js:124`（既存、修正なし）

- [ ] **Step 1: 失敗テストの現状を確認**

Run:
```bash
npx playwright test tests/e2e/markdown_links.spec.js:124 --reporter=list
```

Expected: FAIL at line 152 assertion
```
expect(warnings.some((msg) => msg.indexOf('見出しが見つかりません') !== -1)).toBe(true);
Expected: true
Received: false
```

根本原因確認: `src/template/assets/js/content.js:402` の warning が `'同一ファイル内のジャンプ先が見つかりません'` を出力しており、テストが検索する `'見出しが見つかりません'` 部分文字列を含まない。

- [ ] **Step 2: warning 文言を修正**

`src/template/assets/js/content.js` の L402 を以下のように変更する。

変更前:
```js
      console.warn('[markdown-view] 同一ファイル内のジャンプ先が見つかりません:', target.hash);
```

変更後:
```js
      console.warn('[markdown-view] 同一ファイル内の見出しが見つかりません:', target.hash);
```

周辺コードは変更しない。

- [ ] **Step 3: テストが pass に転じることを確認**

Run:
```bash
npx playwright test tests/e2e/markdown_links.spec.js:124 --reporter=list
```

Expected: PASS（1 passed）

- [ ] **Step 4: 周辺テスト回帰確認**

Run:
```bash
npx playwright test tests/e2e/markdown_links.spec.js --reporter=list
```

Expected: `markdown_links.spec.js:41` は TODO.md に記載された flaky なので retry で pass しうる。L124 を含む他テストが pass すれば OK。L41 が fail した場合は再実行して flaky と確認。

- [ ] **Step 5: Rust 側の無影響を確認**

Run:
```bash
./verify.sh
```

Expected: PASS（本修正は JS アセット文字列のみで Rust 単体テストに影響なし）。

- [ ] **Step 6: コミット**

コミットメッセージを `/tmp/commit-msg.txt` に書き出してから `git commit -F` を使う（heredoc の日本語チェックを通すため）。

```bash
git add src/template/assets/js/content.js
cat > /tmp/commit-msg.txt <<'EOF'
fix: 同一ファイル内未解決hashクリック時の警告文言を他経路と統一

変更内容:
- src/template/assets/js/content.js:402 の warning 文言
  「同一ファイル内のジャンプ先が見つかりません」→「同一ファイル内の見出しが見つかりません」

変更理由:
- 他2箇所 (L287 履歴復元、L1264 別ファイル遷移) の warning が
  「見出しが見つかりません」で統一されており用語を揃える
- 実際の検索対象は parsed.headingId で見出しIDであり用語としても正確
- E2E テスト (markdown_links.spec.js:124) の assertion に一致

影響範囲:
- src/template/assets/js/content.js のみ
- 挙動変更なし（warning 文言のみ）

テスト結果: npx playwright test tests/e2e/markdown_links.spec.js:124 PASS
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

---

### Task 2: Bug #2 — ディレクトリ検索一覧表示テストの summary 期待値更新

**Files:**
- Modify: `tests/e2e/document_search.spec.js:429`
- 参照のみ: `tests/e2e/document_search.spec.js:437-483`（auto-selection 仕様を保証する兄弟テスト）

- [ ] **Step 1: 失敗テストの現状を確認**

Run:
```bash
npx playwright test tests/e2e/document_search.spec.js:389 --reporter=list
```

Expected: FAIL at line 429 assertion
```
Locator:  locator('#document-search-summary')
Expected: "0 / 2 件"
Received: "1 / 2 件"
```

根本原因確認: `getPreferredDirectorySearchSelection` が本文内 "note" ヒットによる `currentDocumentSearchIndex=0` を preferred selection として返し、`resolveDirectorySearchIndex` が API results 先頭 (`README.md`, `file_match_index=0`) とマッチさせるため `currentDirectorySearchIndex=0` になる（auto-selection 動作）。本挙動は兄弟テスト L437 で保証されている仕様であり、テスト側の期待値のみを修正する。

- [ ] **Step 2: 兄弟テスト L437 が現在 pass であることを確認**

Run:
```bash
npx playwright test tests/e2e/document_search.spec.js:437 --reporter=list
```

Expected: PASS（auto-selection 仕様を維持することの根拠。fail していたら方針から見直しが必要）。

- [ ] **Step 3: summary 期待値を修正**

`tests/e2e/document_search.spec.js` の L429 を以下のように変更する。

変更前:
```js
  await expect(page.locator('#document-search-summary')).toHaveText('0 / 2 件');
```

変更後:
```js
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');
```

L430-L434 の他 assertion（件数 2件、先頭が README.md、2件目が notes.md）は変更しない。

- [ ] **Step 4: テストが pass に転じることを確認**

Run:
```bash
npx playwright test tests/e2e/document_search.spec.js:389 --reporter=list
```

Expected: PASS（1 passed）

- [ ] **Step 5: `document_search.spec.js` 全体の回帰確認**

Run:
```bash
npx playwright test tests/e2e/document_search.spec.js --reporter=list
```

Expected: 全 PASS（兄弟の auto-selection 系テストも含めて）。

- [ ] **Step 6: コミット**

```bash
git add tests/e2e/document_search.spec.js
cat > /tmp/commit-msg.txt <<'EOF'
test: ディレクトリ検索一覧表示テストのsummary期待値をauto-selection仕様に合わせる

変更内容:
- tests/e2e/document_search.spec.js:429 の期待値
  '0 / 2 件' → '1 / 2 件'

変更理由:
- PR #51 で導入された auto-selection（現在ファイルの本文ヒットを
  検索結果側でも選択状態にする）が兄弟テスト L437 で保証された仕様
- 本テスト (L389) の意図は API 結果の一覧表示で件数・ファイル名が
  正しいことの検証であり、summary の先頭選択の有無は主題ではない
- 実挙動と期待値の乖離を解消し develop baseline の失敗を解消

影響範囲:
- tests/e2e/document_search.spec.js のみ（プロダクトコードは無変更）

テスト結果: npx playwright test tests/e2e/document_search.spec.js PASS
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

---

### Task 3: TODO.md の High Priority セクション削除と全体検証

**Files:**
- Modify: `docs/todo/TODO.md:7-26`（High Priority セクション全体）

- [ ] **Step 1: 広範な E2E 回帰確認**

Run:
```bash
npx playwright test tests/e2e/markdown_links.spec.js tests/e2e/document_search.spec.js --reporter=list
```

Expected: 全 PASS。flaky な `markdown_links.spec.js:41` が fail した場合は 1 回再実行し、retry で pass するか確認。継続的に失敗する場合は TODO.md の Medium セクションで別途対応する課題のため本 PR では許容。

- [ ] **Step 2: `./verify.sh` で Rust 全検証**

Run:
```bash
./verify.sh
```

Expected: 全 PASS（fmt / clippy / cargo test）。

- [ ] **Step 3: TODO.md から High Priority 2件を削除**

`docs/todo/TODO.md` の L7-L26（`### High Priority` ヘッダから空行含む Medium 直前まで）をまとめて削除する。削除後のファイルは以下のように L3-L5 の失敗サマリー → L27 以降の Medium セクションが連続する構造になる。

削除対象:
```markdown
### High Priority

- [ ] `markdown_links.spec.js:124` 壊れたフラグメントリンクの hash クリア + 警告動作
  - ファイル: `tests/e2e/markdown_links.spec.js`
  - 行番号: L124（テスト開始行）
  - 症状: 同一ファイル内の存在しないフラグメントリンクで URL hash クリアと警告が期待通りに動作しない
  - 再現: `npx playwright test tests/e2e/markdown_links.spec.js:124`
  - 再現性: develop でも再現（stable）
  - 理由: markdown 内リンクの基本 UX、壊れたアンカーの fallback 挙動
  - 優先度: High

- [ ] `document_search.spec.js:389` ディレクトリモード検索 API 結果の一覧表示
  - ファイル: `tests/e2e/document_search.spec.js`
  - 行番号: L389（テスト開始行）
  - 症状: ディレクトリモードで `/api/search` 結果の一覧描画が期待通りにならない
  - 再現: `npx playwright test tests/e2e/document_search.spec.js:389`
  - 再現性: develop でも再現（stable）
  - 理由: 検索機能の E2E 検証、ディレクトリモード固有の描画経路
  - 優先度: High

```

加えて L3-L5 の総括文は残しつつ「5 件の失敗」を「3 件の失敗」に、「develop baseline でも 3 件が再現」を「develop baseline でも 1 件（Medium の flaky 2件を含めて 3 件）が再現」は実態に合わなくなるため次のように書き換える:

変更前 (L3-L5):
```markdown
## E2E テスト失敗（2026-04-20 発見）

`npx playwright test` 全体実行で 5 件の失敗を確認。develop baseline でも 3 件が再現するため PR #76 の変更起因ではなく pre-existing な不具合または flake。`./verify.sh` には E2E が含まれないため CI で検知されていない。
```

変更後:
```markdown
## E2E テスト失敗（2026-04-20 発見）

`npx playwright test` 全体実行で当初 5 件の失敗を確認。うち High Priority 2件は修正済み（本ブランチ）。残る Medium の flake 2件は `./verify.sh` に E2E を組み込む前提で対応する。
```

- [ ] **Step 4: TODO.md 変更差分の確認**

Run:
```bash
git diff docs/todo/TODO.md
```

Expected: High Priority セクション（L7-L26 相当）が削除され、冒頭サマリーが更新されている。Medium 以降のセクションは無変更。

- [ ] **Step 5: コミット**

```bash
git add docs/todo/TODO.md
cat > /tmp/commit-msg.txt <<'EOF'
docs: E2E High Priority 2件の修正完了に伴いTODO.mdを更新

変更内容:
- docs/todo/TODO.md の「### High Priority」セクションを削除
- 冒頭サマリーを Medium 残課題の状況に合わせて更新

変更理由:
- markdown_links.spec.js:124 と document_search.spec.js:389 の
  修正が本ブランチで完了したため進捗を反映

影響範囲:
- docs/todo/TODO.md のみ

テスト結果: N/A（ドキュメント更新のみ）
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

- [ ] **Step 6: PR 作成前の最終チェック**

Run:
```bash
git log --oneline develop..HEAD
```

Expected: 4 コミット（spec 追加 → Bug #1 fix → Bug #2 test 修正 → TODO.md 更新）。

Run:
```bash
git status
```

Expected: clean（未コミット変更なし）。

---

## Self-Review

以下を著者が確認済み:

**Spec coverage**: Spec の「Bug #1 修正」「Bug #2 修正」「検証」「非対象」すべてが Task 1-3 にマッピング済み。非対象項目（Medium flaky、Low 6件）はプランに含めていないことが意図通り。

**Placeholder scan**: TBD / TODO / "implement later" なし。変更前後のコードスニペット、具体的コマンド、期待出力をすべて本文中に記載済み。

**Type consistency**: Task 間で参照する識別子は `handleInternalLinkClick`、`applyContentAnchorNavigation`、`resolveDirectorySearchIndex`、`getPreferredDirectorySearchSelection`、`currentDirectorySearchIndex` などすべて実ソースと一致。Task 1 / 2 は独立したファイル変更で交差なし。

**Known risk**: `markdown_links.spec.js:41` の flake は Task 3 Step 1 で観測される可能性がある。TODO.md の Medium 扱いで本プランの対象外であり、retry で回復するかを一度確認するガイドを入れている。
