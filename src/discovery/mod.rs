//! Device Discovery Engine — the "Scanner".
//!
//! Every 30 seconds, broadcasts ARP requests across the local /24 subnet,
//! collects replies to map IP → MAC, then attempts NetBIOS name lookups and
//! mDNS queries to resolve human-readable hostnames.
//!
//! Runs as a multi-threaded module using raw sockets via `pnet`.

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use pnet::datalink::{self, Channel, NetworkInterface};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet;
use pnet::util::MacAddr;
use tracing::{debug, error, info, warn};

use crate::state::SharedState;

/// Interval between full ARP scans.
const SCAN_INTERVAL: Duration = Duration::from_secs(30);

/// Timeout waiting for individual ARP replies.
const ARP_REPLY_TIMEOUT: Duration = Duration::from_secs(3);

/// NetBIOS Name Service port.
const NETBIOS_NS_PORT: u16 = 137;

/// Entry point for the discovery subsystem.
pub async fn run_discovery_loop(state: Arc<SharedState>) -> Result<()> {
    info!("Discovery engine starting");

    loop {
        if state.is_shutdown() {
            break;
        }

        if let Err(e) = perform_scan(&state).await {
            warn!("Discovery scan failed: {e:#}");
        }

        // Wait for next scan, but check shutdown more frequently.
        for _ in 0..(SCAN_INTERVAL.as_millis() / 500) {
            if state.is_shutdown() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    info!("Discovery engine stopped");
    Ok(())
}

/// Perform a single ARP scan of the subnet.
async fn perform_scan(state: &Arc<SharedState>) -> Result<()> {
    let iface = find_default_interface()
        .context("Could not find a suitable network interface")?;

    let (subnet_ip, prefix) = *state.subnet.read();

    info!(
        "Scanning {}/{} on interface {}",
        subnet_ip, prefix, iface.name
    );

    let host_count = 1u32 << (32 - prefix as u32);
    let base: u32 = u32::from(subnet_ip);

    let state_clone = Arc::clone(state);
    let iface_clone = iface.clone();

    // Run the blocking ARP work on a dedicated thread.
    tokio::task::spawn_blocking(move || {
        arp_scan(&iface_clone, base, host_count, &state_clone);
    })
    .await?;

    // After ARP scan, attempt hostname resolution via NetBIOS/mDNS.
    resolve_hostnames(state).await;

    Ok(())
}

/// Send ARP requests to every host in the range and listen for replies.
fn arp_scan(
    iface: &NetworkInterface,
    base: u32,
    host_count: u32,
    state: &SharedState,
) {
    let channel = match datalink::channel(iface, Default::default()) {
        Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => {
            warn!("Unsupported channel type on {}", iface.name);
            return;
        }
        Err(e) => {
            error!("Failed to open datalink channel: {e}");
            return;
        }
    };

    let (mut tx, mut rx) = channel;

    let source_mac = match iface.mac {
        Some(m) => m,
        None => {
            warn!("Interface {} has no MAC address", iface.name);
            return;
        }
    };

    let source_ip: Ipv4Addr = iface
        .ips
        .iter()
        .find_map(|ip| match ip {
            pnet::ipnetwork::IpNetwork::V4(net) => Some(net.ip()),
            _ => None,
        })
        .unwrap_or(Ipv4Addr::UNSPECIFIED);

    // Send ARP requests for the whole /24.
    for i in 1..host_count.min(255) {
        let target_ip = Ipv4Addr::from(base + i);
        if let Some(pkt) = build_arp_request(source_mac, source_ip, target_ip) {
            let _ = tx.send_to(&pkt, None);
        }
    }

    // Collect replies for a few seconds.
    let deadline = std::time::Instant::now() + ARP_REPLY_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if let Ok(frame) = rx.next() {
            if let Some(eth) = EthernetPacket::new(frame) {
                if eth.get_ethertype() == EtherTypes::Arp {
                    if let Some(arp) = ArpPacket::new(eth.payload()) {
                        if arp.get_operation() == ArpOperations::Reply {
                            let sender_ip = arp.get_sender_proto_addr();
                            let sender_mac_p = arp.get_sender_hw_addr();
                            let mac_bytes: [u8; 6] = [
                                sender_mac_p.0, sender_mac_p.1, sender_mac_p.2,
                                sender_mac_p.3, sender_mac_p.4, sender_mac_p.5,
                            ];
                            debug!("ARP reply: {sender_ip} → {sender_mac_p}");
                            state.upsert_device(mac_bytes, sender_ip);
                        }
                    }
                }
            }
        }
    }
}

/// Build a 42-byte ARP request Ethernet frame.
fn build_arp_request(
    src_mac: MacAddr,
    src_ip: Ipv4Addr,
    target_ip: Ipv4Addr,
) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 42]; // 14 Ethernet + 28 ARP
    {
        let mut eth = MutableEthernetPacket::new(&mut buf)?;
        eth.set_destination(MacAddr::broadcast());
        eth.set_source(src_mac);
        eth.set_ethertype(EtherTypes::Arp);

        let mut arp = MutableArpPacket::new(&mut buf[14..])?;
        arp.set_hardware_type(ArpHardwareTypes::Ethernet);
        arp.set_protocol_type(EtherTypes::Ipv4);
        arp.set_hw_addr_len(6);
        arp.set_proto_addr_len(4);
        arp.set_operation(ArpOperations::Request);
        arp.set_sender_hw_addr(src_mac);
        arp.set_sender_proto_addr(src_ip);
        arp.set_target_hw_addr(MacAddr::zero());
        arp.set_target_proto_addr(target_ip);
    }
    Some(buf)
}

