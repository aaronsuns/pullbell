#!/usr/bin/env bash
# Format, lint, typecheck and test everything. CI runs exactly this.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$*"; }

[ -d node_modules ] || { step "npm ci"; npm ci; }
# tauri-build needs the frontend bundle to exist when compiling the crate.
[ -f dist/index.html ] || { step "vite build"; npx vite build; }

step "rustfmt";   (cd src-tauri && cargo fmt --all -- --check)
step "clippy";    (cd src-tauri && cargo clippy --all-targets --locked -- -D warnings)
step "tsc";       npx tsc --noEmit
step "vitest";    npx vitest run
step "cargo test"; (cd src-tauri && cargo test --locked)

printf '\nAll checks passed.\n'
