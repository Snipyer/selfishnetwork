//! Shared application state: network identity, the device table (UI model),
//! the data-plane engine, and the event broadcast channel.

use crate::engine::Engine;
use crate::net::arp::Peer;
use crate::net::NetCtx;
use crate::protocol::{Device, Endpoint, Event, Throttle};
use pnet::util::MacAddr;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Mutex;
use tokio::sync::broadcast;

const EVENT_CHANNEL_CAP: usize = 256;

/// Everything a WS connection and the engine share. Wrap in an `Arc`.
pub struct App {
    pub ctx: NetCtx,
    pub gateway: Endpoint,
    pub own: Endpoint,
    pub engine: Engine,
    pub events: broadcast::Sender<Event>,
    devices: Mutex<HashMap<Ipv4Addr, Device>>,
}

impl App {
    pub fn new(ctx: NetCtx, gateway_mac: Option<MacAddr>) -> Self {
        let own = Endpoint {
            ip: ctx.our_ip,
            mac: ctx.our_mac.to_string(),
        };
        let gateway = Endpoint {
            ip: ctx.gateway_ip,
            mac: gateway_mac.map(|m| m.to_string()).unwrap_or_default(),
        };
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAP);

        // The engine needs a concrete gateway MAC; fall back to broadcast if we
        // couldn't resolve it (poisoning still works, forwarding to gateway may
        // not until a scan learns it).
        let gw_peer = Peer {
            ip: ctx.gateway_ip,
            mac: gateway_mac.unwrap_or(MacAddr::broadcast()),
        };
        let engine = Engine::new(ctx.clone(), gw_peer, events.clone());

        Self {
            ctx,
            gateway,
            own,
            engine,
            events,
            devices: Mutex::new(HashMap::new()),
        }
    }

    /// Insert or refresh a discovered device, preserving control state.
    pub fn upsert(&self, ip: Ipv4Addr, mac: MacAddr) -> Device {
        let mut devices = self.devices.lock().unwrap();
        let entry = devices.entry(ip).or_insert_with(|| Device {
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

    pub fn snapshot(&self) -> Vec<Device> {
        self.devices.lock().unwrap().values().cloned().collect()
    }

    /// Apply a mutation to a device's UI model, returning the updated copy.
    pub fn update_device<F: FnOnce(&mut Device)>(&self, ip: &Ipv4Addr, f: F) -> Option<Device> {
        let mut devices = self.devices.lock().unwrap();
        let dev = devices.get_mut(ip)?;
        f(dev);
        Some(dev.clone())
    }

    /// MAC of a known device, parsed back from its stored string.
    pub fn device_mac(&self, ip: &Ipv4Addr) -> Option<MacAddr> {
        let devices = self.devices.lock().unwrap();
        devices.get(ip)?.mac.parse().ok()
    }

    /// Publish an event to all connected clients.
    pub fn broadcast(&self, event: Event) {
        let _ = self.events.send(event);
    }
}
