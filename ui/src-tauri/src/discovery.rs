//! Device Discovery — Windows-native approach.
//!
//! 1. Detect local IP & subnet from `ipconfig`
//! 2. Parallel ping sweep to populate the OS ARP cache
//! 3. Parse `arp -a` to get IP → MAC mappings
//!
//! **No Npcap required.** Works on any Windows machine.

use std::net::Ipv4Addr;
use std::process::Command as StdCommand;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::state::SharedState;

const SCAN_INTERVAL: Duration = Duration::from_secs(30);
const PING_CONCURRENCY: usize = 50;
const PING_TIMEOUT_MS: &str = "200";
const NETBIOS_NS_PORT: u16 = 137;

pub async fn run_discovery_loop(state: Arc<SharedState>) -> Result<()> {
    info!("Discovery engine starting (Windows-native, no Npcap needed)");

    loop {
        if state.is_shutdown() {
            break;
        }

        match perform_scan(&state).await {
            Ok(count) => info!("Scan complete: {count} device(s) found"),
            Err(e) => warn!("Discovery scan failed: {e:#}"),
        }

        for _ in 0..(SCAN_INTERVAL.as_millis() / 500) {
            if state.is_shutdown() || state.take_scan_request() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    info!("Discovery engine stopped");
    Ok(())
}

async fn perform_scan(state: &Arc<SharedState>) -> Result<usize> {
    let (local_ip, prefix) = detect_local_network()
        .context("Failed to detect local network")?;

    let mask: u32 = if prefix >= 32 { !0 } else { !0u32 << (32 - prefix as u32) };
    let subnet_ip = Ipv4Addr::from(u32::from(local_ip) & mask);
    *state.subnet.write() = (subnet_ip, prefix);

    let host_count = if prefix >= 32 { 1 } else { 1u32 << (32 - prefix as u32) };
    let base = u32::from(subnet_ip);

    info!(
        "Scanning {subnet_ip}/{prefix} (local IP: {local_ip}, up to {} hosts)",
        host_count.min(255) - 1
    );

    // Step 1: parallel ping sweep → populates the OS ARP cache
    ping_sweep(base, host_count.min(255)).await;

    // Step 2: read the ARP table
    let count = parse_arp_table(state, local_ip)?;

    // Step 3: hostname resolution for newly discovered devices
    resolve_hostnames(state).await;

    Ok(count)
}

// ── Network detection via ipconfig ──────────────────────────────────────────

fn detect_local_network() -> Result<(Ipv4Addr, u8)> {
    let output = StdCommand::new("ipconfig")
        .output()
        .context("Failed to run ipconfig")?;

    let text = String::from_utf8_lossy(&output.stdout);

    let mut found_ip: Option<Ipv4Addr> = None;

    for line in text.lines() {
        let line = line.trim();

        // Match "IPv4 Address. . . . . . . . . . . : 192.168.x.x"
        if (line.contains("IPv4") || line.contains("IP Address")) && line.contains(':') {
            if let Some(ip_str) = line.rsplit(':').next() {
                if let Ok(ip) = ip_str.trim().parse::<Ipv4Addr>() {
                    if is_private_ip(ip) {
                        found_ip = Some(ip);
                    }
                }
            }
        }

        // "Subnet Mask . . . . . . . . . . . : 255.255.255.0"
        if line.contains("Subnet Mask") && line.contains(':') {
            if let (Some(ip), Some(mask_str)) = (found_ip, line.rsplit(':').next()) {
                if let Ok(mask) = mask_str.trim().parse::<Ipv4Addr>() {
                    let prefix = u32::from(mask).count_ones() as u8;
                    info!("Detected network: {ip}/{prefix} (mask {mask})");
                    return Ok((ip, prefix));
                }
            }
        }
    }

    // If we got an IP but no mask line yet, assume /24
    if let Some(ip) = found_ip {
        info!("Detected IP {ip}, assuming /24 subnet");
        return Ok((ip, 24));
    }

    anyhow::bail!(
        "No private IPv4 found in ipconfig output. \
         Make sure you're connected to a local network."
    )
}

fn is_private_ip(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
}

// ── Ping sweep ──────────────────────────────────────────────────────────────

async fn ping_sweep(base: u32, host_count: u32) {
    use std::os::windows::process::CommandExt;
    use tokio::sync::Semaphore;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let sem = Arc::new(Semaphore::new(PING_CONCURRENCY));
    let mut tasks = Vec::new();

    for i in 1..host_count {
        let ip = Ipv4Addr::from(base + i);
        let permit = sem.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = permit.acquire().await;
            let _ = tokio::task::spawn_blocking(move || {
                let _ = StdCommand::new("ping")
                    .args(["-n", "1", "-w", PING_TIMEOUT_MS, &ip.to_string()])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            })
            .await;
        }));
    }

    for t in tasks {
        let _ = t.await;
    }

    debug!("Ping sweep done — {} hosts probed", host_count - 1);
}

