// ─── IPC Client ──────────────────────────────────────────────────────────────
// Communicates with the Rust backend over a Named Pipe via Tauri commands.
// In development mode, falls back to a mock data provider.

import type { Device, Stats, PolicyPayload, UpdateInfo } from "./types";

const IS_TAURI = "__TAURI__" in window;

// ─── Tauri invoke wrapper ────────────────────────────────────────────────────

async function invoke<T>(method: string, params?: unknown): Promise<T> {
  if (IS_TAURI) {
    const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
    return tauriInvoke<T>("ipc_forward", { method, params });
  }
  // Dev fallback: use mock data
  return mockInvoke<T>(method, params);
}

// ─── Public API ──────────────────────────────────────────────────────────────

export async function getDevices(): Promise<Device[]> {
  return invoke<Device[]>("get_devices");
}

export async function setPolicy(
  mac: string,
  policy: PolicyPayload
): Promise<void> {
  await invoke("set_policy", { mac, policy });
}

export async function getStats(): Promise<Stats> {
  return invoke<Stats>("get_stats");
}

export async function triggerScan(): Promise<void> {
  await invoke("trigger_scan");
}

export async function ping(): Promise<boolean> {
  try {
    await invoke("ping");
    return true;
  } catch {
    return false;
  }
}

export async function checkUpdate(): Promise<UpdateInfo> {
  return invoke<UpdateInfo>("check_update");
}

export async function applyUpdate(): Promise<void> {
  await invoke("apply_update");
}

// ─── Mock data for development without the Rust backend ──────────────────────

const MOCK_DEVICES: Device[] = [
  {
    mac: "AA:BB:CC:DD:EE:01",
    ip: "192.168.1.10",
    hostname: "Gaming-PC",
    vendor: "Intel Corp",
    policy: "allow",
    download_limit_kbps: null,
    upload_limit_kbps: null,
    download_bytes: 1_073_741_824,
    upload_bytes: 268_435_456,
  },
  {
    mac: "AA:BB:CC:DD:EE:02",
    ip: "192.168.1.11",
    hostname: "iPhone-13",
    vendor: "Apple Inc",
    policy: "throttle",
    download_limit_kbps: 5000,
    upload_limit_kbps: 2000,
    download_bytes: 524_288_000,
    upload_bytes: 104_857_600,
  },
  {
    mac: "AA:BB:CC:DD:EE:03",
    ip: "192.168.1.12",
    hostname: "Smart-TV",
    vendor: "Samsung",
    policy: "allow",
    download_limit_kbps: null,
    upload_limit_kbps: null,
    download_bytes: 2_147_483_648,
    upload_bytes: 52_428_800,
  },
  {
    mac: "AA:BB:CC:DD:EE:04",
    ip: "192.168.1.13",
    hostname: "Unknown-IoT",
    vendor: "Espressif",
    policy: "block",
    download_limit_kbps: null,
    upload_limit_kbps: null,
    download_bytes: 0,
    upload_bytes: 0,
  },
  {
    mac: "AA:BB:CC:DD:EE:05",
    ip: "192.168.1.14",
    hostname: "Work-Laptop",
    vendor: "Dell Inc",
    policy: "allow",
    download_limit_kbps: null,
    upload_limit_kbps: null,
    download_bytes: 858_993_459,
    upload_bytes: 429_496_729,
  },
];

async function mockInvoke<T>(method: string, _params?: unknown): Promise<T> {
  // Simulate network latency
  await new Promise((r) => setTimeout(r, 100));

  switch (method) {
    case "get_devices":
      // Add small random changes to simulate live data
      return MOCK_DEVICES.map((d) => ({
        ...d,
        download_bytes: d.download_bytes + Math.floor(Math.random() * 1_000_000),
        upload_bytes: d.upload_bytes + Math.floor(Math.random() * 500_000),
      })) as T;

    case "get_stats":
      return {
        total_download_bytes: MOCK_DEVICES.reduce(
          (a, d) => a + d.download_bytes,
          0
        ),
        total_upload_bytes: MOCK_DEVICES.reduce(
          (a, d) => a + d.upload_bytes,
          0
        ),
        device_count: MOCK_DEVICES.length,
      } as T;

    case "ping":
      return { pong: true } as T;

    case "check_update":
      return {
        available: false,
        current_version: "0.1.0",
        latest_version: "0.1.0",
        download_url: "",
        release_notes: "",
      } as T;

    case "apply_update":
      return {} as T;

    default:
      return {} as T;
  }
}
