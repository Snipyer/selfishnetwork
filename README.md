# SelfishNetwork

A cross-platform (v1: **Linux**) tool for managing your own local network via ARP
interception — discover devices, and cut or throttle their bandwidth.

![SelfishNetwork device table](docs/screenshot.png)

## Architecture

Two processes, split by privilege:

```
┌─────────────────────────────┐        ┌──────────────────────────────┐
│  Tauri UI (unprivileged)    │  WS    │  Rust core daemon (root-ish)  │
│  React + TS + Vite          │◄──────►│  net_raw/net_admin/dac_override│
│  device list, live graphs   │ JSON   │  ARP scan · ARP spoof (MITM)  │
│  cut / throttle controls    │ :8734  │  userspace fwd · token bucket │
└─────────────────────────────┘        └──────────────────────────────┘
```

- **`core/`** — Rust daemon. Does the privileged work: ARP discovery, MITM via
  bidirectional ARP spoofing, userspace L2 forwarding with a per-device token
  bucket (cut = stop forwarding). Serves a localhost WebSocket. See
  [`core/README.md`](core/README.md).
- **`ui/`** — Tauri shell (Rust, in `ui/src-tauri/`) wrapping a React/TypeScript
  frontend rendered in the system webview. Runs as your user, connects only to
  `127.0.0.1:8734`, never touches raw sockets. The shell also auto-starts the
  core daemon so you never launch it by hand.
- **`scripts/`** — `setcap` installer so the core runs without a password prompt,
  and a dev launcher.

The WS message contract shared by both sides lives in
[`docs/protocol.md`](docs/protocol.md).

## Quick start (dev)

```bash
# 1. build the core and grant it capabilities (asks for sudo once)
./scripts/setcap-core.sh

# 2. run core + UI together
./scripts/dev.sh
```

## Install (Linux)

Build a `.deb` and an `.AppImage` — both bundle the compiled core daemon and
grant it the required capabilities (the `.deb`'s postinstall script runs
`setcap`; see `ui/build/deb-after-install.sh`). The AppImage build needs
`patchelf` on the build host (`sudo apt install patchelf`):

```bash
cd core && cargo build --release && cd ..
cd ui && npm install && npm run dist:linux
```

Output lands in `ui/src-tauri/target/release/bundle/`:

- `appimage/SelfishNetwork_<version>_amd64.AppImage` — portable, no install
  step. The squashfs is read-only so it can't be `setcap`'d in place; instead
  the app copies the core to a writable per-user location on first launch and
  grants it capabilities via a one-time graphical `pkexec` prompt. If you'd
  rather do it by hand, extract it once
  (`./SelfishNetwork_*.AppImage --appimage-extract`) and run
  `sudo setcap cap_net_raw,cap_net_admin,cap_dac_override+ep squashfs-root/usr/lib/SelfishNetwork/bin/selfishnet-core`.
- `deb/SelfishNetwork_<version>_amd64.deb` — `sudo dpkg -i ui/src-tauri/target/release/bundle/deb/SelfishNetwork_*.deb`
  installs the binary to `/usr/bin`, the core to `/usr/lib/SelfishNetwork/bin`,
  and adds a desktop launcher.

## Status

Functional on Linux. Device discovery, ARP-spoof MITM, userspace forwarding,
per-device **cut** and **throttle**, and live bandwidth stats all work; ARP
caches and kernel sysctls are restored on release/exit. See
[`core/README.md`](core/README.md#how-forwarding-works) for how the data plane
works and its v1 limits (IPv4-only forwarding, drop-based throttling).
