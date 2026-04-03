//! Token Bucket Filter — the traffic shaping core.
//!
//! Each bucket tracks available "tokens" (bytes). Tokens refill at the
//! configured rate. A packet may be sent only when enough tokens are available;
//! otherwise the caller sleeps until the bucket refills enough to cover it.
//! This delays packets rather than dropping them, preserving TCP stability.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// A thread-safe token bucket.
pub struct TokenBucket {
    inner: Mutex<BucketInner>,
}

struct BucketInner {
    /// Maximum burst capacity in bytes.
    capacity: u64,
    /// Currently available tokens (bytes).
    tokens: f64,
    /// Refill rate in bytes per second.
    rate_bps: u64,
    /// Last time tokens were refilled.
    last_refill: Instant,
}

impl TokenBucket {
    /// Create a new bucket.
    ///
    /// * `rate_bps`  — sustained rate in **bytes per second**.
    /// * `burst`     — maximum burst size in bytes (typically 1.5× MTU or more).
    pub fn new(rate_bps: u64, burst: u64) -> Self {
        Self {
            inner: Mutex::new(BucketInner {
                capacity: burst,
                tokens: burst as f64,
                rate_bps,
                last_refill: Instant::now(),
            }),
        }
    }

    /// Update the rate limit dynamically (e.g. user adjusted the slider).
    pub fn set_rate(&self, rate_bps: u64) {
        let mut b = self.inner.lock();
        b.rate_bps = rate_bps;
        // Don't touch tokens — they'll naturally adjust on the next refill.
    }

    /// Try to consume `bytes` tokens.
    ///
    /// Returns `Ok(())` if the packet can be re-injected immediately, or
    /// `Err(delay)` with the [`Duration`] the caller should wait before
    /// re-injecting the packet.
    pub fn consume(&self, bytes: u64) -> Result<(), Duration> {
        let mut b = self.inner.lock();

        // Refill tokens based on elapsed time.
        let now = Instant::now();
        let elapsed = now.duration_since(b.last_refill).as_secs_f64();
        b.tokens = (b.tokens + elapsed * b.rate_bps as f64).min(b.capacity as f64);
        b.last_refill = now;

        if b.tokens >= bytes as f64 {
            b.tokens -= bytes as f64;
            Ok(())
        } else {
            // How long until we have enough tokens?
            let deficit = bytes as f64 - b.tokens;
            let wait_secs = deficit / b.rate_bps as f64;
            // Pre-deduct the tokens so concurrent callers queue correctly.
            b.tokens -= bytes as f64; // may go negative — that's intentional
            Err(Duration::from_secs_f64(wait_secs))
        }
    }

    /// Current fill ratio (0.0 – 1.0), useful for UI gauges.
    pub fn fill_ratio(&self) -> f64 {
        let b = self.inner.lock();
        (b.tokens.max(0.0) / b.capacity as f64).min(1.0)
    }
}

// ---------------------------------------------------------------------------
// Per-device bucket registry
// ---------------------------------------------------------------------------

use dashmap::DashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

/// Direction of traffic relative to the local device.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Direction {
    Download,
    Upload,
}

/// Manages a pair of token buckets (download + upload) per IP.
pub struct BucketRegistry {
    buckets: DashMap<(Ipv4Addr, Direction), Arc<TokenBucket>>,
}

impl BucketRegistry {
    pub fn new() -> Self {
        Self {
            buckets: DashMap::new(),
        }
    }

    /// Retrieve or create a bucket for `(ip, direction)`.
    pub fn get_or_create(
        &self,
        ip: Ipv4Addr,
        direction: Direction,
        rate_bps: u64,
    ) -> Arc<TokenBucket> {
        self.buckets
            .entry((ip, direction))
            .or_insert_with(|| {
                // Burst = 64 KB or 1 second of bandwidth, whichever is larger.
                let burst = rate_bps.max(65_536);
                Arc::new(TokenBucket::new(rate_bps, burst))
            })
            .value()
            .clone()
    }

    /// Remove all buckets for an IP (e.g. policy changed to Allow/Block).
    pub fn remove(&self, ip: &Ipv4Addr) {
        self.buckets.remove(&(*ip, Direction::Download));
        self.buckets.remove(&(*ip, Direction::Upload));
    }

    /// Update the rate for existing buckets.
    pub fn update_rate(&self, ip: &Ipv4Addr, direction: Direction, rate_bps: u64) {
        if let Some(b) = self.buckets.get(&(*ip, direction)) {
            b.set_rate(rate_bps);
        }
    }
}
