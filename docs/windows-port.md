# Windows port

SelfishNetwork runs on Windows. This doc tracks how the port is built and its
one notable runtime limitation.

**Build pipeline: done.** Every OS-coupled piece of the core lives behind one
module (`core/src/platform/`), the core cross-compiles and links to a real
`selfishnet-core.exe`, and `npm run dist:windows` produces an NSIS `.exe`
installer from Linux (see the README "Build (Windows)" section). The installer
bundles the core daemon and installs cleanly.

**Runtime: working on wired Ethernet.** The Windows backend
(`core/src/platform/windows.rs`) is implemented — discovery, cut, throttle and
restore work on wired adapters. The one caveat is Wi-Fi (see below).

## Known limitation: Wi-Fi adapters

Device control only works reliably over a **wired Ethernet** connection on
Windows. Most Wi-Fi adapters, when associated to an access point in managed
(infrastructure) mode, will not transmit layer-2 frames whose source MAC isn't
the adapter's own — the 802.11 stack and driver silently rewrite or drop them.
ARP-spoof MITM depends on sending frames with a spoofed source MAC, so on those
adapters the spoof never reaches the targets and cut/throttle has no effect.

This is a platform/driver constraint (Npcap exposes the same limits as the
underlying 802.11 stack), not something the app can work around in software.
Linux is largely unaffected because its drivers permit raw layer-2 injection on
Wi-Fi. Run SelfishNetwork on a wired connection on Windows for now.

## What's already portable

- Packet capture / injection and ARP poisoning run on `pnet`, which supports
  Windows (via Npcap — see below).
- The data plane (`engine.rs`), discovery sweeps (`net/scan.rs`), subnet math,
  the WebSocket protocol and the Tauri UI are all OS-agnostic.
- The three non-portable surfaces are isolated in `core/src/platform/`:
  - `linux.rs` — the Linux backend.
  - `windows.rs` — the Windows backend (IP Helper API + registry/`netsh`).
  - `mod.rs` — the contract both backends satisfy.

## What the Windows backend must implement

Fill in `core/src/platform/windows.rs`. Each item mirrors a Linux `/proc`
mechanism.

### 1. `ForwardingGuard` — stop the OS forwarding and sending ICMP redirects
Linux writes `net.ipv4.ip_forward` and `.../send_redirects` under `/proc/sys`.
Windows equivalents:
- **IP forwarding** — `IPEnableRouter` registry value under
  `HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters`. The registry route
  needs a service restart to take effect; prefer a per-interface WFP filter or
  `netsh interface ipv4 set interface <idx> forwarding=disabled` so it applies
  live and reverts cleanly.
- **ICMP redirects** — `EnableICMPRedirect` under the same key.
Save the prior values and restore them on `restore()`/drop, exactly like Linux.

### 2. `default_route` — gateway IP + its interface name
Use the IP Helper API: `GetIpForwardTable2` / `GetBestRoute2` to find the
`0.0.0.0/0` route, then map its interface index to the pnet interface name via
`GetAdaptersAddresses`. (Crate options: `windows`/`windows-sys`, or `ipconfig`.)

### 3. `neighbor_table` — the OS ARP cache (discovery seed)
`GetIpNetTable2` (`MIB_IPNET_TABLE2`), filtered to the interface index and to
reachable/stale entries. Lowest priority: returning empty only makes discovery
fall back to active ARP sweeps, which already work.

## Privilege model

Linux uses file capabilities (`cap_net_raw`, `cap_net_admin`,
`cap_dac_override`) granted by the `.deb` postinstall `setcap`. Windows has no
equivalent:
- The core must run **elevated (Administrator)** for raw packet access and
  forwarding changes. Add a UAC manifest (`requireAdministrator`) to the core
  binary, and have the Tauri shell launch it elevated (or install it as a
  Windows service).
- Raw layer-2 send/receive via pnet requires **Npcap** installed (WinPcap API
  compatibility mode). The installer must bundle or require it; detect its
  absence and surface a clear error.

## Packaging — done

- `npm run dist:windows` cross-builds the NSIS `.exe` via `cargo-xwin` +
  `makensis`. The core `.exe` is supplied as a resource through
  `ui/src-tauri/resources.windows.json` (the Linux path lives in
  `resources.linux.json`, since the two differ).
- Remaining packaging polish (not blocking a build):
  - Add a UAC manifest (`requireAdministrator`) so the installed app can get
    raw-socket access, replacing the Linux `setcap` model.
  - Bundle or prompt for **Npcap** at install time (needed at runtime for
    capture).
  - Optionally add `msi` to `--bundles` and a Windows CI runner (none exists
    yet) so releases build without a local cross-toolchain.
  - Code-signing is skipped (cross-build can't sign); set
    `bundle > windows > sign_command` or sign on a Windows host for release.

## Suggested order

1. Implement `default_route` + `neighbor_table` (read-only, easy to verify).
2. Implement `ForwardingGuard` (live mechanism, save/restore).
3. Privilege: UAC manifest + Npcap detection; get a dev build capturing packets.
4. Packaging + CI once it runs end to end.
