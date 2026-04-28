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
  require_node_modules || return $?
  npx --no-install tsc --noEmit
}

run_playwright_e2e() {
  require_node_modules || return $?
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
