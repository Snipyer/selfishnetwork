//! Platform-specific network control.
//!
//! The data plane — packet capture/injection and ARP poisoning — runs on pnet,
//! which is cross-platform. Three things are *not* portable, and they all live
//! here behind one small interface so the rest of the core never needs a `cfg`:
//!
//! - [`ForwardingGuard`] — while we forward in userspace the OS must stop its
//!   own IP forwarding and stop emitting ICMP redirects (either would unravel
//!   the MITM). The guard applies those changes and puts them back on
//!   `restore()`/drop.
//! - [`default_route`] — the default gateway's IPv4 address and the interface
//!   that carries it.
//! - [`neighbor_table`] — the OS ARP/neighbor cache, used to seed discovery.
//!
//! Every backend exposes the same three items with these signatures:
//!
//! ```ignore
//! pub struct ForwardingGuard { /* ... */ }
//! impl ForwardingGuard {
//!     pub fn apply(iface: &str) -> anyhow::Result<Self>;
//!     pub fn restore(&mut self);
//! }
//! impl Drop for ForwardingGuard { /* calls restore() */ }
//!
//! pub fn default_route() -> anyhow::Result<(std::net::Ipv4Addr, String)>;
//! pub fn neighbor_table(iface: &str) -> Vec<(std::net::Ipv4Addr, pnet::util::MacAddr)>;
//! ```
//!
//! Linux is fully implemented. Windows is a stub that compiles but errors at
//! runtime; `docs/windows-port.md` tracks what each stub still needs.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{default_route, neighbor_table, ForwardingGuard};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{active_resolve, default_route, neighbor_table, ForwardingGuard};

#[cfg(not(any(target_os = "linux", windows)))]
compile_error!(
    "selfishnet-core supports Linux (full) and Windows (stub) only; \
     add a backend under src/platform/ to support this OS"
);
