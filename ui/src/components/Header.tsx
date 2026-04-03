import { Activity, Wifi, WifiOff } from "lucide-react";

interface HeaderProps {
  connected: boolean;
  deviceCount: number;
}

export function Header({ connected, deviceCount }: HeaderProps) {
  return (
    <header className="border-b border-gray-800 bg-gray-900/80 backdrop-blur-sm sticky top-0 z-50">
      <div className="max-w-7xl mx-auto px-6 py-4 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Activity className="w-7 h-7 text-brand-500" />
          <h1 className="text-xl font-bold tracking-tight">
            Gos<span className="text-brand-400">3lih</span>
          </h1>
        </div>

        <div className="flex items-center gap-6 text-sm">
          <span className="text-gray-400">
            {deviceCount} device{deviceCount !== 1 ? "s" : ""} online
          </span>

          <div className="flex items-center gap-2">
            {connected ? (
              <>
                <Wifi className="w-4 h-4 text-emerald-400" />
                <span className="text-emerald-400">Engine connected</span>
              </>
            ) : (
              <>
                <WifiOff className="w-4 h-4 text-red-400" />
                <span className="text-red-400">Engine offline</span>
              </>
            )}
          </div>
        </div>
      </div>
    </header>
  );
}
