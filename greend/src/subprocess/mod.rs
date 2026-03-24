//! the child process for data collection.
//! this module is a wrapper around the system-specific implementation

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::watch;

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
    let session_id = session.await?;

    #[cfg(target_os = "macos")]
    macos::spawn_powermetrics(tx, session_id, shutdown, interval_ms).await?;

    #[cfg(target_os = "linux")]
    panic!("linux not supported yet");

    #[cfg(target_os = "windows")]
    panic!("windows not supported yet");

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    panic!("unsupported os :(");

    Ok(())
}
