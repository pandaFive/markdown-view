#!/usr/bin/env bash
set -euo pipefail

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

run_step "フォーマットチェック" cargo fmt --all -- --check
run_step "Lint (clippy)" cargo clippy --all-targets --all-features -- -D warnings
run_step "テスト実行" cargo test --all-targets --all-features

echo "==> 検証が正常に完了しました。"
