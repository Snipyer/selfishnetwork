//! Network context: interface selection, our own identity, gateway detection.

pub mod arp;
pub mod forward;
pub mod scan;

use crate::platform::default_route;
use anyhow::{anyhow, Context, Result};
use pnet::datalink::{self, NetworkInterface};
use pnet::ipnetwork::IpNetwork;
use pnet::util::MacAddr;
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
            default_route().context("could not determine the default route")?;

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

    /// A human-friendly label for the interface. On Windows the raw `name` is a
    /// `\Device\NPF_{GUID}` string, so we prefer the adapter description; on
    /// Linux the description is empty and the name (e.g. `enp5s0`) is friendly.
    pub fn iface_label(&self) -> String {
        let desc = self.iface.description.trim();
        if desc.is_empty() {
            self.iface.name.clone()
        } else {
            desc.to_string()
        }
    }

    /// True if `ip` is a real unicast host on our subnet — i.e. a plausible
    /// device. Filters out multicast/broadcast/loopback and the subnet's own
    /// network and broadcast addresses, which otherwise leak in from the OS
    /// neighbor table (e.g. `224.0.0.22`, `239.255.255.250` on Windows).
    pub fn is_lan_host(&self, ip: Ipv4Addr) -> bool {
        if ip.is_multicast() || ip.is_broadcast() || ip.is_unspecified() || ip.is_loopback() {
            return false;
        }
        let mask = u32::from(self.netmask);
        let network = u32::from(self.our_ip) & mask;
        let broadcast = network | !mask;
        let v = u32::from(ip);
        (v & mask) == network && v != network && v != broadcast
    }
}
