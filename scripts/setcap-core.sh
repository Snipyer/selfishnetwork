#!/usr/bin/env bash
# Build the core in release mode and grant it the capabilities it needs to open
# raw sockets, so it can run without sudo afterwards.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/core/target/release/selfishnet-core"

echo "==> Building core (release)…"
( cd "$ROOT/core" && cargo build --release )

echo "==> Granting cap_net_raw,cap_net_admin to the core binary (needs sudo)…"
sudo setcap cap_net_raw,cap_net_admin+ep "$BIN"

echo "==> Done:"
getcap "$BIN"
echo
echo "Run it with:  $BIN"
