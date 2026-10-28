#!/bin/sh
# Runs as root during `dpkg -i`/apt install. Grants the bundled core daemon
# the raw-socket/forwarding capabilities it needs so the app works without a
# sudo prompt every launch (mirrors scripts/setcap-core.sh).
#
# Tauri bundles the daemon as an app resource, which dpkg installs under
# /usr/lib/SelfishNetwork/; we search there (falling back to /opt for older
# layouts).
set -e

BIN="$(find /usr/lib /opt -maxdepth 6 -type f -name selfishnet-core 2>/dev/null | head -n1)"

if [ -n "$BIN" ] && command -v setcap >/dev/null 2>&1; then
  setcap cap_net_raw,cap_net_admin,cap_dac_override+ep "$BIN" || true
fi

exit 0
