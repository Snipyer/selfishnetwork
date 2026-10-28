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
use anyhow::{anyhow, Result};
use pnet::datalink::{self, Channel, Config};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperation, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::{MutablePacket, Packet};
use pnet::util::MacAddr;
use std::collections::HashSet;
use std::fs;
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
    for host in hosts {
        // Port 9 (discard); the payload is irrelevant, the ARP side effect is
        // the whole point.
        let _ = sock.send_to(&[], SocketAddrV4::new(*host, 9));
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
    for (ip, mac) in neighbor_table(&ctx.iface.name) {
        if ip != ctx.our_ip && seen.insert(ip) {
            on_found(ip, mac);
        }
    }

    // 2. Nudge the kernel to ARP every host (its retrying resolver catches
    //    power-save devices our one-shot broadcast misses). This runs in the
    //    background while our own rounds below play out, settling by the harvest.
    let hosts = ctx.subnet_hosts();
    prod_kernel_arp(&hosts);

    // 3. Several paced rounds, listening between each.
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
                        if seen.insert(ip) {
                            on_found(ip, mac);
                        }
                    }
                }
                Err(ref e) if is_timeout(e) => continue,
                Err(e) => return Err(anyhow!("recv error during scan: {e}")),
            }
        }
    }

    // 4. Final harvest: the kernel may have learned hosts from the replies above
    //    and from the UDP-prodded resolutions.
    for (ip, mac) in neighbor_table(&ctx.iface.name) {
        if ip != ctx.our_ip && seen.insert(ip) {
            on_found(ip, mac);
        }
    }
    Ok(())
}

/// Read completed entries for `iface` from the kernel's `/proc/net/arp`.
fn neighbor_table(iface: &str) -> Vec<(Ipv4Addr, MacAddr)> {
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
    Ok(None)
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
