import { useState, useEffect, useCallback } from "react";
import { ThroughputGauge } from "./components/ThroughputGauge";
import { DeviceList } from "./components/DeviceList";
import { Header } from "./components/Header";
import { UpdateBanner } from "./components/UpdateBanner";
import { getDevices, getStats, checkUpdate } from "./ipc";
import type { Device, Stats, UpdateInfo } from "./types";

const POLL_INTERVAL = 2000; // 2 seconds

function App() {
  const [devices, setDevices] = useState<Device[]>([]);
  const [stats, setStats] = useState<Stats>({
    total_download_bytes: 0,
    total_upload_bytes: 0,
    device_count: 0,
  });
  const [connected, setConnected] = useState(true);
  const [prevStats, setPrevStats] = useState<Stats | null>(null);
  const [throughput, setThroughput] = useState({ download: 0, upload: 0 });
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateDismissed, setUpdateDismissed] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [devs, st] = await Promise.all([getDevices(), getStats()]);
      setDevices(devs);

      // Calculate throughput delta
      if (prevStats) {
        const dlDelta = st.total_download_bytes - prevStats.total_download_bytes;
        const ulDelta = st.total_upload_bytes - prevStats.total_upload_bytes;
        setThroughput({
          download: Math.max(0, (dlDelta / (POLL_INTERVAL / 1000)) * 8), // bits/s
          upload: Math.max(0, (ulDelta / (POLL_INTERVAL / 1000)) * 8),
        });
      }
      setPrevStats(st);
      setStats(st);
      setConnected(true);
    } catch {
      setConnected(false);
    }
  }, [prevStats]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, POLL_INTERVAL);
    return () => clearInterval(id);
  }, [refresh]);

  // Check for updates every 60 seconds
  useEffect(() => {
    const check = async () => {
      try {
        const info = await checkUpdate();
        if (info.available) {
          setUpdateInfo(info);
          setUpdateDismissed(false); // show banner again on new version
        }
      } catch {
        // silently ignore update check failures
      }
    };
    check();
    const id = setInterval(check, 60_000);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="min-h-screen bg-gray-950 flex flex-col">
      {/* Update banner — shown when a new version is detected */}
      {updateInfo?.available && !updateDismissed && (
        <UpdateBanner
          update={updateInfo}
          onDismiss={() => setUpdateDismissed(true)}
        />
      )}

      <Header connected={connected} deviceCount={stats.device_count} />

      <main className="flex-1 p-6 space-y-6 max-w-7xl mx-auto w-full">
        {/* Throughput gauges */}
        <section className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <ThroughputGauge
            label="Download"
            bitsPerSec={throughput.download}
            color="#33a5ff"
          />
          <ThroughputGauge
            label="Upload"
            bitsPerSec={throughput.upload}
            color="#10b981"
          />
        </section>

        {/* Device list */}
        <section>
          <DeviceList devices={devices} onRefresh={refresh} />
        </section>
      </main>
    </div>
  );
}

export default App;
