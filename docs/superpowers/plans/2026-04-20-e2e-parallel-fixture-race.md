# E2E Parallel Fixture Race Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** E2E flake 全件（並列起因 6+ 件 + CDP 非同期 1 件）を解消し `npx playwright test` 10回連続全 PASS を達成する。

**Architecture:** `playwright.config.js` に `workers: 1` を追加して並列 fixture race を一括解消、`markdown_links.spec.js:341` を `expect.poll()` 化して CDP 非同期 warning event 到達を待つ。最後に TODO.md の Medium 項目を削除する。

**Tech Stack:** Playwright (`@playwright/test`)、JavaScript、Rust (`./verify.sh` 経由のみ無回帰確認)

**Spec:** `docs/superpowers/specs/2026-04-20-e2e-parallel-fixture-race-design.md`

**ブランチ:** 実装は `fix/e2e-parallel-fixture-race`（既に spec コミット済み `1781f1e`）

---

## File Structure

本プランで変更するファイル:

- `playwright.config.js` — top-level config に `workers: 1` を追加（並列 fixture race の根本解消）
- `tests/e2e/markdown_links.spec.js` — L341 のみ `expect.poll()` 化（CDP 非同期 warning event）
- `docs/todo/TODO.md` — Medium「Flaky E2E テスト 3件の安定化」エントリ削除と冒頭サマリーの Medium 言及削除

---

### Task 1: Playwright を `workers: 1` で直列化

**Files:**
- Modify: `playwright.config.js`

- [ ] **Step 1: 現在の flake 再現を確認（Baseline）**

Run:
```bash
npx playwright test --reporter=list 2>&1 | tail -5
```

Expected: 何件か失敗（典型的には 2-4件、ほぼ常に `memo_sync.spec.js:48` が含まれる）。
このベースラインは Step 5 の after 比較に使う。

- [ ] **Step 2: `playwright.config.js` に `workers: 1` を追加**

`playwright.config.js` を以下のように変更する。

変更前:
```js
const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
  testDir: './tests/e2e',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:4173',
    headless: true
  },
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' }
    }
  ],
  webServer: {
    command: 'cargo run -- tests/fixtures/e2e --port 4173 --no-open',
    url: 'http://localhost:4173',
    reuseExistingServer: true,
    timeout: 120000
  }
});
```

変更後（`workers: 1` を `timeout` 行の直後に挿入）:
```js
const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
  testDir: './tests/e2e',
  timeout: 30000,
  // E2E は共通 fixture (tests/fixtures/e2e/{README,notes}.md) を読み書きするため
  // ワーカー並列実行で別 spec が同 fixture を上書きし test pollution を起こす。
  // 直列化で決定論性を確保する。
  workers: 1,
  use: {
    baseURL: 'http://localhost:4173',
    headless: true
  },
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' }
    }
  ],
  webServer: {
    command: 'cargo run -- tests/fixtures/e2e --port 4173 --no-open',
    url: 'http://localhost:4173',
    reuseExistingServer: true,
    timeout: 120000
  }
});
```

挿入位置注意: `timeout: 30000,` の直後に `workers: 1,` を追加し、その上にコメントを 3 行付与する。他のフィールドは一切変更しない。

- [ ] **Step 3: 直列化後の flake 状況を 5回確認**

Run:
```bash
for i in 1 2 3 4 5; do
  npx playwright test --reporter=list 2>&1 > /tmp/serial-task1-$i.log
  fails=$(grep -c "✘" /tmp/serial-task1-$i.log)
  echo "Run $i: $fails 件 failed"
  grep "✘" /tmp/serial-task1-$i.log | head -3
done
```

Expected: 失敗があっても `markdown_links.spec.js:310` のみ（5回中 1-2回）。並列由来の他テスト（`memo_sync.spec.js:48`、`text_selection_defer.spec.js:180/311`、`markdown_links.spec.js:41/62/92/253` 等）は出ないこと。L310 が出る場合は Task 2 で解消するので OK。

