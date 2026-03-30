//! macos idle detection
use std::process::Stdio;

use tokio::process::Command;
use tracing::debug;
use tracing::error;
use tracing::trace;

/// Returns seconds since last user input using IOKit via ioreg
pub async fn seconds_since_last_input() -> Option<f64> {
    trace!("idle: querying ioreg for HIDIdleTime");
    let out = Command::new("ioreg")
        .args(["-c", "IOHIDSystem", "-d", "4"])
        .stdout(Stdio::piped())
        .output()
        .await
        .ok()?;

    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains("HIDIdleTime") {
            // value is in nanoseconds
            let ns: u64 = line.split('=').nth(1)?.trim().parse().ok()?;
            let seconds = ns as f64 / 1_000_000_000.0;
            debug!("idle: last user input was {:.2} seconds ago", seconds);
            return Some(seconds);
        }
    }

    error!("idle: HIDIdleTime not found in ioreg output");
    None
}
