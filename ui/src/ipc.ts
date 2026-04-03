// ─── IPC Client ──────────────────────────────────────────────────────────────
// Calls Tauri commands that run in the same process as the backend engine.
// No more named pipes or mock data — everything is real.

import { invoke } from "@tauri-apps/api/core";
import type { Device, Stats, PolicyPayload, UpdateInfo } from "./types";

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

export async function checkUpdate(): Promise<UpdateInfo> {
  return invoke<UpdateInfo>("check_update");
}

export async function applyUpdate(): Promise<void> {
  await invoke("apply_update");
}