- [ ] **Step 4: 並列由来の flake が消えたことを diff で確認**

Step 1 (parallel) と Step 3 (serial) の失敗テスト集合を比較:
```bash
echo "=== Parallel baseline (Step 1) ==="
grep "✘" /tmp/serial-task1-1.log 2>/dev/null || echo "ログなし、Step 1 を再実行してください"
```

Expected: Step 1 baseline に出ていた `memo_sync.spec.js:48` 等が Step 3 では出ない。L310 のみが残候補。

- [ ] **Step 5: コミット**

```bash
git add playwright.config.js
cat > /tmp/commit-msg.txt <<'EOF'
test: PlaywrightのE2E実行を直列化してfixture競合flakeを解消

変更内容:
- playwright.config.js に workers: 1 を追加

変更理由:
- 全 E2E が共通 fixture (tests/fixtures/e2e/{README,notes}.md) を
  beforeEach で書き換えるため、Playwright デフォルトの並列実行で
  別 spec の writeFile が直後の別テストの content を巻き戻す
  test pollution が発生していた
- memo_sync.spec.js:48 (5/5)、text_selection_defer.spec.js:180 (4/5)
  ほか 5+ 件の flake を 1 行設定で一括解消
- ./verify.sh に E2E 組み込み前提で決定論性を優先（実行時間 約20秒→
  約80-120秒の増加は許容）

影響範囲:
- playwright.config.js のみ。テスト本体・プロダクトコード無変更

テスト結果: 直列5回実行で並列起因の flake 全消滅、L310 のみ残（Task 2で対応）
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

---

### Task 2: `markdown_links.spec.js:341` を `expect.poll()` 化

**Files:**
- Modify: `tests/e2e/markdown_links.spec.js:341`

- [ ] **Step 1: L310 単独で flake 再現条件を確認**

L310 はバッチ実行時に flake、単独だと安定。直列バッチ 8回で再現するか確認:
```bash
fail_count=0
for i in $(seq 1 8); do
  result=$(npx playwright test tests/e2e/markdown_links.spec.js --reporter=line 2>&1)
  if echo "$result" | grep -q "✘.*markdown_links.spec.js:310"; then
    fail_count=$((fail_count+1))
    echo "Run $i: L310 FAIL"
  fi
done
echo "L310 batch fails: $fail_count / 8"
```

Expected: 1-3 回程度 L310 が fail（必ず再現するわけではないが概ね 20-40% で発生）。0 件でも次の Step に進む（fix の妥当性は別途検証）。

- [ ] **Step 2: L341 を `expect.poll()` 化**

`tests/e2e/markdown_links.spec.js` の L341 を変更する。

変更前:
```js
  expect(warnings.some((msg) => msg.indexOf('リンク先の見出しが見つかりません') !== -1)).toBe(true);
```

変更後:
```js
  await expect.poll(() => warnings.some((msg) => msg.indexOf('リンク先の見出しが見つかりません') !== -1)).toBe(true);
```

注意:
- 行頭の 2 スペースインデントは維持
- `await` を必ず付ける（付け忘れると polling が同期的に評価されてしまう）
- 同種パターン (L152, L274, L307) は flake 観測なしのため変更しない（YAGNI）

- [ ] **Step 3: L310 が pass に転じることを 8回確認**

Run:
```bash
fail_count=0
for i in $(seq 1 8); do
  result=$(npx playwright test tests/e2e/markdown_links.spec.js --reporter=line 2>&1)
  if echo "$result" | grep -q "✘.*markdown_links.spec.js:310"; then
    fail_count=$((fail_count+1))
    echo "Run $i: L310 STILL FAIL"
  fi
