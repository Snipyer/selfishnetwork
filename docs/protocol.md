# WS protocol

The UI and core talk JSON over a WebSocket on `ws://127.0.0.1:8734`.
Every message is a single JSON object with a discriminator field.

## Commands (UI → core)

Tagged by `"cmd"`:

| cmd         | fields                                        | meaning                                  |
|-------------|-----------------------------------------------|------------------------------------------|
| `scan`      | —                                             | (re)discover devices via ARP sweep       |
| `intercept` | `ip`                                           | start MITM on a device                   |
| `release`   | `ip`                                           | stop MITM, restore the device's ARP cache|
| `cut`       | `ip`, `enabled` (bool)                         | fully block / unblock a device           |
| `throttle`  | `ip`, `down_kbps` (int\|null), `up_kbps` (int\|null) | rate-limit; `null` = unlimited     |
| `status`    | —                                             | ask for a full state snapshot            |

Example: `{"cmd":"throttle","ip":"192.168.1.42","down_kbps":512,"up_kbps":256}`

## Events (core → UI)

Tagged by `"event"`:

| event           | fields                                                        |
|-----------------|---------------------------------------------------------------|
| `hello`         | `iface`, `gateway` (ip/mac), `self` (ip/mac)                   |
| `scan_started`  | —                                                             |
| `device`        | `device` — `{ip, mac, hostname?, vendor?, intercepted, cut, throttle?}` |
| `scan_done`     | `count`                                                       |
| `device_update` | `device` (same shape)                                         |
| `stats`         | `ip`, `down_bps`, `up_bps`, `down_total`, `up_total`          |
| `error`         | `message`                                                     |

`throttle` object shape: `{down_kbps: int|null, up_kbps: int|null}`.
