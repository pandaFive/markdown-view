# updateContent 契約違反 warn と cache invariant 逆方向 E2E テスト Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `src/template/assets/js/content.js` の `updateContent` に契約違反 warn を追加し、`tests/e2e/memo_jump.spec.ts` に warn 捕捉テストと cache invariant 逆方向 E2E テストの 2 本を追加する。

**Architecture:** `data.content === undefined` または `data.toc === undefined` を契約違反として `console.warn` するブロックを `updateContent` 関数冒頭に追加する。副作用は維持し、欠落 field を join した 1 件の warn を出す。E2E テストは Playwright の `page.on('console')` で warn を捕捉、`window.updateContent` 直接呼び出しで cache 不変条件の双方向（一致 no-op / 不一致 再描画）を sentinel ハイライトで検証する。

**Tech Stack:** JavaScript（content.js は ES5 互換）、TypeScript strict（E2E）、Playwright、`page.on('console')`、`page.evaluate`

**設計書:** `docs/superpowers/specs/2026-04-27-update-content-contract-and-inverse-test-design.md`

**ブランチ:** `feat/update-content-contract-and-inverse-test`（設計書コミット済み）

---

## ファイル構造

| 種別 | パス | 役割 |
|------|------|------|
| Modify | `src/template/assets/js/content.js` | `updateContent` 冒頭に契約違反 warn ブロックを追加 |
| Modify | `tests/e2e/memo_jump.spec.ts` | (a) L7 の型宣言修正 (b) warn 捕捉テスト追加 (c) inverse case テスト追加 |
| Modify | `docs/todo/TODO.md` | 該当 2 件の `[ ]` → `[x]` |

すべて既存ファイルへの追記または小修正。新規ファイルなし。

---

## TDD 戦略についての注記

仕様書のコミット計画はコミット 1 に warn 実装、コミット 2 に warn テストと分離する。本 plan では **impl-first** で進める：

1. warn 実装と型修正を先に書く（commit 1）
2. warn 捕捉テストを書く → 即 pass（commit 2）

通常の TDD（test-first）から逸脱する理由：
- warn 実装は 5 行の単純な配列 push + 条件分岐で、TDD の核である「test で設計を駆動する」価値が低い
- 仕様書で承認されたコミット分割を維持するため
- 同一ファイル内の type 修正と test 追加を partial-staging で分離するのは executor の認知負荷が高い

回帰検出力の確保: Task 2 で warn 捕捉テストを書いた後、**手動で warn ブロックを一時的にコメントアウトして fail することを確認**する mutation 検証ステップを必ず実行する。これで「impl が正しく機能していなかったら test が落ちる」ことを担保する。

---

### Task 1: warn 実装と型宣言修正（コミット 1: feat）

**Files:**
- Modify: `src/template/assets/js/content.js`（`updateContent` 冒頭、現状 L1226-1232 周辺）
- Modify: `tests/e2e/memo_jump.spec.ts`（L7 の型宣言）

- [ ] **Step 1: 既存コードを確認**

`src/template/assets/js/content.js` の `updateContent` 関数の現状を確認する。

Run: `grep -n "function updateContent" src/template/assets/js/content.js`
Expected: `1226:function updateContent(data, options) {`

ファイル冒頭付近 L1226-1248 を Read で確認し、`options = options || {};` の位置と `pendingUpdateTimer` クリアの位置を把握する。

- [ ] **Step 2: 既存 E2E 型宣言を確認**

Run: `grep -n "updateContent:" tests/e2e/memo_jump.spec.ts`
Expected: `7:    updateContent: (data: { content: string; toc?: string }, opts: Record<string, unknown>) => void;`

- [ ] **Step 3: `src/template/assets/js/content.js` に warn ブロックを追加**

`function updateContent(data, options) {` の直後、`options = options || {};` の次の行に以下のブロックを挿入する。

挿入前（L1226-1232 周辺）:
```javascript
function updateContent(data, options) {
  options = options || {};
  if (pendingUpdateTimer) {
    clearTimeout(pendingUpdateTimer);
    pendingUpdateTimer = null;
  }
```

