#!/usr/bin/env bash
# Launch the core daemon and the Electron/Vite UI together for development.
# Assumes the core is already built + setcap'd (run scripts/setcap-core.sh once).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/core/target/release/selfishnet-core"

if [ ! -x "$BIN" ]; then
  echo "Core not built. Run: ./scripts/setcap-core.sh" >&2
  exit 1
fi

echo "==> Starting core…"
"$BIN" "$@" &
CORE_PID=$!
trap 'kill "$CORE_PID" 2>/dev/null || true' EXIT

echo "==> Starting UI…"
cd "$ROOT/ui"
[ -d node_modules ] || npm install
npm run dev
