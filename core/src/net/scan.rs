//! ARP sweep discovery.
//!
//! Discovery is deliberately belt-and-suspenders because Wi-Fi devices in
//! power-save mode routinely ignore a single broadcast ARP request:
//!   1. seed from the kernel neighbor table (`/proc/net/arp`) — instant for
//!      anything the host already knows;
//!   2. nudge the kernel to resolve every host over its *own* retrying ARP (a
//!      throwaway UDP probe per host): a dozing device that ignores our one-shot
//!      broadcast still answers the kernel's persistent requests, so it lands in
//!      the neighbor table for the final harvest;
//!   3. send several *paced* rounds of broadcast ARP ourselves, so a dozing
//!      phone gets more chances to answer (and we don't drop packets by bursting
//!      254 at once);
//!   4. harvest the neighbor table again at the end, since the kernel also sees
//!      the replies and may have learned hosts we missed.

use super::NetCtx;
use crate::platform::neighbor_table;
use anyhow::{anyhow, Result};
use tracing::info;
use pnet::datalink::{self, Channel, Config};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperation, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::{MutablePacket, Packet};
use pnet::util::MacAddr;
use std::collections::HashSet;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

/// How many ARP rounds to send during a sweep.
const SWEEP_ROUNDS: u32 = 3;
/// Brief pause after every this-many sends, to avoid overrunning the TX buffer.
const PACE_EVERY: usize = 32;

const ETH_HDR: usize = 14;
const ARP_LEN: usize = 28;

/// Build an Ethernet-framed ARP packet (42 bytes).
pub fn build_arp(
    op: ArpOperation,
    src_mac: MacAddr,
    src_ip: Ipv4Addr,
    dst_mac: MacAddr,
    tgt_mac: MacAddr,
    tgt_ip: Ipv4Addr,
) -> [u8; ETH_HDR + ARP_LEN] {
    let mut buf = [0u8; ETH_HDR + ARP_LEN];
    {
        let mut eth = MutableEthernetPacket::new(&mut buf).expect("eth buf");
        eth.set_destination(dst_mac);
        eth.set_source(src_mac);
        eth.set_ethertype(EtherTypes::Arp);

        let mut arp = MutableArpPacket::new(eth.payload_mut()).expect("arp buf");
        arp.set_hardware_type(ArpHardwareTypes::Ethernet);
        arp.set_protocol_type(EtherTypes::Ipv4);
        arp.set_hw_addr_len(6);
        arp.set_proto_addr_len(4);
        arp.set_operation(op);
        arp.set_sender_hw_addr(src_mac);
        arp.set_sender_proto_addr(src_ip);
        arp.set_target_hw_addr(tgt_mac);
        arp.set_target_proto_addr(tgt_ip);
    }
    buf
}

fn channel(ctx: &NetCtx, read_timeout: Duration) -> Result<(Box<dyn datalink::DataLinkSender>, Box<dyn datalink::DataLinkReceiver>)> {
    let cfg = Config {
        read_timeout: Some(read_timeout),
        ..Default::default()
    };
    match datalink::channel(&ctx.iface, cfg)? {
        Channel::Ethernet(tx, rx) => Ok((tx, rx)),
        _ => Err(anyhow!("unsupported datalink channel type")),
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
}

/// Force the kernel to ARP-resolve each host by sending it a throwaway, empty
/// UDP datagram: the kernel must resolve the destination's MAC before it can
/// send, and its resolver retries persistently — so dozing / power-save devices
/// that ignore our single broadcast still get learned and show up in the
/// neighbor-table harvest. Nothing needs to be listening on the far side; send
/// errors (no route, unreachable) are expected and ignored.
fn prod_kernel_arp(hosts: &[Ipv4Addr]) {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return;
    };

    // Windows: by default, when a UDP socket gets an ICMP "port/host
    // unreachable" (which the many non-existent IPs in a /24 sweep provoke), the
    // *next* send fails with WSAECONNRESET. That aborts the prod partway, so most
    // hosts never get ARP-resolved and discovery misses them. Disable it.
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{SIO_UDP_CONNRESET, WSAIoctl};
        let disable: u32 = 0;
        let mut returned: u32 = 0;
        unsafe {
            WSAIoctl(
                sock.as_raw_socket() as usize,
                SIO_UDP_CONNRESET,
                &disable as *const u32 as *const core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
                core::ptr::null_mut(),
                0,
                &mut returned,
                core::ptr::null_mut(),
                None,
            );
        }
    }

    for host in hosts {
        // Port 9 (discard); the payload is irrelevant — the ARP resolution the
        // kernel must do before it can send is the whole point.
        let _ = sock.send_to(&[0u8], SocketAddrV4::new(*host, 9));
    }
}

