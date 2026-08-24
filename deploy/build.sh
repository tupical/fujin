#!/usr/bin/env bash
# Release build with the git SHA stamped into /healthz (drift-check relies on it).
# Deploy stays manual (stop-before-cp swap on the VPS) — always build via this
# script, not bare `cargo build`, or prod reports git_sha="dev".
set -euo pipefail
cd "$(dirname "$0")/.."
GIT_SHA="$(git rev-parse --short HEAD)" cargo build --release -p fujin-server
echo "built target/release/fujin-server @ $(git rev-parse --short HEAD)"