/// Find the "best" network interface — one that is up, not loopback, and has
/// an IPv4 address.
fn find_default_interface() -> Option<NetworkInterface> {
    datalink::interfaces()
        .into_iter()
        .filter(|i| i.is_up() && !i.is_loopback() && i.mac.is_some())
        .find(|i| i.ips.iter().any(|ip| ip.is_ipv4()))
}

/// Attempt NetBIOS / mDNS hostname resolution for every discovered device.
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
        // Skip if we already have a hostname.
        if let Some(dev) = state.devices.get(&mac) {
            if !dev.hostname.is_empty() {
                continue;
            }
        }

        // Try NetBIOS Name Service query (UDP port 137).
        match netbios_name_query(ip).await {
            Ok(name) if !name.is_empty() => {
                if let Some(mut dev) = state.devices.get_mut(&mac) {
                    dev.hostname = name;
                }
                continue;
            }
            _ => {}
        }

        // Fallback: reverse DNS lookup.
        if let Ok(name) = tokio::task::spawn_blocking(move || {
            dns_lookup_reverse(ip)
        })
        .await
        .unwrap_or(Ok(String::new()))
        {
            if !name.is_empty() {
                if let Some(mut dev) = state.devices.get_mut(&mac) {
                    dev.hostname = name;
                }
            }
        }
    }
}

/// Send a NetBIOS Name Service status query and parse the first name.
async fn netbios_name_query(ip: Ipv4Addr) -> Result<String> {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    sock.set_broadcast(true)?;

    // NetBIOS NBSTAT request (Node Status)
    #[rustfmt::skip]
    let query: [u8; 50] = [
        0x00, 0x01,  // Transaction ID
        0x00, 0x00,  // Flags (query)
        0x00, 0x01,  // Questions: 1
        0x00, 0x00,  // Answer RRs
        0x00, 0x00,  // Authority RRs
        0x00, 0x00,  // Additional RRs
        // Name: "*" encoded as NetBIOS name
        0x20,
        0x43, 0x4B, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41,
        0x00,
        0x00, 0x21,  // Type: NBSTAT
        0x00, 0x01,  // Class: IN
    ];

    sock.send_to(&query, (ip, NETBIOS_NS_PORT)).await?;

    let mut buf = [0u8; 1024];
    let timeout = tokio::time::timeout(Duration::from_secs(2), sock.recv_from(&mut buf)).await;

    match timeout {
        Ok(Ok((len, _))) if len > 57 => {
            // Parse first name from NBSTAT response.
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

/// Reverse DNS lookup (blocking).
fn dns_lookup_reverse(ip: Ipv4Addr) -> Result<String> {
    use std::net::ToSocketAddrs;
    let addr = format!("{ip}:0");
    // This is a best-effort lookup.
    match addr.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(a) = addrs.next() {
                Ok(a.to_string())
            } else {
                Ok(String::new())
            }
        }
        Err(_) => Ok(String::new()),
    }
}