/// Sweep the whole subnet. Calls `on_found` once per discovered host.
///
/// `window` is the total listening budget, split across [`SWEEP_ROUNDS`].
pub fn arp_sweep<F: FnMut(Ipv4Addr, MacAddr)>(
    ctx: &NetCtx,
    window: Duration,
    mut on_found: F,
) -> Result<()> {
    let (mut tx, mut rx) = channel(ctx, Duration::from_millis(100))?;
    let mut seen: HashSet<Ipv4Addr> = HashSet::new();

    // 1. Anything the kernel already knows (skips our own address).
    let seed = neighbor_table(&ctx.iface.name);
    let seed_raw = seed.len();
    let mut seeded = 0usize;
    for (ip, mac) in seed {
        if ip != ctx.our_ip && ctx.is_lan_host(ip) && seen.insert(ip) {
            on_found(ip, mac);
            seeded += 1;
        }
    }

    // 2. Nudge the kernel to ARP every host (its retrying resolver catches
    //    power-save devices our one-shot broadcast misses). This runs in the
    //    background while our own rounds below play out, settling by the harvest.
    let hosts = ctx.subnet_hosts();
    prod_kernel_arp(&hosts);

    // 3. Several paced rounds, listening between each.
    let mut replies = 0usize;
    let per_round = window / SWEEP_ROUNDS.max(1);
    for _ in 0..SWEEP_ROUNDS {
        for (i, host) in hosts.iter().enumerate() {
            let pkt = build_arp(
                ArpOperations::Request,
                ctx.our_mac,
                ctx.our_ip,
                MacAddr::broadcast(),
                MacAddr::zero(),
                *host,
            );
            if let Some(Err(e)) = tx.send_to(&pkt, None) {
                return Err(anyhow!("failed to send ARP request: {e}"));
            }
            if i % PACE_EVERY == PACE_EVERY - 1 {
                thread::sleep(Duration::from_millis(1));
            }
        }

        let deadline = Instant::now() + per_round;
        while Instant::now() < deadline {
            match rx.next() {
                Ok(frame) => {
                    if let Some((ip, mac)) = parse_arp_reply(frame, ctx.our_ip) {
                        if ctx.is_lan_host(ip) && seen.insert(ip) {
                            on_found(ip, mac);
                            replies += 1;
                        }
                    }
                }
                Err(ref e) if is_timeout(e) => continue,
                Err(e) => return Err(anyhow!("recv error during scan: {e}")),
            }
        }
    }

    // 4. Resolve stragglers. On Windows, raw ARP injection over Wi-Fi yields
    //    nothing, so resolve each host through the OS via SendARP. On Linux the
    //    raw sweep works, so just re-prod the kernel and let it settle.
    #[allow(unused_mut)]
    let mut resolved = 0usize;
    #[cfg(windows)]
    for (ip, mac) in crate::platform::active_resolve(&hosts) {
        if ip != ctx.our_ip && ctx.is_lan_host(ip) && seen.insert(ip) {
            on_found(ip, mac);
            resolved += 1;
        }
    }
    #[cfg(not(windows))]
    {
        prod_kernel_arp(&hosts);
        thread::sleep(Duration::from_millis(600));
    }

    // 5. Final harvest: the kernel may have learned hosts from the replies above
    //    and from the UDP-prodded resolutions.
    let harvest = neighbor_table(&ctx.iface.name);
    let harvest_raw = harvest.len();
    let mut harvested = 0usize;
    for (ip, mac) in harvest {
        if ip != ctx.our_ip && ctx.is_lan_host(ip) && seen.insert(ip) {
            on_found(ip, mac);
            harvested += 1;
        }
    }

    info!(
        "scan done: {} candidate hosts swept | neighbor cache raw {}→{} seeded, {}→{} harvested | {} arp replies | {} os-resolved | {} devices total",
        hosts.len(),
        seed_raw,
        seeded,
        harvest_raw,
        harvested,
        replies,
        resolved,
        seen.len(),
    );
    Ok(())
}

/// Resolve a single IP to its MAC (e.g. the gateway), or `None` on timeout.
pub fn resolve_mac(ctx: &NetCtx, target: Ipv4Addr, timeout: Duration) -> Result<Option<MacAddr>> {
    let (mut tx, mut rx) = channel(ctx, Duration::from_millis(200))?;
    let pkt = build_arp(
        ArpOperations::Request,
        ctx.our_mac,
        ctx.our_ip,
        MacAddr::broadcast(),
        MacAddr::zero(),
        target,
    );
    if let Some(Err(e)) = tx.send_to(&pkt, None) {
        return Err(anyhow!("failed to send ARP request: {e}"));
    }

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.next() {
            Ok(frame) => {
                if let Some((ip, mac)) = parse_arp_reply(frame, ctx.our_ip) {
                    if ip == target {
                        return Ok(Some(mac));
                    }
                }
            }
            Err(ref e) if is_timeout(e) => continue,
            Err(e) => return Err(anyhow!("recv error: {e}")),
        }
    }
    // Active ARP timed out (common on Wi-Fi, where the gateway may not answer
    // our raw request). Fall back to the OS neighbor cache, which usually
    // already knows the MAC.
    Ok(neighbor_table(&ctx.iface.name)
        .into_iter()
        .find(|(ip, _)| *ip == target)
        .map(|(_, mac)| mac))
}

/// Extract (sender_ip, sender_mac) from an ARP reply frame, skipping our own.
fn parse_arp_reply(frame: &[u8], our_ip: Ipv4Addr) -> Option<(Ipv4Addr, MacAddr)> {
    let eth = EthernetPacket::new(frame)?;
    if eth.get_ethertype() != EtherTypes::Arp {
        return None;
    }
    let arp = ArpPacket::new(eth.payload())?;
    if arp.get_operation() != ArpOperations::Reply {
        return None;
    }
    let ip = arp.get_sender_proto_addr();
    if ip == our_ip {
        return None;
    }
    Some((ip, arp.get_sender_hw_addr()))
}
