//! Shared application state — the "Control Plane" brain.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

pub type MacAddr = [u8; 6];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DevicePolicy {
    Allow,
    Throttle { download_bps: u64, upload_bps: u64 },
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
    pub download_bytes: u64,
    pub upload_bytes: u64,
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

pub struct SharedState {
    pub devices: DashMap<MacAddr, DeviceInfo>,
    pub ip_to_mac: DashMap<Ipv4Addr, MacAddr>,
    pub gateway_ip: parking_lot::RwLock<Option<Ipv4Addr>>,
    pub subnet: parking_lot::RwLock<(Ipv4Addr, u8)>,
    shutdown: AtomicBool,
    scan_requested: AtomicBool,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            devices: DashMap::new(),
            ip_to_mac: DashMap::new(),
            gateway_ip: parking_lot::RwLock::new(None),
            subnet: parking_lot::RwLock::new((Ipv4Addr::new(192, 168, 1, 0), 24)),
            shutdown: AtomicBool::new(false),
            scan_requested: AtomicBool::new(true), // scan immediately on start
        }
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    pub fn request_scan(&self) {
        self.scan_requested.store(true, Ordering::SeqCst);
    }

    pub fn take_scan_request(&self) -> bool {
        self.scan_requested.swap(false, Ordering::SeqCst)
    }

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

    pub fn policy_for_ip(&self, ip: &Ipv4Addr) -> Option<DevicePolicy> {
        let mac = self.ip_to_mac.get(ip)?;
        let dev = self.devices.get(&*mac)?;
        Some(dev.policy)
    }

    pub fn record_download(&self, ip: &Ipv4Addr, bytes: u64) {
        if let Some(mac) = self.ip_to_mac.get(ip) {
            if let Some(mut dev) = self.devices.get_mut(&*mac) {
                dev.download_bytes = dev.download_bytes.saturating_add(bytes);
                dev.last_seen = Some(Instant::now());
            }
        }
    }

    pub fn record_upload(&self, ip: &Ipv4Addr, bytes: u64) {
        if let Some(mac) = self.ip_to_mac.get(ip) {
            if let Some(mut dev) = self.devices.get_mut(&*mac) {
                dev.upload_bytes = dev.upload_bytes.saturating_add(bytes);
                dev.last_seen = Some(Instant::now());
            }
        }
    }

    pub fn snapshot_devices(&self) -> Vec<DeviceInfo> {
        self.devices.iter().map(|r| r.value().clone()).collect()
    }

    pub fn set_policy(&self, mac: &MacAddr, policy: DevicePolicy) {
        if let Some(mut dev) = self.devices.get_mut(mac) {
            dev.policy = policy;
        }
    }
}
