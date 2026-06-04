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

check_inline_js_generated() {
  require_node_modules || return $?
  npm run check:inline-js-generated
}

run_playwright_e2e() {
  require_node_modules || return $?
  npm run test:e2e
}

check_search_rss_measurement_script() {
  node --check scripts/measure-search-rss-plateau.mjs
  node scripts/measure-search-rss-plateau.mjs --help >/dev/null
  node scripts/measure-search-rss-plateau.mjs --self-test-sanitization >/dev/null
}

check_appmode_toctou_regression() {
  local pattern='\.is_(file|dir)\(\)'
  local allowed_pattern='(^|[^[:alnum:]_])metadata\.file_type\(\)\.is_(file|dir)\(\)'
  local files=(src/server/state.rs src/main.rs)
  local matches
  local search_status
  if command -v rg >/dev/null 2>&1; then
    set +e
    matches="$(rg -n "$pattern" "${files[@]}")"
    search_status=$?
    set -e
  else
    set +e
    matches="$(grep -En "$pattern" "${files[@]}")"
    search_status=$?
    set -e
  fi

  if [[ $search_status -eq 1 ]]; then
    return 0
  fi
  if [[ $search_status -ne 0 ]]; then
    echo "エラー: AppMode TOCTOU回帰チェックの検索に失敗しました。" >&2
    return 1
  fi

  local forbidden_matches
  local local_without_allowed
  forbidden_matches="$(
    while IFS= read -r match_line; do
      local_without_allowed="$(printf '%s\n' "$match_line" | sed -E "s/$allowed_pattern//g")"
      if printf '%s\n' "$local_without_allowed" | grep -Eq "$pattern"; then
        printf '%s\n' "$match_line"
      fi
    done <<<"$matches"
  )"
  if [[ -n "$forbidden_matches" ]]; then
    printf '%s\n' "$forbidden_matches"
    echo "エラー: AppModeの種別判定にis_file()/is_dir()が再導入されています。" >&2
    return 1
  fi
}

run_step "Node依存確認" require_node_modules
run_step "フォーマットチェック" cargo fmt --all -- --check
run_step "Lint (clippy)" cargo clippy --all-targets --all-features -- -D warnings
run_step "AppMode TOCTOU回帰チェック" check_appmode_toctou_regression
run_step "検索RSS計測スクリプトチェック" check_search_rss_measurement_script
run_step "テスト実行" cargo test --all-targets --all-features
run_step "リリースビルドテスト実行" cargo test --all-targets --all-features --release
run_step "E2E型チェック (tsc)" typecheck_e2e
run_step "inline JS fallback同期チェック" check_inline_js_generated

if [[ "$run_e2e" == true ]]; then
  run_step "E2E実行 (Playwright)" run_playwright_e2e
fi

echo "==> 検証が正常に完了しました。"
