#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod discovery;
mod engine;
mod state;
mod throttle;
mod updater;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::info;

use state::{DeviceInfo, DevicePolicy, SharedState};
use updater::UpdateState;

// ── DTOs for the UI ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DeviceDto {
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub vendor: String,
    pub policy: String,
    pub download_limit_kbps: Option<u64>,
    pub upload_limit_kbps: Option<u64>,
    pub download_bytes: u64,
    pub upload_bytes: u64,
}

impl From<&DeviceInfo> for DeviceDto {
    fn from(d: &DeviceInfo) -> Self {
        let (policy_str, dl, ul) = match d.policy {
            DevicePolicy::Allow => ("allow".into(), None, None),
            DevicePolicy::Throttle {
                download_bps,
                upload_bps,
            } => (
                "throttle".into(),
                Some(download_bps * 8 / 1000),
                Some(upload_bps * 8 / 1000),
            ),
            DevicePolicy::Block => ("block".into(), None, None),
        };
        DeviceDto {
            mac: d.mac_string(),
            ip: d.ip.to_string(),
            hostname: d.hostname.clone(),
            vendor: d.vendor.clone(),
            policy: policy_str,
            download_limit_kbps: dl,
            upload_limit_kbps: ul,
            download_bytes: d.download_bytes,
            upload_bytes: d.upload_bytes,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsDto {
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
    pub device_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum PolicyPayload {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "throttle")]
    Throttle {
        download_kbps: u64,
        upload_kbps: u64,
    },
    #[serde(rename = "block")]
    Block,
}

// ── Tauri Commands ──────────────────────────────────────────────────────────

#[tauri::command]
async fn get_devices(
    shared: tauri::State<'_, Arc<SharedState>>,
) -> Result<Vec<DeviceDto>, String> {
    Ok(shared
        .snapshot_devices()
        .iter()
        .map(DeviceDto::from)
        .collect())
}

#[tauri::command]
async fn get_stats(
    shared: tauri::State<'_, Arc<SharedState>>,
) -> Result<StatsDto, String> {
    let devices = shared.snapshot_devices();
    Ok(StatsDto {
        total_download_bytes: devices.iter().map(|d| d.download_bytes).sum(),
        total_upload_bytes: devices.iter().map(|d| d.upload_bytes).sum(),
        device_count: devices.len(),
    })
}

#[tauri::command]
async fn set_policy(
    shared: tauri::State<'_, Arc<SharedState>>,
    mac: String,
    policy: PolicyPayload,
) -> Result<(), String> {
    let mac_bytes = parse_mac(&mac).ok_or_else(|| format!("Invalid MAC: {mac}"))?;
    let device_policy = match policy {
        PolicyPayload::Allow => DevicePolicy::Allow,
        PolicyPayload::Throttle {
            download_kbps,
            upload_kbps,
        } => DevicePolicy::Throttle {
            download_bps: download_kbps * 1000 / 8,
            upload_bps: upload_kbps * 1000 / 8,
        },
        PolicyPayload::Block => DevicePolicy::Block,
    };
    shared.set_policy(&mac_bytes, device_policy);
    Ok(())
}

#[tauri::command]
async fn trigger_scan(
    shared: tauri::State<'_, Arc<SharedState>>,
) -> Result<(), String> {
    shared.request_scan();
    Ok(())
}

#[tauri::command]
async fn check_update(
    us: tauri::State<'_, Arc<UpdateState>>,
) -> Result<updater::UpdateInfo, String> {
    Ok(us.info.read().clone())
}

#[tauri::command]
async fn apply_update(
    us: tauri::State<'_, Arc<UpdateState>>,
) -> Result<(), String> {
    let info = us.info.read().clone();
    if !info.available || info.download_url.is_empty() {
        return Err("No update available".into());
    }
    let url = info.download_url.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = updater::apply_update(&url).await {
            tracing::error!("Update failed: {e:#}");
        }
    });
    Ok(())
}

fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

// ── Entry point ─────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gos3lih=debug,info".into()),
        )
        .init();

    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::*;
        SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
    }

    let shared_state = Arc::new(SharedState::new());
    let update_state = Arc::new(UpdateState::new());

    let ss = shared_state.clone();
    let us = update_state.clone();

    tauri::Builder::default()
        .manage(shared_state)
        .manage(update_state)
        .setup(move |_app| {
            let s1 = ss.clone();
            let s2 = ss.clone();
            let s3 = ss.clone();
            let us1 = us.clone();

            info!("Starting backend subsystems…");

            tauri::async_runtime::spawn(async move {
                if let Err(e) = engine::run_packet_engine(s1).await {
                    tracing::error!("Packet engine error: {e:#}");
                }
            });

            tauri::async_runtime::spawn(async move {
                if let Err(e) = discovery::run_discovery_loop(s2).await {
                    tracing::error!("Discovery error: {e:#}");
                }
            });

            tauri::async_runtime::spawn(async move {
                if let Err(e) = updater::run_update_checker(s3, us1).await {
                    tracing::error!("Updater error: {e:#}");
                }
            });

            info!("All subsystems started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_devices,
            get_stats,
            set_policy,
            trigger_scan,
            check_update,
            apply_update,
        ])
        .run(tauri::generate_context!())
        .expect("error running Gos3lih");
}
