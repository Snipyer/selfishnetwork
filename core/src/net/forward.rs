//! Userspace forwarding + per-device throttling.
//!
//! Once a device is MITM'd, its frames arrive at our NIC (dst MAC = ours) with
//! an IP that isn't ours. With `ip_forward=0` the kernel won't forward them, so
//! we do it: capture the frame, rewrite L2 (src = us, dst = the real next hop),
//! meter it through the device's [`TokenBucket`], then re-inject.
//!
//! - **cut**  → drop every frame for the device.
//! - **throttle** → tokens refill at the configured rate; a frame waits (or is
//!   dropped) when the bucket is empty.
//! - **unlimited** → forward immediately, just accounting bytes for stats.
//!
//! The capture/rewrite/re-inject loop is the next thing to implement; the
//! rate-limiter it depends on is done and unit-tested below.

use std::time::Instant;

/// Classic token bucket. `rate_bps` tokens (bytes) accrue per second up to
/// `capacity`. A bucket with `rate_bps == None` is unlimited.
#[derive(Debug)]
pub struct TokenBucket {
    rate_bps: Option<f64>,
    capacity: f64,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    /// `rate_kbps` in kilobits/s (as the UI sends it); `None` = unlimited.
    pub fn new(rate_kbps: Option<u32>) -> Self {
        let rate_bps = rate_kbps.map(|k| (k as f64) * 1000.0 / 8.0);
        // Burst tolerance: ~250ms of traffic, min one MTU.
        let capacity = rate_bps.map(|r| (r * 0.25).max(1500.0)).unwrap_or(0.0);
        Self {
            rate_bps,
            capacity,
            tokens: capacity,
            last: Instant::now(),
        }
    }

    fn refill(&mut self, now: Instant) {
        if let Some(rate) = self.rate_bps {
            let elapsed = now.duration_since(self.last).as_secs_f64();
            self.tokens = (self.tokens + elapsed * rate).min(self.capacity);
        }
        self.last = now;
    }

    /// Try to spend `bytes`. Returns `true` if allowed to send now.
    /// Unlimited buckets always allow.
    pub fn try_consume(&mut self, bytes: usize) -> bool {
        if self.rate_bps.is_none() {
            return true;
        }
        self.refill(Instant::now());
        if self.tokens >= bytes as f64 {
            self.tokens -= bytes as f64;
            true
        } else {
            false
        }
    }

    pub fn set_rate(&mut self, rate_kbps: Option<u32>) {
        *self = TokenBucket::new(rate_kbps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_always_allows() {
        let mut b = TokenBucket::new(None);
        assert!(b.try_consume(1_000_000));
        assert!(b.try_consume(1_000_000));
    }

    #[test]
    fn limited_bucket_empties_then_blocks() {
        // 8 kbps = 1000 bytes/s, capacity >= 1500.
        let mut b = TokenBucket::new(Some(8));
        // First full-capacity spend should pass.
        assert!(b.try_consume(1500));
        // Immediately asking for another 1500 with an empty-ish bucket fails.
        assert!(!b.try_consume(1500));
    }
}
