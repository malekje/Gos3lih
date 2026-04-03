import { useState } from "react";
import {
  Shield,
  ShieldOff,
  Gauge,
  RefreshCw,
  Monitor,
  Smartphone,
  Tv,
  Cpu,
  Laptop,
} from "lucide-react";
import type { Device, PolicyPayload } from "../types";
import { setPolicy, triggerScan } from "../ipc";

interface DeviceListProps {
  devices: Device[];
  onRefresh: () => void;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1_073_741_824) return (bytes / 1_073_741_824).toFixed(1) + " GB";
  if (bytes >= 1_048_576) return (bytes / 1_048_576).toFixed(1) + " MB";
  if (bytes >= 1_024) return (bytes / 1_024).toFixed(0) + " KB";
  return bytes + " B";
}

function deviceIcon(hostname: string) {
  const lower = hostname.toLowerCase();
  if (lower.includes("iphone") || lower.includes("android") || lower.includes("pixel"))
    return <Smartphone className="w-5 h-5" />;
  if (lower.includes("tv") || lower.includes("roku") || lower.includes("chromecast"))
    return <Tv className="w-5 h-5" />;
  if (lower.includes("laptop") || lower.includes("macbook"))
    return <Laptop className="w-5 h-5" />;
  if (lower.includes("iot") || lower.includes("esp") || lower.includes("arduino"))
    return <Cpu className="w-5 h-5" />;
  return <Monitor className="w-5 h-5" />;
}

function PolicyBadge({ policy }: { policy: string }) {
  switch (policy) {
    case "block":
      return (
        <span className="px-2 py-0.5 text-xs font-medium rounded-full bg-red-900/50 text-red-300 border border-red-800">
          Blocked
        </span>
      );
    case "throttle":
      return (
        <span className="px-2 py-0.5 text-xs font-medium rounded-full bg-amber-900/50 text-amber-300 border border-amber-800">
          Throttled
        </span>
      );
    default:
      return (
        <span className="px-2 py-0.5 text-xs font-medium rounded-full bg-emerald-900/50 text-emerald-300 border border-emerald-800">
          Allowed
        </span>
      );
  }
}

function DeviceRow({ device }: { device: Device }) {
  const [showThrottle, setShowThrottle] = useState(false);
  const [dlLimit, setDlLimit] = useState(device.download_limit_kbps ?? 5000);
  const [ulLimit, setUlLimit] = useState(device.upload_limit_kbps ?? 2000);
  const [loading, setLoading] = useState(false);

  const applyPolicy = async (p: PolicyPayload) => {
    setLoading(true);
    try {
      await setPolicy(device.mac, p);
    } finally {
      setLoading(false);
    }
  };

  const isBlocked = device.policy === "block";
  const isThrottled = device.policy === "throttle";

  return (
    <div className="bg-gray-900 border border-gray-800 rounded-xl p-4 hover:border-gray-700 transition-colors">
      <div className="flex items-center gap-4">
        {/* Icon */}
        <div className="text-gray-400">{deviceIcon(device.hostname)}</div>

        {/* Info */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-medium truncate">
              {device.hostname || "Unknown Device"}
            </span>
            <PolicyBadge policy={device.policy} />
          </div>
          <div className="text-xs text-gray-500 mt-0.5 space-x-3">
            <span>{device.ip}</span>
            <span>{device.mac}</span>
            {device.vendor && <span>{device.vendor}</span>}
          </div>
        </div>

        {/* Traffic stats */}
        <div className="text-right text-xs text-gray-400 hidden sm:block">
          <div>
            <span className="text-brand-400">↓</span> {formatBytes(device.download_bytes)}
          </div>
          <div>
            <span className="text-emerald-400">↑</span> {formatBytes(device.upload_bytes)}
          </div>
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2 ml-4">
          {/* Block toggle */}
          <button
            onClick={() =>
              applyPolicy(isBlocked ? { type: "allow" } : { type: "block" })
            }
            disabled={loading}
            className={`p-2 rounded-lg transition-colors ${
              isBlocked
                ? "bg-red-600 text-white hover:bg-red-700"
                : "bg-gray-800 text-gray-400 hover:bg-red-900/50 hover:text-red-300"
            }`}
            title={isBlocked ? "Unblock" : "Block"}
          >
            {isBlocked ? (
              <ShieldOff className="w-4 h-4" />
            ) : (
              <Shield className="w-4 h-4" />
            )}
          </button>

          {/* Throttle toggle */}
          <button
            onClick={() => {
              if (isThrottled) {
                applyPolicy({ type: "allow" });
                setShowThrottle(false);
              } else {
                setShowThrottle(!showThrottle);
              }
            }}
            disabled={loading}
            className={`p-2 rounded-lg transition-colors ${
              isThrottled
                ? "bg-amber-600 text-white hover:bg-amber-700"
                : "bg-gray-800 text-gray-400 hover:bg-amber-900/50 hover:text-amber-300"
            }`}
            title={isThrottled ? "Remove limit" : "Limit speed"}
          >
            <Gauge className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Throttle slider panel */}
      {(showThrottle || isThrottled) && !isBlocked && (
        <div className="mt-4 pt-4 border-t border-gray-800 space-y-3">
          <div>
            <div className="flex justify-between text-xs text-gray-400 mb-1">
              <span>Download limit</span>
              <span className="font-mono text-brand-400">
                {dlLimit >= 1000
                  ? (dlLimit / 1000).toFixed(1) + " Mbps"
                  : dlLimit + " Kbps"}
              </span>
            </div>
            <input
              type="range"
              min={100}
              max={100000}
              step={100}
              value={dlLimit}
              onChange={(e) => setDlLimit(Number(e.target.value))}
              className="w-full h-1.5 bg-gray-700 rounded-lg appearance-none cursor-pointer accent-brand-500"
            />
          </div>

          <div>
            <div className="flex justify-between text-xs text-gray-400 mb-1">
              <span>Upload limit</span>
              <span className="font-mono text-emerald-400">
                {ulLimit >= 1000
                  ? (ulLimit / 1000).toFixed(1) + " Mbps"
                  : ulLimit + " Kbps"}
              </span>
            </div>
            <input
              type="range"
              min={100}
              max={100000}
              step={100}
              value={ulLimit}
              onChange={(e) => setUlLimit(Number(e.target.value))}
              className="w-full h-1.5 bg-gray-700 rounded-lg appearance-none cursor-pointer accent-emerald-500"
            />
          </div>

          <button
            onClick={() => {
              applyPolicy({
                type: "throttle",
                download_kbps: dlLimit,
                upload_kbps: ulLimit,
              });
              setShowThrottle(false);
            }}
            disabled={loading}
            className="px-4 py-1.5 text-sm font-medium bg-brand-600 hover:bg-brand-700 rounded-lg transition-colors"
          >
            Apply Limit
          </button>
        </div>
      )}
    </div>
  );
}

export function DeviceList({ devices, onRefresh }: DeviceListProps) {
  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-semibold">Connected Devices</h2>
        <button
          onClick={async () => {
            await triggerScan();
            onRefresh();
          }}
          className="flex items-center gap-2 px-3 py-1.5 text-sm bg-gray-800 hover:bg-gray-700 rounded-lg transition-colors"
        >
          <RefreshCw className="w-4 h-4" />
          Scan Network
        </button>
      </div>

      <div className="space-y-3">
        {devices.length === 0 ? (
          <div className="text-center py-12 text-gray-500">
            No devices discovered yet. Click "Scan Network" to start.
          </div>
        ) : (
          devices.map((d) => <DeviceRow key={d.mac} device={d} />)
        )}
      </div>
    </div>
  );
}
