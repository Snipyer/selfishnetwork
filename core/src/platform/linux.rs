//! Linux backend: `/proc`-based route and neighbor lookups, and sysctl toggles.

use anyhow::{anyhow, Context, Result};
use pnet::util::MacAddr;
use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use tracing::warn;

/// Kernel knobs we must toggle for userspace forwarding, with restore-on-drop.
///
/// - `net.ipv4.ip_forward = 0` — we forward in userspace, so the kernel must
///   NOT also forward (that would duplicate every packet).
/// - `net.ipv4.conf.{all,<iface>}.send_redirects = 0` — stop the kernel from
///   emitting ICMP redirects that would tell our victims the real gateway and
///   unravel the MITM.
///
/// Writing these `/proc/sys` files is why the core needs `cap_dac_override`
/// (the files are root-owned) in addition to `cap_net_admin`.
pub struct ForwardingGuard {
    saved: Vec<(PathBuf, String)>,
    restored: bool,
}

impl ForwardingGuard {
    /// Read-then-set the sysctls for `iface`, remembering prior values.
    pub fn apply(iface: &str) -> Result<Self> {
        let mut guard = ForwardingGuard {
            saved: Vec::new(),
            restored: false,
        };
        let targets = [
            ("/proc/sys/net/ipv4/ip_forward".to_string(), "0"),
            ("/proc/sys/net/ipv4/conf/all/send_redirects".to_string(), "0"),
            (
                format!("/proc/sys/net/ipv4/conf/{iface}/send_redirects"),
                "0",
            ),
        ];
        for (path, want) in targets {
            if let Err(e) = guard.set_one(&path, want) {
                // Roll back anything we already changed, then fail.
                guard.restore();
                return Err(e);
            }
        }
        Ok(guard)
    }

    fn set_one(&mut self, path: &str, value: &str) -> Result<()> {
        let pb = PathBuf::from(path);
        let original = fs::read_to_string(&pb)
            .with_context(|| format!("reading sysctl {path}"))?
            .trim()
            .to_string();
        fs::write(&pb, format!("{value}\n"))
            .with_context(|| format!("writing sysctl {path} (need cap_dac_override?)"))?;
        self.saved.push((pb, original));
        Ok(())
    }

    /// Put every changed sysctl back to its original value (best effort).
    pub fn restore(&mut self) {
        if self.restored {
            return;
        }
        for (path, original) in self.saved.iter().rev() {
            if let Err(e) = fs::write(path, format!("{original}\n")) {
                warn!("failed to restore {}: {e}", path.display());
            }
        }
        self.restored = true;
    }
}

impl Drop for ForwardingGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Parse `/proc/net/route` for the default gateway and its interface.
pub fn default_route() -> Result<(Ipv4Addr, String)> {
    let contents = fs::read_to_string("/proc/net/route")?;
    for line in contents.lines().skip(1) {
        let mut cols = line.split_whitespace();
        let iface = cols.next().unwrap_or("");
        let dest = cols.next().unwrap_or("");
        let gw_hex = cols.next().unwrap_or("");
        // Default route: destination 0.0.0.0.
        if dest == "00000000" {
            let raw = u32::from_str_radix(gw_hex, 16)
                .map_err(|_| anyhow!("bad gateway field in route table"))?;
            // Fields are little-endian hex.
            let gw = Ipv4Addr::from(raw.to_be());
            return Ok((gw, iface.to_string()));
        }
    }
    Err(anyhow!("no default route found"))
}

/// Read completed entries for `iface` from the kernel's `/proc/net/arp`.
pub fn neighbor_table(iface: &str) -> Vec<(Ipv4Addr, MacAddr)> {
    match fs::read_to_string("/proc/net/arp") {
        Ok(contents) => parse_neighbor_table(&contents, iface),
        Err(_) => Vec::new(),
    }
}

/// Parse `/proc/net/arp` text, keeping completed entries on `iface`.
fn parse_neighbor_table(contents: &str, iface: &str) -> Vec<(Ipv4Addr, MacAddr)> {
    let mut out = Vec::new();
    // Columns: IP address, HW type, Flags, HW address, Mask, Device
    for line in contents.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 || cols[5] != iface {
            continue;
        }
        // Flags 0x0 = incomplete; skip those and the null MAC.
        if cols[2] == "0x0" || cols[3] == "00:00:00:00:00:00" {
            continue;
        }
        if let (Ok(ip), Ok(mac)) = (cols[0].parse::<Ipv4Addr>(), cols[3].parse::<MacAddr>()) {
            out.push((ip, mac));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_entries_for_iface() {
        let sample = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         1c:5f:2b:d2:42:36     *        enp5s0
192.168.1.66     0x1         0x2         20:34:fb:a3:b9:e4     *        enp5s0
192.168.1.99     0x1         0x0         00:00:00:00:00:00     *        enp5s0
10.0.0.5         0x1         0x2         aa:bb:cc:dd:ee:ff     *        wlan0";
        let got = parse_neighbor_table(sample, "enp5s0");
        assert_eq!(got.len(), 2, "two complete enp5s0 entries");
        assert!(got.contains(&("192.168.1.1".parse().unwrap(), "1c:5f:2b:d2:42:36".parse().unwrap())));
        assert!(got.contains(&("192.168.1.66".parse().unwrap(), "20:34:fb:a3:b9:e4".parse().unwrap())));
    }
}
