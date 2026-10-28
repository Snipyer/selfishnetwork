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

Raw sockets require privileges. Grant capabilities once (preferred over sudo):

```bash
sudo setcap cap_net_raw,cap_net_admin+ep target/release/selfishnet-core
./target/release/selfishnet-core            # auto-detects the default interface
./target/release/selfishnet-core -i wlan0   # or pick one
```

Flags: `-i/--interface <name>`, `-p/--port <port>` (default 8734),
`-b/--bind <addr>` (default 127.0.0.1).

## Layout

| file                    | responsibility                                         |
|-------------------------|--------------------------------------------------------|
| `src/main.rs`           | CLI, interface selection, starts the WS server         |
| `src/protocol.rs`       | serde types for the WS contract (`Command` / `Event`)  |
| `src/state.rs`          | shared device table & session state                    |
| `src/ws.rs`             | WebSocket server; routes commands, pushes events       |
| `src/net/mod.rs`        | interface / gateway / MAC helpers                      |
| `src/net/scan.rs`       | ARP sweep discovery (implemented)                      |
| `src/net/arp.rs`        | ARP-spoof MITM (stub — next)                           |
| `src/net/forward.rs`    | userspace forwarding + token-bucket throttle (stub)    |
