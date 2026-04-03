<p align="center">
  <img src="https://img.shields.io/badge/Platform-Windows%2010%2F11-blue?style=for-the-badge&logo=windows" />
  <img src="https://img.shields.io/badge/Language-Rust-orange?style=for-the-badge&logo=rust" />
  <img src="https://img.shields.io/badge/UI-Tauri%20%2B%20React-purple?style=for-the-badge&logo=tauri" />
  <img src="https://img.shields.io/github/v/release/malekje/Gos3lih?style=for-the-badge&label=Latest&color=green" />
</p>

# Gos3lih

**Real-time network monitor and per-device bandwidth throttler for Windows.**

Gos3lih intercepts all network traffic on your machine using WinDivert, discovers every device on your LAN via ARP scanning, and lets you **block** or **throttle** (limit download/upload speed) any device — all from a clean dashboard UI.

---

## Download

> **[⬇ Download Latest Release](https://github.com/malekje/Gos3lih/releases/latest)**

| File | Purpose |
|------|---------|
| `Gos3lih.zip` | Download this — contains everything you need |
| `gos3lih-service.exe` | Backend engine — run this as Administrator |
| `gos3lih-ui.exe` | Dashboard UI — auto-launched by the service |
| `WinDivert.dll` | Packet driver (must be in same folder) |
| `WinDivert64.sys` | Kernel driver (must be in same folder) |

### Quick Start

1. **Download** [`Gos3lih.zip`](https://github.com/malekje/Gos3lih/releases/latest) from the latest release
2. **Extract** all files into the **same folder** (e.g. `C:\Gos3lih\`)
3. Install **[Npcap](https://npcap.com/#download)** — check **"WinPcap API-compatible Mode"** during install
4. **Right-click** `gos3lih-service.exe` → **Run as Administrator**
5. The **dashboard UI opens automatically** — discover every device on your network and throttle their bandwidth!

> ⚠️ All 4 files must stay in the same folder or the app will fail to start.

> **Important:** Administrator privileges are required for packet interception.

---

## Auto-Update

Gos3lih **updates itself automatically**. When a new version is pushed to `main`:

1. GitHub Actions builds a fresh release binary
2. Your running app detects the new version within 5 minutes
3. A banner appears at the top of the dashboard:
   > *"Update available: v0.1.0 → v0.2.0"*
4. Click **"Restart & Update"** — the app downloads the new `.exe`, replaces itself, and restarts

No manual downloads needed after the first install.

---

## Features

- **Live Network Monitoring** — Real-time download/upload throughput gauges
- **Device Discovery** — ARP scanning + NetBIOS/mDNS to find and name every device on your LAN
- **Per-Device Throttling** — Token Bucket algorithm delays packets (never drops) for TCP stability
- **Per-Device Blocking** — Silently drop all traffic to/from a device
- **Speed Sliders** — Adjustable 100 Kbps to 100 Mbps per device (download & upload independently)
- **Auto-Update** — One-click update when new versions are published
- **Low Resource Usage** — Tauri + WebView2 UI uses ~30 MB RAM (vs ~300 MB for Electron)
- **Windows Service** — Can run as a background service or standalone console app

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Tauri UI (React)                │
│  Throughput Gauges │ Device List │ Block/Limit   │
└──────────────────┬──────────────────────────────┘
                   │  Named Pipe (JSON)
┌──────────────────┴──────────────────────────────┐
│             Gos3lih Service (Rust)               │
│                                                  │
│  ┌──────────┐ ┌───────────┐ ┌────────────────┐  │
│  │  Packet   │ │ Discovery │ │  Auto-Updater  │  │
│  │  Engine   │ │  (ARP +   │ │  (GitHub API)  │  │
│  │ (4 threads│ │  NetBIOS) │ │                │  │
│  │  WinDivert│ │           │ │                │  │
│  └─────┬─────┘ └───────────┘ └────────────────┘  │
│        │                                         │
│  ┌─────┴─────┐ ┌────────────┐                    │
│  │Token Bucket│ │   Shared   │                    │
│  │  Filter    │ │   State    │                    │
│  │(delay pkts)│ │ (DashMap)  │                    │
│  └───────────┘ └────────────┘                    │
└──────────────────────────────────────────────────┘
                   │
            ┌──────┴──────┐
            │ WinDivert.sys│  (kernel driver)
            └─────────────┘
```

---

## Building from Source

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 18+
- [WinDivert 2.2.2](https://reqrypt.org/windivert.html) (download and extract)
- [Npcap SDK](https://npcap.com/) (for raw socket ARP scanning)
- Visual Studio Build Tools (C++ workload)

### Build

```powershell
# Set paths to vendor libraries
$env:WINDIVERT_PATH = ".\vendor\WinDivert-2.2.2-A"
$env:LIB = ".\vendor\npcap-sdk\Lib\x64;.\vendor\WinDivert-2.2.2-A\x64;$env:LIB"

# Build backend
cargo build --release

# Build frontend
cd ui
npm install
npm run tauri build
```

---

## Configuration

Edit `GITHUB_REPO` in `src/updater/mod.rs` to point to your actual GitHub repository for auto-updates to work:

```rust
const GITHUB_REPO: &str = "malekje/Gos3lih";
```

---

## License

MIT
