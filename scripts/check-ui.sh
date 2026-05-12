#!/bin/sh
# Run the PTY-driven UI tests.  These are gated behind the `ui-tests` feature
# so the default `cargo test` and `scripts/check.sh` stay fast.  CI runs the
# equivalent of this in a separate job; this script is here for local use.
set -e

echo '==> ui-tests'
RUST_BACKTRACE=1 cargo test --features ui-tests --tests -- --test-threads=2

echo 'All UI tests passed.'
