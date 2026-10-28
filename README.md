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

## Build (Windows)

A Windows `.exe` installer cross-compiles from Linux. The networking backend is
not implemented yet (it builds and errors at runtime — see
[`docs/windows-port.md`](docs/windows-port.md)), but the installer and app shell
build end to end. Prerequisites on the build host:

- [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin) (`cargo install cargo-xwin`)
  and the MSVC Rust target (`rustup target add x86_64-pc-windows-msvc`),
- `makensis` for the NSIS installer (`sudo apt install nsis`),
- the [Npcap SDK](https://npcap.com/#download) import libs (`Packet.lib`,
  `wpcap.lib`) — pnet links against them for raw capture. Point the core build
  at them with `-L native=<sdk>/Lib/x64`.

```bash
# 1. core daemon → selfishnet-core.exe (needs the Npcap SDK lib path)
cd core
RUSTFLAGS="-L native=/path/to/npcap-sdk/Lib/x64" \
  cargo xwin build --release --target x86_64-pc-windows-msvc
cd ..

# 2. app shell + NSIS installer
cd ui && npm install && npm run dist:windows
```

Output: `ui/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/SelfishNetwork_<version>_x64-setup.exe`,
which bundles `selfishnet-core.exe`.

**Runtime requirement — [Npcap](https://npcap.com/#download):** `selfishnet-core.exe`
load-time-links `Packet.dll`/`wpcap.dll`, so without Npcap it won't even launch
(`packet.dll was not found`). Install Npcap on the target machine **with
"Install Npcap in WinPcap API-compatible Mode" checked** — otherwise the DLLs go
to `System32\Npcap\` (off the default search path) and the loader still can't
find them. The installer should eventually bundle Npcap (see
[`docs/windows-port.md`](docs/windows-port.md)); until then it's a manual
prerequisite.

> ⚠️ **Windows Defender may flag the app.** The installer and `selfishnet-ui.exe`
> are unsigned and have no reputation yet, and the app's purpose (ARP
> interception) resembles attack behavior, so Defender's ML engine may report a
> generic detection such as `Trojan:Win32/Bearfoos.A!ml` — on the UI shell and
> its Start Menu shortcut, not just the core. This is a false positive. If you
> trust your own build, allow it through Defender (Windows Security → Virus &
> threat protection → Protection history → Allow). SmartScreen may likewise warn
> on first run ("More info" → "Run anyway"). The lasting fixes are Authenticode
> **code-signing** (ideally an EV cert) and submitting the binary to Microsoft's
> [false-positive portal](https://www.microsoft.com/en-us/wdsi/filesubmission).

## Status

Functional on Linux. Device discovery, ARP-spoof MITM, userspace forwarding,
per-device **cut** and **throttle**, and live bandwidth stats all work; ARP
caches and kernel sysctls are restored on release/exit. See
[`core/README.md`](core/README.md#how-forwarding-works) for how the data plane
works and its v1 limits (IPv4-only forwarding, drop-based throttling).

Windows builds and installs, but the platform backend
([`core/src/platform/windows.rs`](core/src/platform/windows.rs)) is still a stub
— device control won't work until it's implemented and validated on Windows
hardware. [`docs/windows-port.md`](docs/windows-port.md) tracks what remains.