// ── ARP table parsing ───────────────────────────────────────────────────────

fn parse_arp_table(state: &SharedState, local_ip: Ipv4Addr) -> Result<usize> {
    use std::os::windows::process::CommandExt;

    let output = StdCommand::new("arp")
        .args(["-a"])
        .creation_flags(0x0800_0000)
        .output()
        .context("Failed to run arp -a")?;

    let text = String::from_utf8_lossy(&output.stdout);
    let mut count = 0;

    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Typical: "  192.168.1.1          aa-bb-cc-dd-ee-ff     dynamic"
        if parts.len() >= 3 && parts[2] == "dynamic" {
            if let (Ok(ip), Some(mac)) = (parts[0].parse::<Ipv4Addr>(), parse_win_mac(parts[1]))
            {
                // Skip our own IP and broadcast
                if ip == local_ip {
                    continue;
                }
                if mac == [0xff; 6] {
                    continue;
                }
                debug!("ARP: {ip} → {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
                state.upsert_device(mac, ip);
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Parse Windows-style MAC "aa-bb-cc-dd-ee-ff" → [u8; 6].
fn parse_win_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

// ── Hostname resolution ─────────────────────────────────────────────────────

async fn resolve_hostnames(state: &SharedState) {
    let devices: Vec<_> = state
        .devices
        .iter()
        .map(|r| (r.key().clone(), r.value().ip))
        .collect();

    for (mac, ip) in devices {
        if state.is_shutdown() {
            return;
        }
        if let Some(dev) = state.devices.get(&mac) {
            if !dev.hostname.is_empty() {
                continue;
            }
        }

        // Try NetBIOS name query first
        match netbios_name_query(ip).await {
            Ok(name) if !name.is_empty() => {
                if let Some(mut dev) = state.devices.get_mut(&mac) {
                    dev.hostname = name;
                }
                continue;
            }
            _ => {}
        }

        // Fallback: reverse DNS
        if let Ok(Ok(name)) = tokio::task::spawn_blocking(move || {
            dns_lookup_reverse(ip)
        })
        .await
        {
            if !name.is_empty() {
                if let Some(mut dev) = state.devices.get_mut(&mac) {
                    dev.hostname = name;
                }
            }
        }
    }
}

async fn netbios_name_query(ip: Ipv4Addr) -> Result<String> {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    sock.set_broadcast(true)?;

    #[rustfmt::skip]
    let query: [u8; 50] = [
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x20,
        0x43, 0x4B, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x00,
        0x00, 0x21, 0x00, 0x01,
    ];

    sock.send_to(&query, (ip, NETBIOS_NS_PORT)).await?;

    let mut buf = [0u8; 1024];
    let timeout = tokio::time::timeout(Duration::from_secs(2), sock.recv_from(&mut buf)).await;

    match timeout {
        Ok(Ok((len, _))) if len > 57 => {
            let name_count = buf[56] as usize;
            if name_count > 0 && len >= 57 + 18 {
                let name_bytes = &buf[57..57 + 15];
                let name = String::from_utf8_lossy(name_bytes).trim().to_string();
                Ok(name)
            } else {
                Ok(String::new())
            }
        }
        _ => Ok(String::new()),
    }
}

fn dns_lookup_reverse(ip: Ipv4Addr) -> Result<String> {
    use std::net::ToSocketAddrs;
    let addr = format!("{ip}:0");
    match addr.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(a) = addrs.next() {
                let host = a.to_string();
                // Strip the ":0" port suffix
                Ok(host.trim_end_matches(":0").to_string())
            } else {
                Ok(String::new())
            }
        }
        Err(_) => Ok(String::new()),
    }
}