done
echo "After fix L310 batch fails: $fail_count / 8"
```

Expected: 0 / 8（fix が効いていれば flake 完全消滅）。1 件でも残る場合は Step 2 の `await` 付け忘れ等を疑う。

- [ ] **Step 4: 周辺テスト回帰確認**

Run:
```bash
npx playwright test tests/e2e/markdown_links.spec.js --reporter=list 2>&1 | tail -3
```

Expected: 13 件全 PASS（直列モードに変更済みなので他テストは安定しているはず）。

- [ ] **Step 5: コミット**

```bash
git add tests/e2e/markdown_links.spec.js
cat > /tmp/commit-msg.txt <<'EOF'
test: L310のwarning検証をexpect.poll化してCDP非同期遅延に対応

変更内容:
- tests/e2e/markdown_links.spec.js:341 の同期 expect を
  await expect.poll(...) に変更

変更理由:
- popstate 経由の navigation 完了後でも page.on('console') 経由の
  warning イベントが Node 側へ即時到達せず、warnings 配列が空のまま
  assertion が走って失敗するケースがあった（直列実行でも残る唯一の真の flake）
- 5秒 polling で CDP イベント到達を待ち flake を解消
- 同パターンの warning 検証 (L152, L274, L307) は現状 flake 観測なしで
  予防修正は YAGNI として見送り

影響範囲:
- tests/e2e/markdown_links.spec.js のみ
- プロダクトコード変更なし

テスト結果: L310 を 8回連続バッチ実行して 0 / 8 fail
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

---

### Task 3: 全 E2E 10回連続 PASS の最終検証

**Files:** なし（検証のみ、コードは Task 1-2 で完了）

- [ ] **Step 1: 全 E2E を 10回連続実行**

Run:
```bash
total_fails=0
for i in $(seq 1 10); do
  npx playwright test --reporter=list 2>&1 > /tmp/final-$i.log
  fails=$(grep -c "✘" /tmp/final-$i.log)
  total_fails=$((total_fails + fails))
  if [ $fails -gt 0 ]; then
    echo "Run $i: $fails 件 failed"
    grep "✘" /tmp/final-$i.log | head -3
  else
    echo "Run $i: PASS"
  fi
done
echo
echo "Total fails over 10 runs: $total_fails"
```

Expected: `Total fails over 10 runs: 0`。1 件でも失敗が出たら原因を特定:
- L310 系 → Task 2 の修正が不十分
- 別テスト → 未知の race の可能性、ログを保存してユーザに報告（BLOCKED）

- [ ] **Step 2: `./verify.sh` で Rust 全検証**

Run:
```bash
./verify.sh
```

Expected: PASS（fmt / clippy / cargo test 全 PASS、本変更は JS/設定のみで Rust 影響なし）。

- [ ] **Step 3: 実行時間の体感差をログに残す**

Run:
```bash
{ time npx playwright test --reporter=line ; } 2>&1 | tail -5
```

Expected: real 時間が表示される。直列化前 ~20秒、直列化後 ~80-120秒の範囲を想定。Step 1 / 2 / 3 すべて pass であれば次の Task 4 に進む（時間ログ自体はコミット不要、報告に含めるのみ）。

---

### Task 4: TODO.md の Medium 項目削除

**Files:**
- Modify: `docs/todo/TODO.md` L3-L19（Medium「Flaky E2E テスト 3件の安定化」と冒頭サマリー）

- [ ] **Step 1: 現状の TODO.md を確認**

Run:
```bash
sed -n '1,20p' docs/todo/TODO.md
```

Expected: L3-L5 に E2E テスト失敗サマリー、L7 に `### Medium Priority`、L9-L19 に「Flaky E2E テスト 3 件の安定化」エントリが表示される。

- [ ] **Step 2: TODO.md を編集**

`docs/todo/TODO.md` を以下のように編集する。

**変更①**: L3-L5 の冒頭サマリー

変更前:
```markdown
## E2E テスト失敗（2026-04-20 発見）

`npx playwright test` 全体実行で当初 5 件の失敗を確認。うち High Priority 2件は修正済み（本ブランチ）。残る Medium の flake は `./verify.sh` に E2E を組み込む前提で対応する。
```

変更後（セクション全体ごと削除し、続く Medium セクションも削除）:
```markdown
```

