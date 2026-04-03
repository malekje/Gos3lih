import { useState } from "react";
import { Download, RefreshCw, X } from "lucide-react";
import type { UpdateInfo } from "../types";
import { applyUpdate } from "../ipc";

interface UpdateBannerProps {
  update: UpdateInfo;
  onDismiss: () => void;
}

export function UpdateBanner({ update, onDismiss }: UpdateBannerProps) {
  const [applying, setApplying] = useState(false);

  if (!update.available) return null;

  const handleApply = async () => {
    setApplying(true);
    try {
      await applyUpdate();
      // The backend will exit and restart — the UI will reconnect.
    } catch {
      setApplying(false);
    }
  };

  return (
    <div className="bg-gradient-to-r from-brand-900/80 to-brand-800/80 border-b border-brand-700">
      <div className="max-w-7xl mx-auto px-6 py-3 flex items-center justify-between gap-4">
        <div className="flex items-center gap-3 min-w-0">
          <Download className="w-5 h-5 text-brand-300 shrink-0" />
          <div className="min-w-0">
            <span className="text-sm font-medium text-white">
              Update available:{" "}
              <span className="text-brand-300">
                v{update.current_version} → v{update.latest_version}
              </span>
            </span>
            {update.release_notes && (
              <p className="text-xs text-brand-200/70 truncate mt-0.5">
                {update.release_notes.split("\n")[0]}
              </p>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <button
            onClick={handleApply}
            disabled={applying}
            className="flex items-center gap-2 px-4 py-1.5 text-sm font-medium bg-brand-500 hover:bg-brand-600 disabled:opacity-50 rounded-lg transition-colors text-white"
          >
            {applying ? (
              <>
                <RefreshCw className="w-4 h-4 animate-spin" />
                Updating…
              </>
            ) : (
              <>
                <RefreshCw className="w-4 h-4" />
                Restart &amp; Update
              </>
            )}
          </button>

          <button
            onClick={onDismiss}
            className="p-1.5 text-brand-300 hover:text-white rounded-lg transition-colors"
            title="Dismiss"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
