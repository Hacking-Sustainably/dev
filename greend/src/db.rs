//! deutche bahn (sqlite)

use chrono::Utc;
use rusqlite::Connection;
use rusqlite::params;
use tokio::sync::mpsc::Receiver;
use tokio::sync::oneshot;

use crate::BUFFER_SIZE;
use crate::InternalError;
use crate::schema::EnergySample;
use crate::schema::MonitoringSession;
use crate::schema::Timestamp;

/// Creates the `monitoring_sessions` and `energy_samples` tables if they don't
/// already exist.
fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
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
    )
}

/// Inserts a new [`MonitoringSession`] row and returns the assigned `rowid`.
/// `session.id` is ignored on insert — SQLite assigns it automatically.
fn insert_session(conn: &Connection, session: &MonitoringSession) -> rusqlite::Result<i64> {
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

    Ok(conn.last_insert_rowid())
}

/// Updates the `ended_at` column on a session row once monitoring stops.
fn end_session(conn: &Connection, session_id: i64, ended_at: Timestamp) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE monitoring_sessions SET ended_at = ?1 WHERE id = ?2",
        params![ended_at.to_rfc3339(), session_id],
    )?;
    Ok(())
}

pub async fn writer_task(
    mut rx: Receiver<EnergySample>,
    db_path: &str,
    session_tx: oneshot::Sender<i64>,
) -> Result<(), InternalError> {
    let conn = Connection::open(db_path)?;

    conn.execute_batch(
        "
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        PRAGMA cache_size=-8000;
    ",
    )?;

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
    tracing::info!(session_id, "monitoring session started");

    let _ = session_tx.send(session_id);

    let mut buffer: Vec<EnergySample> = Vec::with_capacity(BUFFER_SIZE);

    while let Some(sample) = rx.recv().await {
        buffer.push(sample);
        if buffer.len() >= BUFFER_SIZE {
            flush(&conn, session_id, &mut buffer)?;
        }
    }

    // flush remainder on shutdown
    flush(&conn, session_id, &mut buffer)?;

    end_session(&conn, session_id, Utc::now())?;
    tracing::info!(session_id, "monitoring session ended");
    Ok(())
}

fn flush(
    conn: &Connection,
    session_id: i64,
    buffer: &mut Vec<EnergySample>,
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    for s in buffer.drain(..) {
        conn.execute(
            "INSERT INTO energy_samples
                 (session_id, timestamp, app_name, pid,
                  power_watts, energy_joules, cpu_percent, memory_mb, gpu_percent,
                  disk_read_mb, disk_write_mb, network_sent_mb, network_recv_mb,
                  category, is_background)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
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
            ],
        )?;
    }
    tx.commit()
}