挿入後:
```javascript
function updateContent(data, options) {
  options = options || {};

  // UpdateMessage (src/template/message.rs) の content/toc には skip_serializing_if が
  // 付いていないため、サーバー契約として両 field は常に存在する。欠落は中継プロキシ
  // 改変やサーバー実装の契約違反のサインで、サイレントに no-op になるとデバッグ困難。
  // warn を出した上で、TOC 更新等の既存副作用は早期 return せず継続する（部分回復ケース許容）。
  var missing = [];
  if (data.content === undefined) missing.push('content');
  if (data.toc === undefined) missing.push('toc');
  if (missing.length > 0) {
    console.warn('[markdown-view] updateContent: ' + missing.join(', ') + ' が欠落 (契約違反)', data);
  }

  if (pendingUpdateTimer) {
    clearTimeout(pendingUpdateTimer);
    pendingUpdateTimer = null;
  }
```

注意:
- ES5 互換維持（`var` 使用、アロー関数禁止、`const`/`let` 禁止）
- 既存の `if (pendingUpdateTimer)` 以降は変更しない

- [ ] **Step 4: `tests/e2e/memo_jump.spec.ts:7` の型宣言を修正**

L7 を以下に変更:

変更前:
```typescript
    updateContent: (data: { content: string; toc?: string }, opts: Record<string, unknown>) => void;
```

変更後:
```typescript
    updateContent: (data: { content: string; toc: string }, opts: Record<string, unknown>) => void;
```

理由: サーバー契約として toc も必須なので、型宣言を契約と一致させる。

- [ ] **Step 5: 既存テストが破壊されていないことを確認**

Run: `./verify.sh`

Expected:
- `cargo fmt`: pass
- `cargo clippy`: pass
- `cargo test`: 全件 pass
- `npm run typecheck`: pass

警告が出ても既存挙動に影響なし（warn は契約違反時のみ発火、現状運用では発火しない）。

- [ ] **Step 6: 既存 E2E テストが pass することを確認**

Run: `npm run test:e2e -- --grep "同じdata.contentでの2回目updateContent"`

Expected: 1 件 pass。
- 既存テストは `data.content`/`data.toc` 両方を渡すため warn は発火しない。
- 型宣言修正は実行時挙動に影響しない（TypeScript の型チェックのみ）。

- [ ] **Step 7: コミット 1 (feat)**

```bash
git add src/template/assets/js/content.js tests/e2e/memo_jump.spec.ts
git diff --cached --stat
```

Expected: 2 files changed (content.js: +9 行程度、memo_jump.spec.ts: +1 -1 行)。

```bash
cat > /tmp/commit-msg.txt <<'EOF'
feat: updateContentにdata.content/toc欠落時の契約違反warnを追加

変更内容:
- src/template/assets/js/content.js の updateContent 冒頭に missing field を集めて 1 件の console.warn を出すブロックを追加
- tests/e2e/memo_jump.spec.ts の window.updateContent 型宣言を toc?: string → toc: string に修正

変更理由:
- UpdateMessage (src/template/message.rs) の content/toc には skip_serializing_if が付かないため、サーバー契約として両 field は常に serialize される
- 中継プロキシ改変やサーバー契約違反でこれらが欠落するとサイレントな no-op となりデバッグ困難
- 契約違反を warn で観測できるようにする
- TS 型宣言を契約と一致させる

影響範囲:
- 正常運用では warn は発火しないため挙動互換
- 既存テスト全件 pass

テスト結果: ./verify.sh + 既存 E2E pass
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

Expected: コミット成功、`feat: updateContentに...` のコミットが作られる。

---

### Task 2: warn 捕捉 E2E テスト追加（コミット 2: test）

**Files:**
- Modify: `tests/e2e/memo_jump.spec.ts`（末尾、既存最終 test の後）

- [ ] **Step 1: 既存テストファイル末尾の構造を確認**

Run: `tail -30 tests/e2e/memo_jump.spec.ts`

Expected: 既存最終テスト「同じdata.contentでの2回目updateContent...」が L437-482 で終わり、その後に閉じ `}` または空行があることを確認。

- [ ] **Step 2: warn 捕捉テストを末尾に追加**

`tests/e2e/memo_jump.spec.ts` の末尾（既存最終 test の後、ファイルの末尾）に以下を追加:

```typescript

