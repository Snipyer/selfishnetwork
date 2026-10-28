//! Wire types shared with the UI. See `docs/protocol.md`.

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// Commands sent by the UI to the core.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// (Re)discover devices on the LAN via an ARP sweep.
    Scan,
    /// Begin MITM interception of a device.
    Intercept { ip: Ipv4Addr },
    /// Stop intercepting and restore the device's ARP cache.
    Release { ip: Ipv4Addr },
    /// Fully block (or unblock) a device's traffic.
    Cut { ip: Ipv4Addr, enabled: bool },
    /// Rate-limit a device. `None` means unlimited in that direction.
    Throttle {
        ip: Ipv4Addr,
        down_kbps: Option<u32>,
        up_kbps: Option<u32>,
    },
    /// Request a full snapshot of current state.
    Status,
}

/// Events pushed by the core to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Sent once on connect: which interface we're on and our own identity.
    Hello {
        iface: String,
        gateway: Endpoint,
        #[serde(rename = "self")]
        own: Endpoint,
    },
    ScanStarted,
    Device { device: Device },
    ScanDone { count: usize },
    DeviceUpdate { device: Device },
    Stats {
        ip: Ipv4Addr,
        down_bps: u64,
        up_bps: u64,
        down_total: u64,
        up_total: u64,
    },
    Error { message: String },
}

/// An IP + MAC pair (gateway or ourselves).
#[derive(Debug, Clone, Serialize)]
pub struct Endpoint {
    pub ip: Ipv4Addr,
    pub mac: String,
}

/// Throttle settings for a device; `None` = unlimited.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Throttle {
    pub down_kbps: Option<u32>,
    pub up_kbps: Option<u32>,
}

impl Throttle {
    pub fn is_unlimited(&self) -> bool {
        self.down_kbps.is_none() && self.up_kbps.is_none()
    }
}

/// A discovered device and its current interception state.
#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub ip: Ipv4Addr,
    pub mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    pub intercepted: bool,
    pub cut: bool,
    #[serde(skip_serializing_if = "Throttle::is_unlimited")]
    pub throttle: Throttle,
}
