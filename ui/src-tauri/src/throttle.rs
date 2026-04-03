//! Token Bucket Filter — the traffic shaping core.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

pub struct TokenBucket {
    inner: Mutex<BucketInner>,
}

struct BucketInner {
    capacity: u64,
    tokens: f64,
    rate_bps: u64,
    last_refill: Instant,
}

impl TokenBucket {
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

    pub fn set_rate(&self, rate_bps: u64) {
        let mut b = self.inner.lock();
        b.rate_bps = rate_bps;
    }

    pub fn consume(&self, bytes: u64) -> Result<(), Duration> {
        let mut b = self.inner.lock();

        let now = Instant::now();
        let elapsed = now.duration_since(b.last_refill).as_secs_f64();
        b.tokens = (b.tokens + elapsed * b.rate_bps as f64).min(b.capacity as f64);
        b.last_refill = now;

        if b.tokens >= bytes as f64 {
            b.tokens -= bytes as f64;
            Ok(())
        } else {
            let deficit = bytes as f64 - b.tokens;
            let wait_secs = deficit / b.rate_bps as f64;
            b.tokens -= bytes as f64;
            Err(Duration::from_secs_f64(wait_secs))
        }
    }
}

use dashmap::DashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Direction {
    Download,
    Upload,
}

pub struct BucketRegistry {
    buckets: DashMap<(Ipv4Addr, Direction), Arc<TokenBucket>>,
}

impl BucketRegistry {
    pub fn new() -> Self {
        Self {
            buckets: DashMap::new(),
        }
    }

    pub fn get_or_create(
        &self,
        ip: Ipv4Addr,
        direction: Direction,
        rate_bps: u64,
    ) -> Arc<TokenBucket> {
        self.buckets
            .entry((ip, direction))
            .or_insert_with(|| {
                let burst = rate_bps.max(65_536);
                Arc::new(TokenBucket::new(rate_bps, burst))
            })
            .value()
            .clone()
    }
}
