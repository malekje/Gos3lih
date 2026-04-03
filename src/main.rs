//! Gos3lih — Real-time network monitor and per-device bandwidth throttler.
//!
//! Runs as a high-priority Windows Service. Intercepts all traffic via WinDivert,
//! applies token-bucket throttling per device, and exposes an IPC control channel.

mod engine;
mod discovery;
mod ipc;
mod state;
mod throttle;
mod updater;

use anyhow::Result;
use std::sync::Arc;
use tracing::{info, error};

use crate::state::SharedState;
use crate::updater::UpdateState;

// ---------------------------------------------------------------------------
// Windows Service boilerplate
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn main() -> Result<()> {
    // When launched by the SCM, run as a service.
    // When launched from a console (dev mode), run directly.
    if let Err(_) = windows_service_main() {
        run_standalone()?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() -> Result<()> {
    run_standalone()
}

/// Attempt to register with the Windows Service Control Manager.
#[cfg(windows)]
fn windows_service_main() -> Result<()> {
    use windows_service::service_dispatcher;
    service_dispatcher::start("Gos3lih", ffi_service_main)?;
    Ok(())
}

#[cfg(windows)]
windows_service::define_windows_service!(ffi_service_main, service_main);

#[cfg(windows)]
fn service_main(args: Vec<std::ffi::OsString>) {
    if let Err(e) = run_service(args) {
        error!("Service failed: {e:#}");
    }
}

#[cfg(windows)]
fn run_service(_args: Vec<std::ffi::OsString>) -> Result<()> {
    use windows_service::service::*;
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use std::sync::mpsc;
    use std::time::Duration;

    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register("Gos3lih", event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // Build and run the core runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .thread_name("gos3lih-worker")
        .build()?;

    rt.block_on(async {
        let state = Arc::new(SharedState::new());
        let update_state = Arc::new(UpdateState::new());
        let state2 = Arc::clone(&state);
        let state3 = Arc::clone(&state);
        let state4 = Arc::clone(&state);
        let state5 = Arc::clone(&state);
        let us1 = Arc::clone(&update_state);
        let us2 = Arc::clone(&update_state);

        // Spawn subsystems
        let engine_handle = tokio::spawn(engine::run_packet_engine(state2));
        let discovery_handle = tokio::spawn(discovery::run_discovery_loop(state3));
        let ipc_handle = tokio::spawn(ipc::run_ipc_server(state4, us1));
        let updater_handle = tokio::spawn(updater::run_update_checker(state5, us2));

        // Wait for SCM stop signal
        let _ = shutdown_rx.recv();
        info!("Received stop signal, shutting down\u{2026}");

        state.request_shutdown();
        let _ = tokio::join!(engine_handle, discovery_handle, ipc_handle, updater_handle);
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

/// Standalone console mode — for development and testing.
fn run_standalone() -> Result<()> {
    // Initialise structured logging — disable ANSI colors so Windows CMD
    // doesn't show raw escape sequences like ←[32m.
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gos3lih=debug,info".into()),
        )
        .with_target(true)
        .init();

    info!("Gos3lih starting in standalone (console) mode");

    // Raise process priority
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, SetPriorityClass, HIGH_PRIORITY_CLASS,
        };
        unsafe {
            SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
        }
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .thread_name("gos3lih-worker")
        .build()?;

    rt.block_on(async {
        let state = Arc::new(SharedState::new());
        let update_state = Arc::new(UpdateState::new());

        let s1 = Arc::clone(&state);
        let s2 = Arc::clone(&state);
        let s3 = Arc::clone(&state);
        let s4 = Arc::clone(&state);
        let us1 = Arc::clone(&update_state);
        let us2 = Arc::clone(&update_state);

        // Spawn the four core subsystems
        let engine_handle = tokio::spawn(engine::run_packet_engine(s1));
        let discovery_handle = tokio::spawn(discovery::run_discovery_loop(s2));
        let ipc_handle = tokio::spawn(ipc::run_ipc_server(s3, us1));
        let updater_handle = tokio::spawn(updater::run_update_checker(s4, us2));

        info!("All subsystems started. Press Ctrl+C to stop.");

        // Graceful shutdown on Ctrl+C
        tokio::signal::ctrl_c().await.ok();
        info!("Ctrl+C received, shutting down…");
        state.request_shutdown();

        let _ = tokio::join!(engine_handle, discovery_handle, ipc_handle, updater_handle);
        info!("Gos3lih stopped.");
    });

    Ok(())
}
