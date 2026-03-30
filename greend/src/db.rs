//! deutche bahn (sqlite)

use std::path::Path;

use chrono::Utc;
use rusqlite::Connection;
use rusqlite::Statement;
use rusqlite::params;
use tokio::sync::mpsc::Receiver;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

use crate::InternalError;
use crate::SAMPLE_BUFFER_SIZE;
use crate::schema::EnergySample;
use crate::schema::MonitoringSession;
use crate::schema::Timestamp;

/// Creates the `monitoring_sessions` and `energy_samples` tables if they don't
/// already exist.
fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    debug!("db: initializing schema");
    let result = conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS monitoring_sessions (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT    NOT NULL DEFAULT 'Unnamed Session',
            device_name TEXT,
            os_name     TEXT,
            os_version  TEXT,
            started_at  TEXT    NOT NULL,
            ended_at    TEXT,
            created_at  TEXT    NOT NULL
        );

        CREATE TABLE IF NOT EXISTS energy_samples (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id       INTEGER NOT NULL REFERENCES monitoring_sessions(id),
            timestamp        TEXT    NOT NULL,
            app_name         TEXT    NOT NULL,
            pid              INTEGER,
            power_watts      REAL,
            energy_joules    REAL,
            cpu_percent      REAL,
            memory_mb        REAL,
            gpu_percent      REAL,
            disk_read_mb     REAL,
            disk_write_mb    REAL,
            network_sent_mb  REAL,
            network_recv_mb  REAL,
            category         TEXT,
            is_background    INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_energy_samples_session_id
            ON energy_samples(session_id);
        CREATE INDEX IF NOT EXISTS idx_energy_samples_timestamp
            ON energy_samples(timestamp);
        CREATE INDEX IF NOT EXISTS idx_energy_samples_app_name
            ON energy_samples(app_name);
    ",
    );

    match result {
        Ok(()) => {
            debug!("db: schema initialized successfully");
            Ok(())
        }
        Err(e) => {
            error!("db: failed to initialize schema: {e}");
            Err(e)
        }
    }
}

/// Inserts a new [`MonitoringSession`] row and returns the assigned `rowid`.
/// `session.id` is ignored on insert — SQLite assigns it automatically.
fn insert_session(conn: &Connection, session: &MonitoringSession) -> rusqlite::Result<i64> {
    debug!("db: inserting new monitoring session: {}", session.name);
    let started_at = session.started_at.to_rfc3339();
    let created_at = session.created_at.to_rfc3339();
    let ended_at = session.ended_at.as_ref().map(|t| t.to_rfc3339());

    conn.execute(
        "INSERT INTO monitoring_sessions
             (name, device_name, os_name, os_version, started_at, ended_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session.name,
            session.device_name,
            session.os_name,
            session.os_version,
            started_at,
            ended_at,
            created_at,
        ],
    )?;

    let session_id = conn.last_insert_rowid();
    info!(session_id, os = %session.os_name.as_ref().unwrap_or(&"unknown".to_string()), "db: monitoring session created");
    Ok(session_id)
}

/// Updates the `ended_at` column on a session row once monitoring stops.
fn end_session(conn: &Connection, session_id: i64, ended_at: Timestamp) -> rusqlite::Result<()> {
    debug!(session_id, "db: ending monitoring session");
    let result = conn.execute(
        "UPDATE monitoring_sessions SET ended_at = ?1 WHERE id = ?2",
        params![ended_at.to_rfc3339(), session_id],
    );

    match result {
        Ok(_) => {
            debug!(session_id, "db: session ended successfully");
            Ok(())
        }
        Err(e) => {
            error!(session_id, error = %e, "db: failed to end session");
            Err(e)
        }
    }
}

pub async fn writer_task(
    mut rx: Receiver<EnergySample>,
    db_path: &Path,
    session_tx: oneshot::Sender<i64>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), InternalError> {
    info!("db: writer task started");
    debug!("db: opening database at {}", db_path.display());
    let conn = Connection::open(db_path)?;
    info!("db: database connection established");

    debug!("db: configuring pragmas");
    conn.execute_batch(
        "
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        PRAGMA cache_size=-8000;
        PRAGMA temp_store=MEMORY;
    ",
    )?;
    debug!("db: pragmas configured");

    init_schema(&conn)?;

    let session = MonitoringSession {
        id: None,
        name: "greend session".to_string(),
        device_name: None,
        os_name: Some(std::env::consts::OS.to_string()),
        os_version: None,
        started_at: Utc::now(),
        ended_at: None,
        created_at: Utc::now(),
    };

    let session_id = insert_session(&conn, &session)?;

    if session_tx.send(session_id).is_err() {
        error!(session_id, "db: failed to send session_id to metrics task");
    }

    let mut buffer: Vec<EnergySample> = Vec::with_capacity(SAMPLE_BUFFER_SIZE);
    let mut total_samples = 0u64;

    debug!(session_id, "db: preparing insert statement");
    let mut stmt = conn.prepare("INSERT INTO energy_samples (session_id, timestamp, app_name, pid, power_watts, energy_joules, cpu_percent, memory_mb, gpu_percent, disk_read_mb, disk_write_mb, network_sent_mb, network_recv_mb, category, is_background) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")?;
    debug!(session_id, "db: insert statement prepared");

    let flush = |conn: &Connection,
                 stmt: &mut Statement,
                 session_id: i64,
                 buffer: &mut Vec<EnergySample>|
     -> rusqlite::Result<()> {
        let sample_count = buffer.len();
        if sample_count == 0 {
            return Ok(());
        }

        trace!(session_id, sample_count, "db: starting flush transaction");
        let tx = conn.unchecked_transaction()?;
        for s in buffer.drain(..) {
            stmt.execute(params![
                session_id,
                s.timestamp.to_rfc3339(),
                s.app_name,
                s.pid,
                s.power_watts,
                s.energy_joules,
                s.cpu_percent,
                s.memory_mb,
                s.gpu_percent,
                s.disk_read_mb,
                s.disk_write_mb,
                s.network_sent_mb,
                s.network_recv_mb,
                s.category,
                s.is_background as i32,
            ])?;
        }
        tx.commit()?;
        debug!(session_id, sample_count, "db: flush transaction committed");
        Ok(())
    };

    debug!(
        session_id,
        buffer_capacity = SAMPLE_BUFFER_SIZE,
        "db: buffer initialized, entering receive loop"
    );
    shutdown.mark_unchanged();
    loop {
        tokio::select! {
            x = rx.recv() => {
                if let Some(sample) = x {
                    buffer.push(sample);
                    total_samples += 1;

                    if buffer.len() >= SAMPLE_BUFFER_SIZE {
                        debug!(session_id, buffer_size = buffer.len(), "db: buffer full, flushing to database");
                        if let Err(e) = flush(&conn, &mut stmt, session_id, &mut buffer) {
                            error!(session_id, error = %e, "db: flush error");
                            return Err(e.into());
                        }
                    }
                }
                else {
                    warn!(session_id, total_samples, "db: sample receiver closed, exiting receive loop");
                    break;
                }
            }
            Ok(()) = shutdown.changed() => {
                info!(session_id, total_samples, "db: shutdown signal received");
                break;
            }
        }
    }

    // flush remainder on shutdown
    if !buffer.is_empty() {
        debug!(
            session_id,
            buffer_size = buffer.len(),
            "db: flushing remaining samples on shutdown"
        );
        flush(&conn, &mut stmt, session_id, &mut buffer)?;
    }

    end_session(&conn, session_id, Utc::now())?;
    info!(
        session_id,
        total_samples, "db: monitoring session ended gracefully"
    );
    Ok(())
}
