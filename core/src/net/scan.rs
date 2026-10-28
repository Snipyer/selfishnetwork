//! ARP sweep discovery.
//!
//! We broadcast an ARP request for every host in the subnet and collect the
//! replies. This is the one piece of the engine that's fully implemented in the
//! scaffold — it gives the UI a real device list to work with.

use super::NetCtx;
use anyhow::{anyhow, Result};
use pnet::datalink::{self, Channel, Config};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperation, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::{MutablePacket, Packet};
use pnet::util::MacAddr;
use std::collections::HashSet;
use std::io;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

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

/// Sweep the whole subnet. Calls `on_found` once per responding host.
pub fn arp_sweep<F: FnMut(Ipv4Addr, MacAddr)>(
    ctx: &NetCtx,
    overall_timeout: Duration,
    mut on_found: F,
) -> Result<()> {
    let (mut tx, mut rx) = channel(ctx, Duration::from_millis(200))?;

    for host in ctx.subnet_hosts() {
        let pkt = build_arp(
            ArpOperations::Request,
            ctx.our_mac,
            ctx.our_ip,
            MacAddr::broadcast(),
            MacAddr::zero(),
            host,
        );
        if let Some(Err(e)) = tx.send_to(&pkt, None) {
            return Err(anyhow!("failed to send ARP request: {e}"));
        }
    }

    let deadline = Instant::now() + overall_timeout;
    let mut seen: HashSet<Ipv4Addr> = HashSet::new();
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
