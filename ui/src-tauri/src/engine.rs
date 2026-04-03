//! Packet Engine — the "Data Plane".
//!
//! Opens WinDivert with filter `true`, runs multi-threaded packet loop that
//! reads packets, checks device policy, and applies Allow/Block/Throttle.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use windivert::prelude::*;

use crate::state::{DevicePolicy, SharedState};
use crate::throttle::{BucketRegistry, Direction};

const PACKET_BUF_SIZE: usize = 65_535;
const WORKER_THREADS: usize = 4;

pub async fn run_packet_engine(state: Arc<SharedState>) -> Result<()> {
    info!("Packet engine starting — opening WinDivert handle");

    let handle = windivert::WinDivert::network(
        "true",
        0,
        WinDivertFlags::new(),
    )
    .context("Failed to open WinDivert handle (is WinDivert.dll + .sys in the same folder? Running as Admin?)")?;

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

    while !state.is_shutdown() {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    info!("Packet engine shutting down");
    drop(handle);

    for w in workers {
        let _ = w.await;
    }

    info!("Packet engine stopped");
    Ok(())
}

fn worker_loop(
    id: usize,
    handle: Arc<windivert::WinDivert<windivert::layer::NetworkLayer>>,
    state: Arc<SharedState>,
    buckets: Arc<BucketRegistry>,
) {
    info!("Worker {id} started");

    loop {
        if state.is_shutdown() {
            break;
        }

        let recv_result = handle.recv(None);

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

        let (src_ip, dst_ip) = match parse_ipv4_addrs(data) {
            Some(addrs) => addrs,
            None => {
                let _ = handle.send(&packet);
                continue;
            }
        };

        let policy_dst = state.policy_for_ip(&dst_ip);
        let policy_src = state.policy_for_ip(&src_ip);

        // Download path
        if let Some(policy) = policy_dst {
            match policy {
                DevicePolicy::Block => {
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
                        std::thread::sleep(delay);
                    }
                }
                DevicePolicy::Allow => {
                    state.record_download(&dst_ip, data_len);
                }
            }
        }

        // Upload path
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

        if let Err(e) = handle.send(&packet) {
            warn!("Worker {id}: WinDivert send error: {e}");
        }
    }

    info!("Worker {id} stopped");
}

use std::net::Ipv4Addr;

fn parse_ipv4_addrs(data: &[u8]) -> Option<(Ipv4Addr, Ipv4Addr)> {
    if data.len() < 20 {
        return None;
    }
    let version = (data[0] >> 4) & 0x0F;
    if version != 4 {
        return None;
    }
    let src = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
    let dst = Ipv4Addr::new(data[16], data[17], data[18], data[19]);
    Some((src, dst))
}
