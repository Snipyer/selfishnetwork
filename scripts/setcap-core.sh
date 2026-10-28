#!/usr/bin/env bash
# Build the core in release mode and grant it the capabilities it needs to open
# raw sockets, so it can run without sudo afterwards.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/core/target/release/selfishnet-core"

echo "==> Building core (release)…"
( cd "$ROOT/core" && cargo build --release )

# cap_net_raw  — open AF_PACKET raw sockets (capture/inject).
# cap_net_admin — network admin bits used around raw sockets.
# cap_dac_override — write the root-owned /proc/sys/net knobs (ip_forward,
#                    send_redirects) that userspace forwarding needs.
echo "==> Granting cap_net_raw,cap_net_admin,cap_dac_override to the core (needs sudo)…"
sudo setcap cap_net_raw,cap_net_admin,cap_dac_override+ep "$BIN"

echo "==> Done:"
getcap "$BIN"
echo
echo "Run it with:  $BIN"
