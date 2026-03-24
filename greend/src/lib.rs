//! greend daemon for greenb.
//! periodically samples system wide energy usage,
//! writes it to the greenb database for later processing

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;

use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

pub mod db;
pub mod schema;
pub mod subprocess;

/// how often to sample the system usage in milliseconds
pub static SAMPLING_RATE: AtomicUsize = AtomicUsize::new(5000);
/// how many samples to buffer before flushing to the database file
pub const BUFFER_SIZE: usize = 10;
/// global shutdown flag
pub static SHUTDOWN: AtomicBool = AtomicBool::new(false);

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
}
