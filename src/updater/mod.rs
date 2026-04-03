//! Auto-Updater — checks GitHub Releases for new versions.
//!
//! On every poll cycle (default: 5 minutes), fetches the latest release tag from
//! the GitHub API, compares it against the compiled-in version, and exposes
//! the result to the IPC layer so the UI can show an update banner.
//!
//! When the user accepts, the updater downloads the new `.exe`, writes it next
//! to the running binary as `<name>.update.exe`, then spawns a tiny batch
//! script that waits for this process to exit, replaces the old exe, and
//! re-launches.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::state::SharedState;

// ---------------------------------------------------------------------------
// Configuration — change these to match your GitHub repository
// ---------------------------------------------------------------------------

/// GitHub owner/repo (e.g. "myuser/gos3lih").
/// The CI workflow publishes releases here.
const GITHUB_REPO: &str = "malekje/Gos3lih";

/// How often to poll for updates.
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Compiled-in version from Cargo.toml `package.version`.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    pub release_notes: String,
}

impl UpdateInfo {
    fn none() -> Self {
        Self {
            available: false,
            current_version: CURRENT_VERSION.to_string(),
            latest_version: CURRENT_VERSION.to_string(),
            download_url: String::new(),
            release_notes: String::new(),
        }
    }
}

/// Shared update state readable by the IPC layer.
pub struct UpdateState {
    pub info: RwLock<UpdateInfo>,
}

impl UpdateState {
    pub fn new() -> Self {
        Self {
            info: RwLock::new(UpdateInfo::none()),
        }
    }
}

// ---------------------------------------------------------------------------
// GitHub API types (only what we need)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

// ---------------------------------------------------------------------------
// Updater loop
// ---------------------------------------------------------------------------

pub async fn run_update_checker(
    state: Arc<SharedState>,
    update_state: Arc<UpdateState>,
) -> Result<()> {
    info!(
        "Update checker starting (current version: {CURRENT_VERSION}, repo: {GITHUB_REPO})"
    );

    let client = reqwest::Client::builder()
        .user_agent(format!("Gos3lih/{CURRENT_VERSION}"))
        .timeout(Duration::from_secs(30))
        .build()?;

    loop {
        if state.is_shutdown() {
            break;
        }

        match check_for_update(&client).await {
            Ok(Some(info)) => {
                info!(
                    "Update available: {} → {}",
                    info.current_version, info.latest_version
                );
                *update_state.info.write() = info;
            }
            Ok(None) => {
                // Already up-to-date — clear any stale update info.
                *update_state.info.write() = UpdateInfo::none();
            }
            Err(e) => {
                warn!("Update check failed: {e:#}");
            }
        }

        // Wait for next cycle, checking shutdown frequently.
        for _ in 0..(CHECK_INTERVAL.as_millis() / 500) {
            if state.is_shutdown() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    info!("Update checker stopped");
    Ok(())
}

/// Query GitHub Releases API and compare versions.
async fn check_for_update(client: &reqwest::Client) -> Result<Option<UpdateInfo>> {
    let url = format!(
        "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
    );

    let resp = client.get(&url).send().await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // No releases yet.
        return Ok(None);
    }

    let release: GitHubRelease = resp
        .error_for_status()?
        .json()
        .await
        .context("Failed to parse GitHub release JSON")?;

    let latest_tag = release.tag_name.trim_start_matches('v').to_string();

    if version_is_newer(&latest_tag, CURRENT_VERSION) {
        // Find the zip bundle asset.
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == "Gos3lih.zip");

        let download_url = asset
            .map(|a| a.browser_download_url.clone())
            .unwrap_or_default();

        Ok(Some(UpdateInfo {
            available: true,
            current_version: CURRENT_VERSION.to_string(),
            latest_version: latest_tag,
            download_url,
            release_notes: release.body.unwrap_or_default(),
        }))
    } else {
        Ok(None)
    }
}

/// Simple semver comparison: "0.2.0" > "0.1.0".
/// Strips any pre-release/build suffix (e.g. "0.1.0-build.4" → "0.1.0").
fn version_is_newer(latest: &str, current: &str) -> bool {
    fn clean(v: &str) -> &str {
        v.split('-').next().unwrap_or(v)
    }
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .filter_map(|s| s.parse::<u64>().ok())
            .collect()
    };
    let l = parse(clean(latest));
    let c = parse(clean(current));
    l > c
}

// ---------------------------------------------------------------------------
// Download & self-replace
// ---------------------------------------------------------------------------

/// Download the zip bundle and perform a self-replace + restart.
pub async fn apply_update(download_url: &str) -> Result<()> {
    if download_url.is_empty() {
        anyhow::bail!("No download URL available");
    }

    let client = reqwest::Client::builder()
        .user_agent(format!("Gos3lih/{CURRENT_VERSION}"))
        .timeout(Duration::from_secs(300))
        .build()?;

    info!("Downloading update from {download_url}");
    let bytes = client
        .get(download_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let current_exe = std::env::current_exe()?;
    let parent = current_exe.parent().unwrap_or_else(|| std::path::Path::new("."));
    let update_exe = parent.join("gos3lih-service.update.exe");
    let backup_exe = parent.join("gos3lih-service.old.exe");

    // Extract the zip — update exe + DLLs side by side.
    let cursor = std::io::Cursor::new(&bytes[..]);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to open update zip")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let dest = if name.contains("gos3lih-service") {
            update_exe.clone()
        } else {
            parent.join(std::path::Path::new(&name).file_name().unwrap_or_default())
        };
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut file, &mut out)?;
    }

    info!("Update extracted ({} bytes), scheduling restart…", bytes.len());

    let bat_path = parent.join("_gos3lih_update.bat");
    let bat_content = format!(
        r#"@echo off
:wait
tasklist /FI "PID eq {pid}" 2>NUL | find "{pid}" >NUL
if not errorlevel 1 (
    timeout /t 1 /nobreak >NUL
    goto wait
)
if exist "{backup}" del /f "{backup}"
move /y "{current}" "{backup}"
move /y "{update}" "{current}"
start "" "{current}"
del /f "{backup}" 2>NUL
del /f "%~f0"
"#,
        pid = std::process::id(),
        current = current_exe.display(),
        update = update_exe.display(),
        backup = backup_exe.display(),
    );

    tokio::fs::write(&bat_path, bat_content).await?;

    std::process::Command::new("cmd.exe")
        .args(["/C", "start", "/min", "", &bat_path.to_string_lossy()])
        .spawn()
        .context("Failed to launch update script")?;

    info!("Update script launched — exiting for restart");
    std::process::exit(0);
}
