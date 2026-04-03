//! Shared application state — the "Control Plane" brain.
//!
//! A concurrent HashMap keyed by MAC address holds per-device config and live
//! counters. All subsystems share an `Arc<SharedState>`.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Per-device record
// ---------------------------------------------------------------------------

/// Unique device identifier (MAC address as 6 bytes).
pub type MacAddr = [u8; 6];

/// Actions the control plane can request for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DevicePolicy {
    /// Traffic flows normally.
    Allow,
    /// Traffic is shaped to the given limits (bytes/sec).
    Throttle {
        download_bps: u64,
        upload_bps: u64,
    },
    /// All traffic is silently dropped.
    Block,
}

impl Default for DevicePolicy {
    fn default() -> Self {
        Self::Allow
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub hostname: String,
    pub vendor: String,
    pub policy: DevicePolicy,

    // Live counters (bytes since last reset window)
    pub download_bytes: u64,
    pub upload_bytes: u64,

    /// Timestamp of last packet seen from/to this device.
    #[serde(skip)]
    pub last_seen: Option<Instant>,
}

impl DeviceInfo {
    pub fn new(mac: MacAddr, ip: Ipv4Addr) -> Self {
        Self {
            mac,
            ip,
            hostname: String::new(),
            vendor: String::new(),
            policy: DevicePolicy::default(),
            download_bytes: 0,
            upload_bytes: 0,
            last_seen: Some(Instant::now()),
        }
    }

    pub fn mac_string(&self) -> String {
        format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac[0], self.mac[1], self.mac[2],
            self.mac[3], self.mac[4], self.mac[5],
        )
    }
}

// ---------------------------------------------------------------------------
// Global shared state
// ---------------------------------------------------------------------------

pub struct SharedState {
    /// Key = MAC address, Value = mutable device info.
    pub devices: DashMap<MacAddr, DeviceInfo>,

    /// IP → MAC reverse lookup for the fast path (packet engine).
    pub ip_to_mac: DashMap<Ipv4Addr, MacAddr>,

    /// The local gateway IP (detected on start).
    pub gateway_ip: parking_lot::RwLock<Option<Ipv4Addr>>,

    /// Subnet to scan (e.g. 192.168.1.0, mask 24).
    pub subnet: parking_lot::RwLock<(Ipv4Addr, u8)>,

    /// Shutdown flag shared across subsystems.
    shutdown: AtomicBool,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            devices: DashMap::new(),
            ip_to_mac: DashMap::new(),
            gateway_ip: parking_lot::RwLock::new(None),
            subnet: parking_lot::RwLock::new((Ipv4Addr::new(192, 168, 1, 0), 24)),
            shutdown: AtomicBool::new(false),
        }
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Upsert a device. If it already exists, update IP/last_seen but keep policy.
    pub fn upsert_device(&self, mac: MacAddr, ip: Ipv4Addr) {
        self.ip_to_mac.insert(ip, mac);

        self.devices
            .entry(mac)
            .and_modify(|d| {
                d.ip = ip;
                d.last_seen = Some(Instant::now());
            })
            .or_insert_with(|| DeviceInfo::new(mac, ip));
    }

    /// Look up a device policy by IP (hot path — must be fast).
    pub fn policy_for_ip(&self, ip: &Ipv4Addr) -> Option<DevicePolicy> {
        let mac = self.ip_to_mac.get(ip)?;
        let dev = self.devices.get(&*mac)?;
        Some(dev.policy)
    }

    /// Record bandwidth towards a device.
    pub fn record_download(&self, ip: &Ipv4Addr, bytes: u64) {
        if let Some(mac) = self.ip_to_mac.get(ip) {
            if let Some(mut dev) = self.devices.get_mut(&*mac) {
                dev.download_bytes = dev.download_bytes.saturating_add(bytes);
                dev.last_seen = Some(Instant::now());
            }
        }
    }

    /// Record bandwidth from a device.
    pub fn record_upload(&self, ip: &Ipv4Addr, bytes: u64) {
        if let Some(mac) = self.ip_to_mac.get(ip) {
            if let Some(mut dev) = self.devices.get_mut(&*mac) {
                dev.upload_bytes = dev.upload_bytes.saturating_add(bytes);
                dev.last_seen = Some(Instant::now());
            }
        }
    }

    /// Snapshot all devices for the UI.
    pub fn snapshot_devices(&self) -> Vec<DeviceInfo> {
        self.devices.iter().map(|r| r.value().clone()).collect()
    }

    /// Set the policy for a device identified by MAC.
    pub fn set_policy(&self, mac: &MacAddr, policy: DevicePolicy) {
        if let Some(mut dev) = self.devices.get_mut(mac) {
            dev.policy = policy;
        }
    }
}
