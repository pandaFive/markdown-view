#!/usr/bin/env bash
set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

current_step=""

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

typecheck_e2e() {
  if [[ ! -d node_modules ]]; then
    echo "エラー: node_modules が存在しません。'npm ci' を先に実行してください。" >&2
    return 1
  fi
  npx --no-install tsc --noEmit
}

run_step "フォーマットチェック" cargo fmt --all -- --check
run_step "Lint (clippy)" cargo clippy --all-targets --all-features -- -D warnings
run_step "テスト実行" cargo test --all-targets --all-features
run_step "E2E型チェック (tsc)" typecheck_e2e

echo "==> 検証が正常に完了しました。"
