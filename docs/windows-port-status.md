# Windows port — status & handoff

Snapshot for picking this up fresh. Date: 2026-10-28. All work below is
**uncommitted** on branch `master`.

## TL;DR

The Windows build is **complete and installs/runs**. The core daemon
cross-compiles, the NSIS installer builds from Linux, and the app launches,
connects, shows the right identity/gateway. **The blocker is runtime discovery
and MITM over Wi-Fi:** on the test machine's Wi-Fi adapter, every active
method (raw ARP injection, UDP-prod, and Windows' own `SendARP`) resolves
**zero** hosts. Evidence strongly indicates this is a **Wi-Fi medium
limitation**, not a code bug. **Next step: test on wired Ethernet** (not yet
done) — expected to work.

## How to build (from Linux)

Toolchain on the dev box: `cargo-xwin`, `makensis`, Rust target
`x86_64-pc-windows-msvc`, Node. The Npcap **SDK import libs** (`Packet.lib`,
`wpcap.lib`) live in `./.build-deps/npcap-sdk/Lib/x64` (gitignored; fetched from
https://npcap.com/dist/npcap-sdk-1.13.zip).

```bash
# core daemon → selfishnet-core.exe  (needs the Npcap SDK lib path)
cd core
RUSTFLAGS="-L native=$PWD/../.build-deps/npcap-sdk/Lib/x64" \
  cargo xwin build --release --target x86_64-pc-windows-msvc
cd ..
# shell + NSIS installer
cd ui && npm run dist:windows
# → ui/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/SelfishNetwork_0.1.0_x64-setup.exe
```

Runtime prereqs on the Windows machine:
- **Npcap** installed **with "WinPcap API-compatible Mode"** (else
  `packet.dll was not found` at launch).
- Run **as Administrator** (no UAC manifest yet; raw sockets need elevation).
- Windows **Defender may flag** the unsigned exe as `Trojan:Win32/Bearfoos.A!ml`
  (false positive — unsigned + new; allow it).

## Architecture context

- `core/` — privileged Rust daemon: ARP discovery, ARP-spoof MITM, userspace
  forwarding + per-device cut/throttle. Talks to the UI over `ws://127.0.0.1:8734`.
- `ui/` — Tauri v2 shell (React). Auto-spawns the core daemon as a bundled
  resource (`bin/selfishnet-core[.exe]`).
- OS-specific logic is isolated in **`core/src/platform/`** (`mod.rs` contract,
  `linux.rs`, `windows.rs`). The rest of the core is platform-neutral.

## What was changed (all uncommitted)

New: `core/src/platform/{mod,linux,windows}.rs`, `docs/windows-port.md`,
`docs/windows-port-status.md` (this file),
`ui/src-tauri/resources.{linux,windows}.json`.
Deleted: `core/src/net/sysctl.rs` (moved into `platform/linux.rs`).
Modified: `core/Cargo.toml` (+`windows-sys` 0.61 for `cfg(windows)`,
`+tracing-appender`), `core/src/{main,engine,ws,net/mod,net/scan}.rs`,
`ui/package.json` (`dist:windows` script), `ui/src-tauri/Cargo.toml`
(+`windows-sys` for Dwm), `ui/src-tauri/src/lib.rs`, `ui/src-tauri/tauri.conf.json`
(resources moved out to per-OS configs), `ui/src/{App.tsx,styles.css}`, README,
`.gitignore` (+`/.build-deps`).

### Windows backend (`core/src/platform/windows.rs`)
- `default_route()` — `GetBestRoute(0.0.0.0)` → gateway + interface index, mapped
  to the pnet interface name (`\Device\NPF_{GUID}`) with a subnet fallback. **Works.**
- `neighbor_table()` — `GetIpNetTable` (OS ARP cache). **Works** (reads the cache;
  filter is NOT by interface index — pnet's index ≠ `GetIpNetTable` dwIndex).
- `active_resolve()` — `SendARP` per host across 64 threads. **Returns 0 on the
  Wi-Fi adapter** (see below).
- `ForwardingGuard` — **no-op** on Windows (no live `/proc/sys` equivalent;
  relies on client defaults). Untested, since MITM never got that far.

### Discovery flow (`core/src/net/scan.rs::arp_sweep`)
1. seed from OS ARP cache (`neighbor_table`),
2. UDP-prod all 253 IPs to force kernel ARP (Windows: `SIO_UDP_CONNRESET`
   disabled so unreachable IPs don't abort the loop),
3. raw ARP broadcast rounds via pnet/Npcap, listen for replies,
4. `#[cfg(windows)]` `SendARP` active resolve / `#[cfg(not(windows))]` re-prod+settle,
5. final cache harvest.
   Each scan logs a `scan done: … | N arp replies | N os-resolved | N devices total`
   line. `NetCtx::is_lan_host()` filters results to real unicast hosts on the subnet.

### Logging
`core` now logs to stderr **and** to `%LOCALAPPDATA%\SelfishNetwork\core.log`
(`init_tracing` in `main.rs`, via `tracing-appender`). This is the primary
diagnostic channel since the console is hidden.

## The blocker — diagnostic evidence

From `%LOCALAPPDATA%\SelfishNetwork\core.log` on the test machine:

```
interface \Device\NPF_{...} (Qualcomm FastConnect 6900 Wi-Fi 6E ... WiFiCx Network Adapter)
  | self 192.168.1.38 | netmask 255.255.255.0 | gateway 192.168.1.1 | 253 candidate hosts
gateway MAC 1c:5f:2b:d2:42:36
scan done: 253 candidate hosts swept | neighbor cache raw 16→6 seeded, 16→0 harvested
  | 0 arp replies | 0 os-resolved | 6 devices total
```

Interpretation:
- Identity, netmask, subnet size, gateway MAC: **all correct.**
- **`0 arp replies`** every scan → our raw ARP broadcasts (via pnet/Npcap) elicit
  no responses (either not transmitted, or replies not captured).
- **`0 os-resolved`** → `SendARP` resolved **nothing, not even the gateway**
  (which is in the cache). The OS primitive itself fails on this adapter.
- The only devices shown are passive ARP-cache entries; the count grows
  (3→4→5→6) only as the user's own machine happens to talk to a host. A manual
  `ping <ip>` DOES populate the cache and that host then appears — so the cache
  read works; active resolution is what fails.

## Root-cause analysis

The adapter is a **Qualcomm Wi-Fi 6E, WiFiCx driver model**. Best current
understanding:
1. **Managed-mode Wi-Fi can't inject raw L2/ARP frames** (and WiFiCx is more
   locked down than older models) → `0 arp replies`.
2. **`SendARP` is unsupported/ineffective** on this WiFiCx adapter → `0 os-resolved`.
3. Even if discovery were forced, **ARP-spoof MITM fundamentally cannot work over
   Wi-Fi**: the AP encrypts each station's traffic with per-station keys, so a
   poisoned victim's packets never reach our adapter in a usable form. So
   **cut/throttle can't work over Wi-Fi regardless of discovery.**

Conclusion: this is a **medium/driver limitation, not a code defect.** Swapping
discovery APIs won't help on Wi-Fi.

## Recommended next steps (in order)

1. **Test on wired Ethernet** (or a USB-Ethernet dongle) on the Windows box — run
   one scan, check the log. Expect `arp replies` > 0, a full device list, and
   working cut/throttle. (Linux earlier saw 9 hosts, consistent with injection
   working there.) This is the make-or-break test and has NOT been done yet.
2. If wired works: the port is essentially functional — then do packaging polish
   (UAC manifest for auto-elevation, bundle/detect Npcap, code-signing).
3. If wired also shows `0 arp replies`: investigate pnet/Npcap send on Windows
   (does `tx.send_to` actually transmit? add logging of its return), and whether
   the capture needs promiscuous mode. Consider testing raw send with a minimal
   Npcap probe outside the app.
4. Do NOT keep adding Wi-Fi discovery methods — the evidence says the medium is
   the wall.

## Other open issues

- **Win11 window border hairline** (thin light line on sides/bottom of the dark,
  `decorations:false` window). Tried: (a) `html { background }` CSS — no effect;
  (b) `DWMWA_BORDER_COLOR = COLOR_NONE` — no effect (no framed border to color on
  a decorationless window); (c) `DWMWA_BORDER_COLOR = #0e1116` + 
  `DWMWA_WINDOW_CORNER_PREFERENCE = DONOTROUND` — **result unconfirmed by user.**
  Code is in `ui/src-tauri/src/lib.rs::strip_window_border`. Likely needs a
  `WM_NCCALCSIZE`/window-style approach or is a known Tauri+Win11 quirk. Minor,
  cosmetic (1px).
- **No code signing** → Defender ML false positives. Fix: Authenticate/EV cert +
  submit FP to Microsoft.
- **No UAC manifest** → must right-click "Run as administrator".
- **`ForwardingGuard` no-op on Windows** — untested; may need live forwarding/
  redirect control if MITM ever runs on wired.
- Everything is **uncommitted**. The `skills-lock.json` / `.claude/skills/tauri-v2`
  changes in `git status` are pre-existing and unrelated — exclude them.
