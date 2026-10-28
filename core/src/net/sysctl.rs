//! Kernel knobs we must toggle for userspace forwarding, with restore-on-drop.
//!
//! - `net.ipv4.ip_forward = 0` — we forward in userspace, so the kernel must
//!   NOT also forward (that would duplicate every packet).
//! - `net.ipv4.conf.{all,<iface>}.send_redirects = 0` — stop the kernel from
//!   emitting ICMP redirects that would tell our victims the real gateway and
//!   unravel the MITM.
//!
//! Writing these `/proc/sys` files is why the core needs `cap_dac_override`
//! (the files are root-owned) in addition to `cap_net_admin`.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::warn;

/// Holds the original values so [`restore`](SysctlGuard::restore) can put them
/// back. Also restores on drop as a safety net.
pub struct SysctlGuard {
    saved: Vec<(PathBuf, String)>,
    restored: bool,
}

impl SysctlGuard {
    /// Read-then-set the sysctls for `iface`, remembering prior values.
    pub fn apply(iface: &str) -> Result<Self> {
        let mut guard = SysctlGuard {
            saved: Vec::new(),
            restored: false,
        };
        let targets = [
            ("/proc/sys/net/ipv4/ip_forward".to_string(), "0"),
            ("/proc/sys/net/ipv4/conf/all/send_redirects".to_string(), "0"),
            (
                format!("/proc/sys/net/ipv4/conf/{iface}/send_redirects"),
                "0",
            ),
        ];
        for (path, want) in targets {
            if let Err(e) = guard.set_one(&path, want) {
                // Roll back anything we already changed, then fail.
                guard.restore();
                return Err(e);
            }
        }
        Ok(guard)
    }

    fn set_one(&mut self, path: &str, value: &str) -> Result<()> {
        let pb = PathBuf::from(path);
        let original = fs::read_to_string(&pb)
            .with_context(|| format!("reading sysctl {path}"))?
            .trim()
            .to_string();
        fs::write(&pb, format!("{value}\n"))
            .with_context(|| format!("writing sysctl {path} (need cap_dac_override?)"))?;
        self.saved.push((pb, original));
        Ok(())
    }

    /// Put every changed sysctl back to its original value (best effort).
    pub fn restore(&mut self) {
        if self.restored {
            return;
        }
        for (path, original) in self.saved.iter().rev() {
            if let Err(e) = fs::write(path, format!("{original}\n")) {
                warn!("failed to restore {}: {e}", path.display());
            }
        }
        self.restored = true;
    }
}

impl Drop for SysctlGuard {
    fn drop(&mut self) {
        self.restore();
    }
}
