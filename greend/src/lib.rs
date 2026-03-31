//! greend daemon for greenb.
//! periodically samples system wide energy usage,
//! writes it to the greenb database for later processing

use std::path::PathBuf;

use chrono::Local;
use tokio::sync::oneshot;
use tracing_subscriber::fmt::time::FormatTime;

pub mod db;
pub mod idle;
pub mod schema;
pub mod subprocess;

/// how many samples to buffer before flushing to the database file
#[cfg(not(debug_assertions))]
pub const SAMPLE_BUFFER_SIZE: usize = 2000;
#[cfg(debug_assertions)]
pub const SAMPLE_BUFFER_SIZE: usize = 100;

#[derive(Debug, thiserror::Error)]
pub enum InternalError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    PlistParseError(#[from] plist::Error),
    #[error(transparent)]
    DbError(#[from] rusqlite::Error),
    #[error("database failed to send session_id, sender dropped before sending")]
    OneshotError(#[from] oneshot::error::RecvError),
    #[error("task exited for no reason")]
    TaskError,
    #[error(transparent)]
    TaskJoinError(#[from] tokio::task::JoinError),
}

pub fn get_database_path() -> std::io::Result<PathBuf> {
    let system = std::env::consts::OS;

    let db_dir = match system {
        "linux" => PathBuf::from("/var/lib/greenb"),
        "macos" => {
            let home = std::env::var("HOME").expect("HOME not set");
            PathBuf::from(home).join("Library/Application Support/greenb")
        }
        "windows" => PathBuf::from("C:/ProgramData/GreenB"),
        _ => {
            let home = std::env::var("HOME").expect("HOME not set");
            PathBuf::from(home).join(".greenb")
        }
    };

    // create directory if it doesn't exist
    std::fs::create_dir_all(&db_dir)?;

    Ok(db_dir.join("energy_monitor.db"))
}

pub struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer) -> std::fmt::Result {
        write!(w, "{}", Local::now().format("%H:%M:%S%.3f"))
    }
}

pub async fn wait_for_signal(
    sigterm: &mut tokio::signal::unix::Signal,
    sigint: &mut tokio::signal::unix::Signal,
) {
    tokio::select! {
        _ = sigterm.recv() => {},
        _ = sigint.recv() => {},
    }
}
