//! Network context: interface selection, our own identity, gateway detection.

pub mod arp;
pub mod forward;
pub mod scan;

use anyhow::{anyhow, Context, Result};
use pnet::datalink::{self, NetworkInterface};
use pnet::ipnetwork::IpNetwork;
use pnet::util::MacAddr;
use std::fs;
use std::net::Ipv4Addr;

/// Everything the engine needs to know about where it's operating.
#[derive(Debug, Clone)]
pub struct NetCtx {
    pub iface: NetworkInterface,
    pub our_ip: Ipv4Addr,
    pub our_mac: MacAddr,
    pub netmask: Ipv4Addr,
    pub gateway_ip: Ipv4Addr,
}

impl NetCtx {
    /// Build a context for the given interface name, or auto-pick the interface
    /// that carries the default route.
    pub fn resolve(iface_name: Option<&str>) -> Result<Self> {
        let (gateway_ip, default_iface) =
            default_route().context("could not read default route from /proc/net/route")?;

        let want = iface_name.unwrap_or(&default_iface);
        let iface = datalink::interfaces()
            .into_iter()
            .find(|i| i.name == want)
            .ok_or_else(|| anyhow!("interface '{}' not found", want))?;

        let our_mac = iface
            .mac
            .ok_or_else(|| anyhow!("interface '{}' has no MAC address", iface.name))?;

        let (our_ip, netmask) = iface
            .ips
            .iter()
            .find_map(|ip| match ip {
                IpNetwork::V4(v4) => Some((v4.ip(), v4.mask())),
                _ => None,
            })
            .ok_or_else(|| anyhow!("interface '{}' has no IPv4 address", iface.name))?;

        Ok(Self {
            iface,
            our_ip,
            our_mac,
            netmask,
            gateway_ip,
        })
    }

    /// All host IPv4 addresses in our subnet, excluding network/broadcast and
    /// our own address.
    pub fn subnet_hosts(&self) -> Vec<Ipv4Addr> {
        let ip = u32::from(self.our_ip);
        let mask = u32::from(self.netmask);
        let network = ip & mask;
        let broadcast = network | !mask;

        let mut hosts = Vec::new();
        let mut addr = network.wrapping_add(1);
        while addr < broadcast {
            let candidate = Ipv4Addr::from(addr);
            if candidate != self.our_ip {
                hosts.push(candidate);
            }
            addr = addr.wrapping_add(1);
        }
        hosts
    }
}

/// Format a MAC the way the UI expects (lowercase, colon-separated).
pub fn mac_string(mac: MacAddr) -> String {
    mac.to_string()
}

/// Parse `/proc/net/route` for the default gateway and its interface.
fn default_route() -> Result<(Ipv4Addr, String)> {
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
