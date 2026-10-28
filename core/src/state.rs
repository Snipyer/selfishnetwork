//! Shared, mutable session state: the device table and our network identity.

use crate::net::NetCtx;
use crate::protocol::{Device, Endpoint, Throttle};
use pnet::util::MacAddr;
use std::collections::HashMap;
use std::net::Ipv4Addr;

/// Everything the WS layer reads and mutates. Guard with a `Mutex`.
pub struct AppState {
    pub ctx: NetCtx,
    pub gateway: Endpoint,
    pub own: Endpoint,
    devices: HashMap<Ipv4Addr, Device>,
}

impl AppState {
    pub fn new(ctx: NetCtx, gateway_mac: Option<MacAddr>) -> Self {
        let own = Endpoint {
            ip: ctx.our_ip,
            mac: ctx.our_mac.to_string(),
        };
        let gateway = Endpoint {
            ip: ctx.gateway_ip,
            mac: gateway_mac.map(|m| m.to_string()).unwrap_or_default(),
        };
        Self {
            ctx,
            gateway,
            own,
            devices: HashMap::new(),
        }
    }

    /// Insert or refresh a discovered device, preserving any control state.
    /// Returns the current device record.
    pub fn upsert(&mut self, ip: Ipv4Addr, mac: MacAddr) -> Device {
        let entry = self.devices.entry(ip).or_insert_with(|| Device {
            ip,
            mac: mac.to_string(),
            hostname: None,
            vendor: None,
            intercepted: false,
            cut: false,
            throttle: Throttle::default(),
        });
        entry.mac = mac.to_string();
        entry.clone()
    }

    pub fn get(&self, ip: &Ipv4Addr) -> Option<&Device> {
        self.devices.get(ip)
    }

    pub fn snapshot(&self) -> Vec<Device> {
        self.devices.values().cloned().collect()
    }

    /// Apply a mutation to a device and return the updated copy, if it exists.
    pub fn update<F: FnOnce(&mut Device)>(&mut self, ip: &Ipv4Addr, f: F) -> Option<Device> {
        let dev = self.devices.get_mut(ip)?;
        f(dev);
        Some(dev.clone())
    }
}
