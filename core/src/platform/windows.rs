//! Windows backend — IP Helper (`iphlpapi`) for routes/neighbors, and pnet for
//! interface identity.
//!
//! The data plane (capture/inject/ARP) runs on pnet + Npcap exactly as on
//! Linux. The only Windows-specific logic is here:
//!   - `default_route` asks the OS for the best route to 0.0.0.0 (the default
//!     route) and maps its interface index to the exact name pnet reports.
//!   - `neighbor_table` reads the ARP cache (`GetIpNetTable`).
//!   - `ForwardingGuard` is a documented no-op (see below).
//!
//! Status: compiles and cross-builds; validate on real Windows hardware.

use anyhow::{anyhow, bail, Result};
use pnet::datalink;
use pnet::ipnetwork::IpNetwork;
use pnet::util::MacAddr;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetBestRoute, GetIpNetTable, SendARP, MIB_IPFORWARDROW, MIB_IPNETTABLE,
};

/// A network-order DWORD (as IP Helper returns IPv4 addresses) → `Ipv4Addr`.
/// Windows is little-endian, so the low byte is the first octet.
fn dword_to_ipv4(v: u32) -> Ipv4Addr {
    Ipv4Addr::new(v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8)
}

/// Kernel forwarding controls — a deliberate no-op on Windows.
///
/// Linux toggles `/proc/sys` knobs live; Windows has no equivalent. IP
/// forwarding (`IPEnableRouter`) and ICMP redirects (`EnableICMPRedirect`) live
/// in the registry and only take effect after a reboot, so a guard can't flip
/// them for the lifetime of a MITM. We rely on the Windows client defaults,
/// which already match what we need: IP forwarding is off (so the kernel won't
/// duplicate the frames we forward in userspace) and the host emits ICMP
/// redirects only when it's actually routing — which it isn't; we only poison
/// ARP. Userspace cut/throttle still bite because the kernel isn't forwarding
/// the victim's traffic at all. See docs/windows-port.md.
pub struct ForwardingGuard;

impl ForwardingGuard {
    pub fn apply(_iface: &str) -> Result<Self> {
        Ok(ForwardingGuard)
    }

    pub fn restore(&mut self) {}
}

impl Drop for ForwardingGuard {
    fn drop(&mut self) {}
}

/// The default gateway's IPv4 address and the interface pnet uses for it.
///
/// The best route to `0.0.0.0` is the default route. We take its next hop
/// (gateway) and interface index, then map that index to the exact name
/// `pnet::datalink::interfaces()` reports (`\Device\NPF_{GUID}`) so
/// `NetCtx::resolve` can match it. If the index doesn't line up, fall back to
/// the interface whose subnet contains the gateway.
pub fn default_route() -> Result<(Ipv4Addr, String)> {
    let mut row: MIB_IPFORWARDROW = unsafe { std::mem::zeroed() };
    let rc = unsafe { GetBestRoute(0, 0, &mut row) };
    if rc != NO_ERROR {
        bail!("GetBestRoute failed (error {rc}) — no default route?");
    }

    let gateway = dword_to_ipv4(row.dwForwardNextHop);
    let if_index = row.dwForwardIfIndex;

    let ifaces = datalink::interfaces();
    let name = ifaces
        .iter()
        .find(|i| i.index == if_index)
        .or_else(|| {
            ifaces.iter().find(|i| {
                i.ips.iter().any(
                    |ip| matches!(ip, IpNetwork::V4(v4) if v4.contains(gateway)),
                )
            })
        })
        .map(|i| i.name.clone())
        .ok_or_else(|| {
            anyhow!("no local interface matches the default route (index {if_index}, gateway {gateway})")
        })?;

    Ok((gateway, name))
}

/// Actively resolve each host's MAC via the OS, using `SendARP` (the IP Helper
/// primitive behind `arp -a`/`ping`). This is how discovery works on Windows:
/// raw ARP injection over 802.11 managed-mode Wi-Fi yields nothing, but
/// `SendARP` goes through the OS stack and reliably resolves reachable hosts.
///
/// `SendARP` blocks ~1–2s for a host that doesn't answer, so we fan out across a
/// pool of threads pulling from a shared work index to keep a full /24 sweep to
/// a few seconds. Unreachable hosts are simply omitted.
pub fn active_resolve(hosts: &[Ipv4Addr]) -> Vec<(Ipv4Addr, MacAddr)> {
    if hosts.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::new());
    let workers = 64.min(hosts.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(&ip) = hosts.get(i) else { break };

                // SendARP takes the target as a network-order DWORD; on
                // little-endian Windows that's the octets in order.
                let dest = u32::from_le_bytes(ip.octets());
                let mut mac = [0u8; 8];
                let mut len: u32 = 6;
                let rc = unsafe {
                    SendARP(dest, 0, mac.as_mut_ptr() as *mut core::ffi::c_void, &mut len)
                };
                if rc == NO_ERROR && len >= 6 && mac[..6] != [0u8; 6] {
                    out.lock().unwrap().push((
                        ip,
                        MacAddr(mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]),
                    ));
                }
            });
        }
    });

    out.into_inner().unwrap()
}

/// The OS ARP cache (every interface).
///
/// We deliberately do NOT filter by interface here: `GetIpNetTable`'s
/// `dwIndex` is not guaranteed to match pnet's interface index, and filtering
/// on it was dropping real entries (e.g. the gateway). The caller scopes
/// results to our subnet via `NetCtx::is_lan_host`, which is what matters.
/// Non-fatal everywhere: any failure yields an empty list and discovery falls
/// back to active ARP sweeps.
pub fn neighbor_table(_iface: &str) -> Vec<(Ipv4Addr, MacAddr)> {
    unsafe {
        // First call sizes the buffer.
        let mut size: u32 = 0;
        if GetIpNetTable(std::ptr::null_mut(), &mut size, 0) != ERROR_INSUFFICIENT_BUFFER
            || size == 0
        {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize];
        let table = buf.as_mut_ptr() as *mut MIB_IPNETTABLE;
        if GetIpNetTable(table, &mut size, 0) != NO_ERROR {
            return Vec::new();
        }

        let n = (*table).dwNumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).table.as_ptr(), n);
        let mut out = Vec::new();
        for r in rows {
            // Keep only complete entries (6-byte, non-null MAC).
            if r.dwPhysAddrLen != 6 {
                continue;
            }
            let p = r.bPhysAddr;
            if p[..6] == [0u8; 6] {
                continue;
            }
            out.push((
                dword_to_ipv4(r.dwAddr),
                MacAddr(p[0], p[1], p[2], p[3], p[4], p[5]),
            ));
        }
        out
    }
}
