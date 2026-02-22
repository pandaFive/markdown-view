#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

run_step() {
  local step_name="$1"
  shift
  echo "==> ${step_name}"
  "$@"
}

run_step "Format check" cargo fmt --all -- --check
run_step "Lint (clippy)" cargo clippy --all-targets --all-features -- -D warnings
run_step "Tests" cargo test --all-targets --all-features

echo "==> Verification completed successfully."
