#!/usr/bin/env bash
# Runs every required check. Exits non-zero on the first failure.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> rust: fmt"
(cd "$root/rust" && cargo fmt --all --check)
echo "==> rust: clippy"
(cd "$root/rust" && cargo clippy --workspace --all-targets --all-features -- -D warnings)
echo "==> rust: test"
if [ -z "${DATABASE_URL:-}" ]; then
  echo "    note: DATABASE_URL is not set; undrly-store database tests are skipped"
  echo "          (they report as passed). CI runs them with UNDRLY_REQUIRE_DATABASE=1."
fi
(cd "$root/rust" && cargo test --workspace)

echo "==> typescript: install"
(cd "$root/typescript" && bun install --frozen-lockfile)
echo "==> typescript: typecheck"
(cd "$root/typescript" && bun run typecheck)
echo "==> typescript: lint"
(cd "$root/typescript" && bun run lint)
echo "==> typescript: test (bun)"
(cd "$root/typescript" && bun test)
echo "==> typescript: test (vitest)"
(cd "$root/typescript" && bun run test)

echo "all checks passed"
