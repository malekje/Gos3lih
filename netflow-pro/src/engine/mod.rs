//! Packet Engine — the "Data Plane".
//!
//! Opens WinDivert with filter `true` (capture everything), runs a
//! multi-threaded packet loop that:
//!   1. Reads a packet + address from WinDivert.
//!   2. Extracts src/dst IPv4.
//!   3. Checks the shared state for the device's policy.
//!   4. For `Allow`  → re-inject immediately.
//!      For `Block`  → drop (don't re-inject).
//!      For `Throttle` → consult the token bucket; sleep if needed, then re-inject.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use windivert::prelude::*;

use crate::state::{DevicePolicy, SharedState};
use crate::throttle::{BucketRegistry, Direction};

/// Size of the packet buffer. WinDivert can capture jumbo frames; 64 KB is safe.
const PACKET_BUF_SIZE: usize = 65_535;

/// Number of worker threads pulling packets from the WinDivert handle.
const WORKER_THREADS: usize = 4;

/// Main entry-point: spawns worker threads and waits for shutdown.
pub async fn run_packet_engine(state: Arc<SharedState>) -> Result<()> {
    info!("Packet engine starting — opening WinDivert handle");

    // Open WinDivert on the NETWORK layer with filter "true" (all traffic).
    // Priority 0 (default). Flags = 0 (synchronous, no drop, no sniff).
    let handle = windivert::WinDivert::network(
        "true",                          // filter string — capture all
        0,                               // priority
        WinDivertFlags::new(),           // no special flags
    )
    .context("Failed to open WinDivert handle (is WinDivert.dll + .sys in PATH?)")?;

    let handle = Arc::new(handle);
    let buckets = Arc::new(BucketRegistry::new());

    let mut workers = Vec::with_capacity(WORKER_THREADS);
    for id in 0..WORKER_THREADS {
        let h = Arc::clone(&handle);
        let s = Arc::clone(&state);
        let b = Arc::clone(&buckets);
        workers.push(tokio::task::spawn_blocking(move || {
            worker_loop(id, h, s, b);
        }));
    }

    info!("{WORKER_THREADS} packet-engine workers running");

    // Wait until shutdown is requested.
    while !state.is_shutdown() {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    info!("Packet engine shutting down");
    // WinDivert handle is closed when Arc<WinDivert> drops — workers will
    // exit because recv() will fail.
    drop(handle);

    for w in workers {
        let _ = w.await;
    }

    info!("Packet engine stopped");
    Ok(())
}

/// A blocking worker loop that pulls packets from WinDivert.
fn worker_loop(
    id: usize,
    handle: Arc<windivert::WinDivert<windivert::layer::NetworkLayer>>,
    state: Arc<SharedState>,
    buckets: Arc<BucketRegistry>,
) {
    info!("Worker {id} started");

    let mut buf = vec![0u8; PACKET_BUF_SIZE];

    loop {
        if state.is_shutdown() {
            break;
        }

        // Blocking receive — fills buf and returns (packet_len, address).
        let recv_result = handle.recv(Some(&mut buf));

        let packet = match recv_result {
            Ok(p) => p,
            Err(e) => {
                if state.is_shutdown() {
                    break;
                }
                warn!("Worker {id}: WinDivert recv error: {e}");
                continue;
            }
        };

        let data = &packet.data;
        let data_len = data.len() as u64;

        // Parse IPv4 header to get src/dst IP.
        let (src_ip, dst_ip) = match parse_ipv4_addrs(data) {
            Some(addrs) => addrs,
            None => {
                // Non-IPv4 or malformed — re-inject as-is.
                let _ = handle.send(&packet);
                continue;
            }
        };

        // Determine direction and the "other" device IP.
        // If the packet is coming TO a local device → download for dst.
        // If the packet is going FROM a local device → upload for src.
        // We check both directions against the throttled list.

        let policy_dst = state.policy_for_ip(&dst_ip);
        let policy_src = state.policy_for_ip(&src_ip);

        // --- Download path (destination is a monitored device) ---
        if let Some(policy) = policy_dst {
            match policy {
                DevicePolicy::Block => {
                    // Silently drop
                    debug!("Dropping packet to {dst_ip} (blocked)");
                    continue;
                }
                DevicePolicy::Throttle {
                    download_bps,
                    upload_bps: _,
                } => {
                    state.record_download(&dst_ip, data_len);
                    let bucket = buckets.get_or_create(dst_ip, Direction::Download, download_bps);
                    if let Err(delay) = bucket.consume(data_len) {
                        // Sleep to shape traffic — delay rather than drop.
                        std::thread::sleep(delay);
                    }
                }
                DevicePolicy::Allow => {
                    state.record_download(&dst_ip, data_len);
                }
            }
        }

        // --- Upload path (source is a monitored device) ---
        if let Some(policy) = policy_src {
            match policy {
                DevicePolicy::Block => {
                    debug!("Dropping packet from {src_ip} (blocked)");
                    continue;
                }
                DevicePolicy::Throttle {
                    download_bps: _,
                    upload_bps,
                } => {
                    state.record_upload(&src_ip, data_len);
                    let bucket = buckets.get_or_create(src_ip, Direction::Upload, upload_bps);
                    if let Err(delay) = bucket.consume(data_len) {
                        std::thread::sleep(delay);
                    }
                }
                DevicePolicy::Allow => {
                    state.record_upload(&src_ip, data_len);
                }
            }
        }

        // Re-inject the packet.
        if let Err(e) = handle.send(&packet) {
            warn!("Worker {id}: WinDivert send error: {e}");
        }
    }

    info!("Worker {id} stopped");
}

// ---------------------------------------------------------------------------
// Minimal IPv4 header parser (no external dependency on the hot path)
// ---------------------------------------------------------------------------

use std::net::Ipv4Addr;

/// Extract source and destination IPv4 addresses from a raw IP packet.
/// Returns `None` for non-IPv4 packets.
fn parse_ipv4_addrs(data: &[u8]) -> Option<(Ipv4Addr, Ipv4Addr)> {
    if data.len() < 20 {
        return None;
    }
    // Version must be 4.
    let version = (data[0] >> 4) & 0x0F;
    if version != 4 {
        return None;
    }
    let src = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
    let dst = Ipv4Addr::new(data[16], data[17], data[18], data[19]);
    Some((src, dst))
}
