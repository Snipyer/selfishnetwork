#!/usr/bin/env bash
# Launch the SelfishNetwork desktop app in development. Tauri runs the Vite dev
# server (beforeDevCommand) and the Rust shell auto-starts the core daemon, so
# this just checks the core is built + setcap'd, then hands off to `tauri dev`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/core/target/release/selfishnet-core"

if [ ! -x "$BIN" ]; then
  echo "Core not built. Run: ./scripts/setcap-core.sh" >&2
  exit 1
fi

cd "$ROOT/ui"
[ -d node_modules ] || npm install
echo "==> Starting SelfishNetwork (Tauri dev)…"
npm run tauri dev
