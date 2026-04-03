//! IPC Server — Named Pipe JSON-RPC interface.
//!
//! The high-privilege backend exposes a Windows Named Pipe that the low-privilege
//! Tauri UI connects to. Messages are newline-delimited JSON.
//!
//! # Protocol
//! Request  → `IpcRequest`  (JSON + '\n')
//! Response → `IpcResponse` (JSON + '\n')

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

use crate::state::{DeviceInfo, DevicePolicy, MacAddr, SharedState};

// ---------------------------------------------------------------------------
// IPC JSON Schema
// ---------------------------------------------------------------------------

/// Requests from the UI → Backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum IpcRequest {
    /// Get the list of all discovered devices.
    #[serde(rename = "get_devices")]
    GetDevices,

    /// Set the policy for a specific device.
    #[serde(rename = "set_policy")]
    SetPolicy {
        mac: String,  // "AA:BB:CC:DD:EE:FF"
        policy: PolicyPayload,
    },

    /// Force an immediate network scan.
    #[serde(rename = "trigger_scan")]
    TriggerScan,

    /// Get global throughput stats.
    #[serde(rename = "get_stats")]
    GetStats,

    /// Ping / health check.
    #[serde(rename = "ping")]
    Ping,
}

/// Simplified policy payload from the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Responses from the Backend → UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum IpcResponse {
    #[serde(rename = "ok")]
    Ok { data: serde_json::Value },
    #[serde(rename = "error")]
    Error { message: String },
}

/// A device as presented to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDto {
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub vendor: String,
    pub policy: String,               // "allow" | "throttle" | "block"
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
                Some(download_bps / 1000 * 8), // bytes/s → kbps
                Some(upload_bps / 1000 * 8),
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

/// Global throughput stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsDto {
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
    pub device_count: usize,
}

// ---------------------------------------------------------------------------
// Named Pipe Server
// ---------------------------------------------------------------------------

/// Name of the Named Pipe (Windows) or Unix socket (dev fallback).
const PIPE_NAME: &str = r"\\.\pipe\netflow-pro-ipc";

pub async fn run_ipc_server(state: Arc<SharedState>) -> Result<()> {
    info!("IPC server starting on {PIPE_NAME}");

    loop {
        if state.is_shutdown() {
            break;
        }

        // Use interprocess crate for named pipe server.
        match accept_client(&state).await {
            Ok(()) => {}
            Err(e) => {
                warn!("IPC client error: {e:#}");
            }
        }
    }

    info!("IPC server stopped");
    Ok(())
}

/// Accept and handle a single IPC client connection.
async fn accept_client(state: &Arc<SharedState>) -> Result<()> {
    use interprocess::local_socket::{
        tokio::prelude::*,
        GenericNamespaced, ListenerOptions,
    };

    let name = PIPE_NAME.to_ns_name::<GenericNamespaced>()?;

    let listener = ListenerOptions::new()
        .name(name)
        .create_tokio()?;

    info!("IPC: waiting for client connection…");

    loop {
        if state.is_shutdown() {
            return Ok(());
        }

        // Use a timeout so we can check shutdown periodically.
        let accept = tokio::time::timeout(Duration::from_secs(2), listener.accept());
        let stream = match accept.await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                warn!("IPC accept error: {e}");
                continue;
            }
            Err(_) => continue, // timeout — loop back and check shutdown
        };

        info!("IPC: client connected");
        let state = Arc::clone(state);
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, &state).await {
                warn!("IPC client session error: {e:#}");
            }
        });
    }
}

/// Handle a single client session: read requests, write responses.
async fn handle_client(
    stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    state: &SharedState,
) -> Result<()> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        debug!("IPC recv: {line}");

        let response = match serde_json::from_str::<IpcRequest>(&line) {
            Ok(req) => process_request(req, state),
            Err(e) => IpcResponse::Error {
                message: format!("Invalid JSON: {e}"),
            },
        };

        let mut resp_json = serde_json::to_string(&response)?;
        resp_json.push('\n');
        writer.write_all(resp_json.as_bytes()).await?;
        writer.flush().await?;
    }

    info!("IPC: client disconnected");
    Ok(())
}

/// Process a single IPC request.
fn process_request(req: IpcRequest, state: &SharedState) -> IpcResponse {
    match req {
        IpcRequest::GetDevices => {
            let devices: Vec<DeviceDto> = state
                .snapshot_devices()
                .iter()
                .map(DeviceDto::from)
                .collect();
            IpcResponse::Ok {
                data: serde_json::to_value(devices).unwrap_or_default(),
            }
        }

        IpcRequest::SetPolicy { mac, policy } => {
            match parse_mac(&mac) {
                Some(mac_bytes) => {
                    let device_policy = match policy {
                        PolicyPayload::Allow => DevicePolicy::Allow,
                        PolicyPayload::Throttle {
                            download_kbps,
                            upload_kbps,
                        } => DevicePolicy::Throttle {
                            // kbps → bytes/sec:  kbps * 1000 / 8
                            download_bps: download_kbps * 1000 / 8,
                            upload_bps: upload_kbps * 1000 / 8,
                        },
                        PolicyPayload::Block => DevicePolicy::Block,
                    };
                    state.set_policy(&mac_bytes, device_policy);
                    IpcResponse::Ok {
                        data: serde_json::json!({"applied": true}),
                    }
                }
                None => IpcResponse::Error {
                    message: format!("Invalid MAC address: {mac}"),
                },
            }
        }

        IpcRequest::TriggerScan => {
            // The next discovery cycle will pick it up; we just ack.
            IpcResponse::Ok {
                data: serde_json::json!({"scan_triggered": true}),
            }
        }

        IpcRequest::GetStats => {
            let devices = state.snapshot_devices();
            let stats = StatsDto {
                total_download_bytes: devices.iter().map(|d| d.download_bytes).sum(),
                total_upload_bytes: devices.iter().map(|d| d.upload_bytes).sum(),
                device_count: devices.len(),
            };
            IpcResponse::Ok {
                data: serde_json::to_value(stats).unwrap_or_default(),
            }
        }

        IpcRequest::Ping => IpcResponse::Ok {
            data: serde_json::json!({"pong": true}),
        },
    }
}

/// Parse "AA:BB:CC:DD:EE:FF" → [u8; 6].
fn parse_mac(s: &str) -> Option<MacAddr> {
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