test('updateContentはdata.content/toc欠落時に契約違反warnを出す', async ({ page }) => {
  // beforeEach で /?file=long.md へ goto 済み
  await expect(page.locator('#content h2').first()).toBeVisible();

  // page.on('console') で warn を蓄積。test 中の page.evaluate 内で起きた warn は
  // Playwright 経由で配信されるため waitForTimeout で flush を待つ。
  const warnings: string[] = [];
  page.on('console', (msg) => {
    if (msg.type() === 'warning') warnings.push(msg.text());
  });

  // ケース 1: 正常呼び出し → warn は出ない
  await page.evaluate(() => {
    window.updateContent({ content: '<p>ok</p>', toc: '<ul></ul>' }, {});
  });

  // ケース 2: content だけ欠落 → 'content が欠落' warn
  await page.evaluate(() => {
    // 契約違反呼び出しを意図的に再現するため as any でキャストする
    window.updateContent({ toc: '<ul></ul>' } as unknown as { content: string; toc: string }, {});
  });

  // ケース 3: toc だけ欠落 → 'toc が欠落' warn
  await page.evaluate(() => {
    window.updateContent({ content: '<p>ok</p>' } as unknown as { content: string; toc: string }, {});
  });

  // ケース 4: content と toc 両方欠落 → 'content, toc が欠落' warn
  await page.evaluate(() => {
    window.updateContent({} as unknown as { content: string; toc: string }, {});
  });

  // console イベントは page → test runner へ非同期配信されるため flush を待つ
  await page.waitForTimeout(50);

  const contractWarnings = warnings.filter((w) =>
    w.includes('updateContent') && w.includes('契約違反')
  );

  expect(contractWarnings).toHaveLength(3);
  expect(contractWarnings[0]).toContain('content が欠落');
  expect(contractWarnings[0]).not.toContain('toc'); // ケース 2 は content のみ
  expect(contractWarnings[1]).toContain('toc が欠落');
  expect(contractWarnings[1]).not.toContain('content,'); // ケース 4 ('content, toc') との混同防止
  expect(contractWarnings[2]).toContain('content, toc が欠落');
});
```

注意点:
- `as unknown as { content: string; toc: string }` のダブルキャスト: 直接 `as any` は ESLint で warning が出る可能性、`as unknown as T` 経由が strict 設定下で標準的
- 末尾改行を 1 行残す（POSIX 準拠）

- [ ] **Step 3: 型チェックを実行**

Run: `npm run typecheck`

Expected: pass。`as unknown as` キャストで型エラーは出ない。

- [ ] **Step 4: warn 捕捉テスト単体実行**

Run: `npm run test:e2e -- --grep "updateContentはdata.content/toc欠落時に契約違反warnを出す"`

Expected: 1 件 pass。`contractWarnings` が 3 件、各メッセージが期待内容を含む。

- [ ] **Step 5: mutation 検証 — warn ブロック無効化で fail することを確認**

このステップで「テストが impl の有無を実際に検出できる」ことを確認する。

`src/template/assets/js/content.js` の Step 3 で追加した warn ブロックを **一時的に** コメントアウト:

```javascript
function updateContent(data, options) {
  options = options || {};

  // var missing = [];
  // if (data.content === undefined) missing.push('content');
  // if (data.toc === undefined) missing.push('toc');
  // if (missing.length > 0) {
  //   console.warn('[markdown-view] updateContent: ' + missing.join(', ') + ' が欠落 (契約違反)', data);
  // }

  if (pendingUpdateTimer) {
```

Run: `npm run test:e2e -- --grep "updateContentはdata.content/toc欠落時に契約違反warnを出す"`

Expected: **FAIL**。`expect(contractWarnings).toHaveLength(3)` が `Received: []` で失敗する。これにより impl が無いと test が落ちることを確認できた。

- [ ] **Step 6: warn ブロックを復元**

Step 5 でコメントアウトした 6 行を元に戻す。

Run: `git diff src/template/assets/js/content.js`
Expected: 差分なし（コミット済みの状態と一致）。

Run: `npm run test:e2e -- --grep "updateContentはdata.content/toc欠落時に契約違反warnを出す"`
Expected: 再度 pass。

- [ ] **Step 7: 既存テスト全件回帰チェック**

Run: `npm run test:e2e`

Expected: 全件 pass。新テストが他テストへ漏出していないこと、`page.on('console')` の listener が後続テストへ影響していないことを確認（Playwright の test 間 page reset で防御済み）。

- [ ] **Step 8: コミット 2 (test)**

```bash
git add tests/e2e/memo_jump.spec.ts
git diff --cached --stat
```

Expected: 1 file changed, +40 行程度（warn 捕捉テスト追加分）。

```bash
cat > /tmp/commit-msg.txt <<'EOF'
test: updateContent契約違反warnのE2Eテストを追加

変更内容:
- tests/e2e/memo_jump.spec.ts に warn 捕捉 E2E テストを追加
- page.on('console') で warning を捕捉し、4 ケース (正常 / content 欠落 / toc 欠落 / 両方欠落) で warn の発火条件と内容を検証

変更理由:
- 直前のコミットで追加した契約違反 warn ロジックの回帰防止
- リグレッションで warn が消えても気付けるようにする

影響範囲:
- E2E テスト追加のみ
- workers=1 で test 間隔離されているため他テストへの影響なし

テスト結果: 新テスト pass + mutation 検証 (warn 無効化で FAIL) + 既存 E2E 全件 pass
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

Expected: コミット成功。

---

### Task 3: inverse case E2E テスト追加（コミット 3: test）

**Files:**
- Modify: `tests/e2e/memo_jump.spec.ts`（warn 捕捉テストの直後）

- [ ] **Step 1: inverse case テストを追加**

`tests/e2e/memo_jump.spec.ts` の Task 2 で追加した warn 捕捉テストの **直後** に以下を追加:

```typescript

test('data.contentが変わるとupdateContentは再描画される (cache invariantの逆方向)', async ({ page }) => {
  // beforeEach で /?file=long.md へ goto 済み
  await expect(page.locator('#content h2').first()).toBeVisible();

  // Step 1: 実 fetch で prime → lastAppliedContent に実 HTML を入れる。
  // 既存テスト「同じdata.contentでの2回目...」と同形のエラーメッセージ prefix で識別性を維持。
  const primeContentLen = await page.evaluate(async () => {
    const res = await fetch('/api/content?file=long.md');
    if (!res.ok) throw new Error('prime fetch failed: ' + res.status);
    const data = await res.json();
    if (typeof data.content !== 'string') throw new Error('prime data.content missing');
    window.updateContent(data, {});
    return data.content.length;
  });
  expect(primeContentLen).toBeGreaterThan(0);

  // Step 2: 再描画 sentinel として .jump-highlight を付与
  await page.evaluate(() => {
    const h = document.querySelector('#content h2')!;
    h.classList.add('jump-highlight');
  });

  // Step 3: ダミー HTML で updateContent → cache 不一致で再描画される
  // toc も <ul></ul> を渡して契約違反 warn が出ないようにする
  const DUMMY = '<h1 data-test-changed>changed content</h1>';
  await page.evaluate((dummy) => {
    window.updateContent({ content: dummy, toc: '<ul></ul>' }, {});
  }, DUMMY);

  const afterRerender = await page.evaluate(() => ({
    highlighted: !!document.querySelector('#content .jump-highlight'),
    hasDummy: !!document.querySelector('#content [data-test-changed]'),
  }));
  expect(afterRerender.highlighted, 'cache 不一致時に再描画されず .jump-highlight が残存').toBe(false);
  expect(afterRerender.hasDummy, 'ダミー HTML が反映されていない').toBe(true);

  // Step 4: 再度 sentinel 付与 → 同一ダミー HTML で updateContent → cache 一致で no-op
  // lastAppliedContent が新値で更新されたことの逆方向検証
  await page.evaluate(() => {
    const h = document.querySelector('#content [data-test-changed]')!;
    h.classList.add('jump-highlight');
  });
  await page.evaluate((dummy) => {
    window.updateContent({ content: dummy, toc: '<ul></ul>' }, {});
  }, DUMMY);

  const afterNoOp = await page.evaluate(() =>
    !!document.querySelector('#content .jump-highlight')
  );
  expect(afterNoOp, 'cache 更新後の同一呼び出しが no-op にならず再描画された').toBe(true);
});
```

注意点:
- `data-test-changed` 属性で long.md fixture の見出しと識別を分ける
- toc は `<ul></ul>` を渡し、Task 1 で追加した契約違反 warn を発火させない
- `as` キャスト不要（content と toc 両方 string で型一致）

- [ ] **Step 2: 型チェック実行**

Run: `npm run typecheck`
Expected: pass

- [ ] **Step 3: inverse case テスト単体実行**

Run: `npm run test:e2e -- --grep "data.contentが変わるとupdateContentは再描画される"`
Expected: 1 件 pass。Step 3 で `.jump-highlight` が消え、`[data-test-changed]` が現れ、Step 4 で `.jump-highlight` が残る。

- [ ] **Step 4: mutation 検証 — cache 比較条件を一時的に逆転して fail することを確認**

`src/template/assets/js/content.js` の cache 比較条件を **一時的に**逆転（`!==` を `===` に変える）:

L1242 周辺:
```javascript
// 変更前 (現状)
if (data.content !== undefined && data.content !== lastAppliedContent) {

// 変更後 (mutation 検証用)
if (data.content !== undefined && data.content === lastAppliedContent) {
```

Run: `npm run test:e2e -- --grep "data.contentが変わるとupdateContentは再描画される"`

Expected: **FAIL**。Step 3 で再描画されないため `.jump-highlight` が残り、`afterRerender.highlighted` が `true` になり assertion 失敗。

これで「cache 比較ロジックの逆転を逆方向テストが検出できる」ことを確認できた。

- [ ] **Step 5: mutation を元に戻す**

L1242 周辺の `===` を `!==` に戻す。

Run: `git diff src/template/assets/js/content.js`
Expected: 差分なし。

Run: `npm run test:e2e -- --grep "data.contentが変わるとupdateContentは再描画される"`
Expected: 再度 pass。

- [ ] **Step 6: 既存テスト全件回帰チェック**

Run: `npm run test:e2e`
Expected: 全件 pass。

- [ ] **Step 7: コミット 3 (test)**

```bash
git add tests/e2e/memo_jump.spec.ts
git diff --cached --stat
```
Expected: 1 file changed, +45 行程度。

```bash
cat > /tmp/commit-msg.txt <<'EOF'
test: updateContentのcache invariant逆方向 (再描画) E2Eテストを追加

変更内容:
- tests/e2e/memo_jump.spec.ts に「data.content が変わったら必ず再描画される」逆方向 E2E テストを追加
- 4 ステップ構成: 実 fetch で prime → sentinel ハイライト付与 → ダミー HTML で updateContent → 再描画検証 → 同一ダミーで no-op 検証

変更理由:
- 既存テストは cache 一致時の no-op だけを検証していた
- cache 比較条件が逆転する書き換えや常に false 化する誤りを既存 suite では検出できなかった
- 双方向の cache invariant を担保する

影響範囲:
- E2E テスト追加のみ
- workers=1 で test 間隔離されているため他テストへの影響なし

テスト結果: 新テスト pass + mutation 検証 (cache 比較逆転で FAIL) + 既存 E2E 全件 pass
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

Expected: コミット成功。

---

### Task 4: TODO.md 更新（コミット 4: chore）

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: 該当 2 件の現在状態を確認**

Run: `grep -n "updateContent" docs/todo/TODO.md`

Expected: 2 件、共に `- [ ]` で始まる。

- [ ] **Step 2: 2 件を `[x]` に更新**

`docs/todo/TODO.md` 内、以下 2 行を `- [ ]` から `- [x]` に変更:

1. `- [ ] updateContent の inverse case (file-switch / data.content 変更時) の再描画検証`
2. `- [ ] updateContent で data.content === undefined を契約違反として明示ログ`

注意:
- 行番号は `grep -n "updateContent"` で動的に確認すること（手動の行番号特定はファイル更新で陳腐化する）
- 同じ tier 内の他項目は変更しない

- [ ] **Step 3: TODO #3（`build_lagged_recovery_message`）が `[ ]` のままであることを確認**

Run: `grep -n "build_lagged_recovery_message" docs/todo/TODO.md`

Expected: `- [ ] build_lagged_recovery_message ...` が残っている。これは「将来の宿題」として保持。

- [ ] **Step 4: コミット 4 (chore)**

```bash
git add docs/todo/TODO.md
git diff --cached
```
Expected: 2 行の `- [ ]` → `- [x]` 差分のみ。

```bash
cat > /tmp/commit-msg.txt <<'EOF'
chore: TODO.mdのupdateContent関連完了項目を更新

変更内容:
- updateContent の inverse case 再描画検証 を [x] に更新
- updateContent の data.content 欠落時の契約違反ログ を [x] に更新

変更理由:
- 直前 3 コミットで実装完了したため

影響範囲:
- ドキュメントのみ

テスト結果: N/A
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

Expected: コミット成功。

---

### Task 5: 全体検証

- [ ] **Step 1: コミット履歴を確認**

Run: `git log --oneline origin/develop..HEAD`

Expected: 5 件（設計書 1 + セルフレビュー 1 + 本実装 4）。新規実装側は以下:
- `chore: TODO.mdのupdateContent関連完了項目を更新`
- `test: updateContentのcache invariant逆方向 (再描画) E2Eテストを追加`
- `test: updateContent契約違反warnのE2Eテストを追加`
- `feat: updateContentにdata.content/toc欠落時の契約違反warnを追加`

- [ ] **Step 2: `./verify.sh` を実行**

Run: `./verify.sh`

Expected:
- `cargo fmt --all -- --check`: pass
- `cargo clippy --all-targets --all-features -- -D warnings`: pass
- `cargo test --all-targets --all-features`: 全件 pass
- `npm run typecheck`: pass

- [ ] **Step 3: E2E 全件実行**

Run: `npm run test:e2e`

Expected: 全件 pass（新規 2 件 + 既存全件）。

- [ ] **Step 4: 新規 2 件のテストが期待通り pass することを再確認**

Run: `npm run test:e2e -- --grep "updateContent"`

Expected:
- `updateContentはdata.content/toc欠落時に契約違反warnを出す` pass
- `data.contentが変わるとupdateContentは再描画される (cache invariantの逆方向)` pass
- `同じdata.contentでの2回目updateContentは.jump-highlightを消さない` pass（既存）

- [ ] **Step 5: 完了報告**

ユーザーへの完了報告に含める内容:
- 変更ファイル一覧（content.js / memo_jump.spec.ts / TODO.md）
- 各コミットの hash と subject
- `./verify.sh` 結果
- `npm run test:e2e` 結果
- 残 TODO（#3 は引き続き「将来の宿題」、`waitForTimeout(50)` の flake 監視）
- 次の TODO 候補: PR 作成（`gh pr create` で develop 向け）

---

## 完了条件

- 4 つのコミットがブランチ `feat/update-content-contract-and-inverse-test` に存在する
- `./verify.sh` が全件 pass
- `npm run test:e2e` が全件 pass
- mutation 検証で「impl 無効化 → test FAIL」が確認済み（Task 2 Step 5、Task 3 Step 4）
- TODO.md の該当 2 件が `[x]`、TODO #3 は `[ ]` のまま
