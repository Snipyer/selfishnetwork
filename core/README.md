# selfishnet-core

The privileged Rust daemon. Discovers devices, performs the ARP-spoof MITM,
forwards intercepted traffic in userspace with per-device token buckets, and
exposes everything over a localhost WebSocket.

## Build

```bash
cargo build --release
```

No `libpcap-dev` needed on Linux — `pnet` uses `AF_PACKET` raw sockets directly.

## Run

Raw sockets and the `/proc/sys` network knobs require privileges. Grant
capabilities once (preferred over sudo):

```bash
sudo setcap cap_net_raw,cap_net_admin,cap_dac_override+ep target/release/selfishnet-core
./target/release/selfishnet-core            # auto-detects the default interface
./target/release/selfishnet-core -i wlan0   # or pick one
```

`cap_dac_override` is needed because userspace forwarding toggles the
root-owned `net.ipv4.ip_forward` and `send_redirects` sysctls (restored on
release / exit). Prefer `scripts/setcap-core.sh`, which sets all three.

Flags: `-i/--interface <name>`, `-p/--port <port>` (default 8734),
`-b/--bind <addr>` (default 127.0.0.1).

## Layout

| file                    | responsibility                                         |
|-------------------------|--------------------------------------------------------|
| `src/main.rs`           | CLI, interface selection, starts engine + WS server    |
| `src/protocol.rs`       | serde types for the WS contract (`Command` / `Event`)  |
| `src/state.rs`          | shared `App`: device table, engine, event broadcast    |
| `src/ws.rs`             | WebSocket server; routes commands, broadcasts events   |
| `src/engine.rs`         | data plane: capture→throttle→re-inject, poison & stats |
| `src/net/mod.rs`        | interface / gateway helpers, subnet math               |
| `src/net/scan.rs`       | ARP sweep discovery                                    |
| `src/net/arp.rs`        | ARP-spoof poison / restore primitives                  |
| `src/net/forward.rs`    | token-bucket rate limiter                              |
| `src/net/sysctl.rs`     | ip_forward / send_redirects guard (restore on drop)    |

## How forwarding works

With a device intercepted, the poison loop makes both it and the gateway send
their traffic to our MAC. The capture thread reads those frames, meters each
through the device's up/down token bucket (dropping when **cut** or out of
tokens), rewrites L2 (src = us, dst = the real next hop) and re-injects.
`ip_forward=0` ensures the kernel doesn't forward too; `send_redirects=0` stops
ICMP redirects from unravelling the MITM. On release/exit, ARP caches and
sysctls are restored.

v1 forwards IPv4 only (a target loses IPv6 while intercepted) and throttles by
dropping rather than queueing — effective but bursty.