つまり L3-L19 までの「## E2E テスト失敗」セクション全体を空に置き換える。具体的には次の Step で示す Edit を 1 回で行う。

**変更②**: 削除対象（L3-L19、空行含む）:

```markdown
## E2E テスト失敗（2026-04-20 発見）

`npx playwright test` 全体実行で当初 5 件の失敗を確認。うち High Priority 2件は修正済み（本ブランチ）。残る Medium の flake は `./verify.sh` に E2E を組み込む前提で対応する。

### Medium Priority

- [ ] Flaky E2E テスト 3 件の安定化
  - ファイル:
    - `tests/e2e/markdown_links.spec.js:41` ディレクトリモード相対リンクフラグメント遷移
    - `tests/e2e/markdown_links.spec.js:310` popstateで別ファイル壊れたフラグメントに戻っても履歴エントリのhashは破壊しない
    - `tests/e2e/memo_sync.spec.js:48` 別ページへのメモ更新同期
  - 症状: 同じコマンドを連続実行すると pass/fail が揺れる
  - 再現: `npx playwright test tests/e2e/markdown_links.spec.js:41` を複数回実行
  - 方針: `test.retry(2)` で許容せず、待機条件（selector の安定化 / broadcast 到達確認）を特定して根治
  - 理由: `./verify.sh` に E2E を組み込む前提で flake は許容しない
  - 優先度: Medium
  - 備考: L310 は 2026-04-20 PR #78 検証中に観測（1 回再実行で pass）

```

削除後、L1-L2 の `# TODO Issues` 直下に L21 以降の `## TODO Issues (レビュー日: 2026-04-20, PR #76 レビュー)` が続く構造になる（空行 1 つを残して接続）。

Read で現状を確認してから Edit ツールで上記ブロックを削除する。

- [ ] **Step 3: 変更差分を確認**

Run:
```bash
git diff docs/todo/TODO.md
```

Expected: E2E テスト失敗セクション (`## E2E テスト失敗（2026-04-20 発見）` から Medium エントリ末尾までの計 17 行) が削除されている。PR #76, #77 レビュー由来の Low セクションは無変更。

- [ ] **Step 4: コミット**

```bash
git add docs/todo/TODO.md
cat > /tmp/commit-msg.txt <<'EOF'
docs: E2E flake全件解消に伴いTODO.mdからMediumエントリを削除

変更内容:
- docs/todo/TODO.md から「## E2E テスト失敗（2026-04-20 発見）」
  セクションを削除（冒頭サマリー + Medium「Flaky E2E テスト 3件の安定化」
  エントリを含む計 17 行）

変更理由:
- workers=1 設定と L310 の expect.poll 化により全 flake が解消し
  該当 TODO は完了したため

影響範囲:
- docs/todo/TODO.md のみ
- PR #76 / #77 レビュー由来の Low セクションは無変更

テスト結果: N/A（ドキュメント整理のみ）
EOF
git commit -F /tmp/commit-msg.txt
rm /tmp/commit-msg.txt
```

- [ ] **Step 5: PR 作成前の最終確認**

Run:
```bash
git log --oneline develop..HEAD
git status
```

Expected:
- `git log` で 4 コミット表示（spec → workers=1 → expect.poll → TODO.md 削除）
- `git status` clean

---

## Self-Review

- **Spec coverage**: Spec の「修正 1 (workers=1)」「修正 2 (expect.poll)」「修正 3 (TODO.md)」「検証 (10回連続 PASS)」「verify.sh 確認」「実行時間共有」すべてに対応する Task が存在。
- **Placeholder scan**: TBD / TODO / "implement later" なし。code block / 期待出力 / コマンドすべて具体記載。
- **Type consistency**: 関連識別子 `workers`、`expect.poll`、`warnings.some(...)`、`toBe(true)` などすべて Playwright 公式 API と一致。
- **Known risk**: Task 3 Step 1 で 10回中 1件でも fail した場合の対処を BLOCKED で明記済み（未知 race の場合）。
