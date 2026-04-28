# verify.sh E2E Opt-In Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `./verify.sh --e2e` の明示指定時だけ Playwright E2E まで実行できるようにし、通常の `./verify.sh` は現状の検証内容を維持する。

**Architecture:** `verify.sh` に小さな引数パーサ、usage、Node 依存確認ヘルパー、E2E 実行関数を追加する。E2E 本体は既存の `npm run test:e2e` に委譲し、Playwright 設定や npm scripts は変更しない。完了後に `docs/todo/BACKLOG.md` の対象項目を完了済みにする。

**Tech Stack:** Bash (`verify.sh`), npm scripts, Playwright, Rust/Cargo verification commands

---

## Files

- Modify: `verify.sh` — `--e2e` / `--help` / 未知引数の処理、E2E 実行関数を追加する。
- Modify: `docs/todo/BACKLOG.md` — `E2E を verify.sh に統合するか検討` を完了済みにする。
- Reference: `package.json` — `npm run test:e2e` が Playwright 実行の入口であることを確認する。
- Reference: `playwright.config.ts` — E2E が `cargo run -- tests/fixtures/e2e --port 4173 --no-open` を起動する前提を確認する。

## Task 1: `verify.sh` に E2E opt-in を追加する

**Files:**
- Modify: `verify.sh`

- [ ] **Step 1: 現在の関連識別子が未実装であることを確認する**

Run:

```bash
rg -n -- "--e2e|show_usage|run_e2e|require_node_modules" verify.sh
```

Expected: no matches. `rg` は exit code 1 でよい。

- [ ] **Step 2: `verify.sh` を完成形に置き換える**

Replace the entire file with:

```bash
#!/usr/bin/env bash
set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

current_step=""
run_e2e=false

show_usage() {
  cat <<'USAGE'
Usage: ./verify.sh [--e2e]

Options:
  --e2e       通常検証の後に Playwright E2E も実行する
  -h, --help  このヘルプを表示する
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --e2e)
      run_e2e=true
      ;;
    -h|--help)
      show_usage
      exit 0
      ;;
    *)
      echo "エラー: 未知のオプションです: $1" >&2
      show_usage >&2
      exit 2
      ;;
  esac
  shift
done

# ERR トラップ: 失敗したステップ名を明示的に報告する
cleanup_on_error() {
  if [[ -n "$current_step" ]]; then
    echo "==> 失敗: ${current_step}" >&2
  fi
}
trap cleanup_on_error ERR

run_step() {
  local step_name="$1"
  shift
  current_step="$step_name"
  echo "==> ${step_name}"
  "$@"
  current_step=""
}

require_node_modules() {
  if [[ ! -d node_modules ]]; then
    echo "エラー: node_modules が存在しません。'npm ci' を先に実行してください。" >&2
    return 1
  fi
}

typecheck_e2e() {
  require_node_modules
  npx --no-install tsc --noEmit
}

run_playwright_e2e() {
  require_node_modules
  npm run test:e2e
}

run_step "フォーマットチェック" cargo fmt --all -- --check
run_step "Lint (clippy)" cargo clippy --all-targets --all-features -- -D warnings
run_step "テスト実行" cargo test --all-targets --all-features
run_step "E2E型チェック (tsc)" typecheck_e2e

if [[ "$run_e2e" == true ]]; then
  run_step "E2E実行 (Playwright)" run_playwright_e2e
fi

echo "==> 検証が正常に完了しました。"
```

- [ ] **Step 3: Bash 構文を検証する**

Run:

```bash
bash -n verify.sh
```

Expected: no output and exit code 0.

- [ ] **Step 4: help が通常検証を走らせず成功することを確認する**

Run:

```bash
./verify.sh --help
```

Expected: exit code 0. Output includes:

```text
Usage: ./verify.sh [--e2e]
  --e2e       通常検証の後に Playwright E2E も実行する
```

- [ ] **Step 5: 未知引数が失敗することを確認する**

Run:

```bash
./verify.sh --unknown
```

Expected: exit code 2. Output includes:

```text
エラー: 未知のオプションです: --unknown
Usage: ./verify.sh [--e2e]
```

- [ ] **Step 6: Commit**

Run:

```bash
git add verify.sh
git commit -m "chore: verifyにE2E opt-inを追加"
```

Expected: commit succeeds with only `verify.sh` staged.

## Task 2: BACKLOG の対象項目を完了済みにする

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 対象項目の現在位置を確認する**

Run:

```bash
rg -n "E2E を `verify.sh` に統合するか検討|npm run test:e2e|verify.sh で Rust server" docs/todo/BACKLOG.md
```

Expected: P1 セクション内に対象項目が表示される。

- [ ] **Step 2: checkbox を完了済みに変える**

Change this line:

```markdown
- [ ] E2E を `verify.sh` に統合するか検討
```

to:

```markdown
- [x] E2E を `verify.sh` に統合するか検討
```

Keep the existing file/content/reason/origin bullets unchanged.

- [ ] **Step 3: 完了化だけが入ったことを確認する**

Run:

```bash
git diff -- docs/todo/BACKLOG.md
```

Expected diff:

```diff
-- [ ] E2E を `verify.sh` に統合するか検討
+- [x] E2E を `verify.sh` に統合するか検討
```

- [ ] **Step 4: Commit**

Run:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: verify e2e統合backlogを完了"
```

Expected: commit succeeds with only `docs/todo/BACKLOG.md` staged.

## Task 3: 最終検証を実行する

**Files:**
- Verify: `verify.sh`
- Verify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 通常検証が従来どおり通ることを確認する**

Run:

```bash
./verify.sh
```

Expected: fmt, clippy, cargo test, E2E typecheck が pass し、最後に次が表示される。

```text
==> 検証が正常に完了しました。
```

- [ ] **Step 2: E2E opt-in 検証を実行する**

Run:

```bash
./verify.sh --e2e
```

Expected when Playwright browsers are installed: fmt, clippy, cargo test, E2E typecheck, Playwright E2E が pass し、最後に次が表示される。

```text
==> E2E実行 (Playwright)
==> 検証が正常に完了しました。
```

If Playwright browser installation is missing, record the Playwright error exactly and stop. Do not change `playwright.config.ts` or install dependencies unless the user approves that environment action.

- [ ] **Step 3: セキュリティ境界に不要な変更がないことを確認する**

Run:

```bash
git diff HEAD~2..HEAD --stat
git diff HEAD~2..HEAD -- playwright.config.ts package.json src
```

Expected: stat includes only `verify.sh` and `docs/todo/BACKLOG.md`. The second command prints no diff.

- [ ] **Step 4: 最終状態を確認する**

Run:

```bash
git status --short --branch
```

Expected: clean working tree on the implementation branch.

## Self-Review Notes

- Spec coverage: `--e2e` opt-in、通常検証維持、help、未知引数、`node_modules` 確認、`npm run test:e2e` 委譲、BACKLOG 完了化、セキュリティ境界維持を Task 1-3 で扱う。
- Placeholder scan: no placeholder text is intentionally left in this plan.
- Type/name consistency: plan uses `run_e2e`, `show_usage`, `require_node_modules`, `typecheck_e2e`, and `run_playwright_e2e` consistently.
