//! the child process for data collection.
//! this module is a wrapper around the system-specific implementation

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::debug;
use tracing::info;

use crate::InternalError;
use crate::schema::EnergySample;

mod linux;
mod macos;
mod windows;

pub async fn metrics_task(
    tx: Sender<EnergySample>,
    session: oneshot::Receiver<i64>,
    shutdown: watch::Receiver<bool>,
    interval_ms: watch::Receiver<u64>,
) -> Result<(), InternalError> {
    debug!("metrics task: waiting for session id");
    let session_id = session.await?;
    info!(session_id, "metrics task: received session id");

    #[cfg(target_os = "macos")]
    {
        info!("metrics task: spawning powermetrics on macOS");
        macos::spawn_powermetrics(tx, session_id, shutdown, interval_ms).await?;
    }

    #[cfg(target_os = "linux")]
    {
        warn!("metrics task: linux not yet supported");
        panic!("linux not supported yet");
    }

    #[cfg(target_os = "windows")]
    {
        warn!("metrics task: windows not yet supported");
        panic!("windows not supported yet");
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        warn!("metrics task: unsupported operating system");
        panic!("unsupported os :(");
    }

    info!("metrics task: exiting");
    Ok(())
}
