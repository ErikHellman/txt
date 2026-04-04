#!/bin/sh
# Run the same checks as CI. Exit non-zero on first failure.
set -e

echo '==> fmt'
cargo fmt --all -- --check

echo '==> build'
RUSTFLAGS="-D warnings" cargo build --all-targets

echo '==> clippy'
RUSTFLAGS="-D warnings" cargo clippy --all-targets -- -D warnings

echo '==> test'
cargo test --all-targets

echo 'All checks passed.'
