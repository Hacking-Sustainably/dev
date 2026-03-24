//! macos idle detection
use std::process::Stdio;

use tokio::process::Command;

/// Returns seconds since last user input using IOKit via ioreg
pub async fn seconds_since_last_input() -> Option<f64> {
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
            return Some(ns as f64 / 1_000_000_000.0);
        }
    }
    None
}
