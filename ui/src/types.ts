// ─── Types ───────────────────────────────────────────────────────────────────
// Mirrors the Rust Tauri command DTOs.

export interface Device {
  mac: string;
  ip: string;
  hostname: string;
  vendor: string;
  policy: "allow" | "throttle" | "block";
  download_limit_kbps: number | null;
  upload_limit_kbps: number | null;
  download_bytes: number;
  upload_bytes: number;
}

export interface Stats {
  total_download_bytes: number;
  total_upload_bytes: number;
  device_count: number;
}

export type PolicyPayload =
  | { type: "allow" }
  | { type: "throttle"; download_kbps: number; upload_kbps: number }
  | { type: "block" };

export interface UpdateInfo {
  available: boolean;
  current_version: string;
  latest_version: string;
  download_url: string;
  release_notes: string;
}

export interface EngineStatus {
  running: boolean;
  error: string;
}
