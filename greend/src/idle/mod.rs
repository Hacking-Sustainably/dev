//! detect when the system is idle and adjust sampling frequency

use tokio::sync::watch;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::InternalError;

mod macos;

pub async fn idle_task(
    sample_rate: watch::Sender<u64>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), InternalError> {
    info!("idle task: starting idle detection");
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(20));
    debug!("idle task: check interval set to 20 seconds");
    shutdown.mark_unchanged();
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                info!("idle task: shutdown signal received");
                break;
            }
            _ = interval.tick() => {
                let idle_secs = seconds_since_last_input().await.unwrap_or(0.0);
                let new_rate = if idle_secs > 900.0 {
                    30 * 60 * 1000 // more than 15 minutes idle, might as well be sleeping
                } else if idle_secs > 300.0 {
                    60_000 // system very idle
                } else if idle_secs > 60.0 {
                    30_000 // mildly idle
                } else if idle_secs > 30.0 {
                    15_000 // somewhat idle
                } else {
                    5_000 // active (default)
                };
                if let Err(e) = sample_rate.send(new_rate) {
                    warn!("idle task: failed to send sample rate update: {e}");
                    break;
                }
                debug!(idle_secs, new_rate_ms = new_rate, "idle task: adjusted sampling rate");
            }
        }
    }

    info!("idle task: exiting");
    Ok(())
}

pub async fn seconds_since_last_input() -> Option<f64> {
    #[cfg(target_os = "macos")]
    return macos::seconds_since_last_input().await;
    #[cfg(target_os = "linux")]
    unimplemented!();
    #[cfg(target_os = "windows")]
    unimplemented!();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    unimplemented!();
}
