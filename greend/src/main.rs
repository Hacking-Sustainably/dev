//! greend daemon for greenb.
//! periodically samples system wide energy usage,
//! writes it to the greenb database for later processing

use greend::InternalError;
use greend::SAMPLE_BUFFER_SIZE;
use greend::db;
use greend::get_database_path;
use greend::idle;
use greend::subprocess;
use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::error;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), InternalError> {
    // todo:
    // - detect database from any of:
    //   - environment variable
    //   - command line argument
    //   - greenb config file
    //   - default path
    // - create database if needeD?
    // - create spsc for EnergySample(s)
    // - spawn writer task with consumer
    // - spawn metrics task with producer
    // - detect system idle and update sampling/buffering rate

    let (session_tx, session_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (sample_rate_tx, sample_rate) = watch::channel(5_000);
    let (sample_sender, sample_receiver) = mpsc::channel(SAMPLE_BUFFER_SIZE * 4);

    let mut join_set = tokio::task::JoinSet::new();

    // hardcoded for now
    let db_path = get_database_path()?;

    // metrics task:
    // - spawns powermetrics as subprocess
    // - keep reading output until '\0xc' or </plist> (?)
    // - use serde and plist to deserialise output into PowermetricsSample
    // - convert PowermetricsSample into EnergySample
    // - send over spsc channel

    let shutdown = shutdown_rx.clone();
    join_set.spawn(async move {
        subprocess::metrics_task(sample_sender, session_rx, shutdown, sample_rate).await
    });

    // writer task:
    // - on spawn, write a new MonitoringSession to the database
    // - read from channel
    // - buffer EnergySamples
    // - flush to database
    // - on quit, update end time in MonitoringSession

    let shutdown = shutdown_rx.clone();
    join_set.spawn(async move {
        db::writer_task(sample_receiver, &db_path, session_tx, shutdown).await
    });

    join_set.spawn(async move { idle::idle_task(sample_rate_tx, shutdown_rx).await });

    // signal handler:
    // - process SIGTERM
    // - flush database and update MonitoringSession
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        Some(join_result) = join_set.join_next() => {
            let _ = shutdown_tx.send(true);
            let _ = join_set.join_all().await;
            match join_result {
                Ok(Ok(())) => {
                    error!("some task exited for no reason");
                    return Err(InternalError::TaskError);
                }
                Ok(Err(err)) => {
                    return Err(err);
                }
                Err(err) => {
                    error!("task join error: {err}");
                    return Err(err.into());
                }
            }
        }
        _ = sigterm.recv() => {
            tracing::info!("SIGTERM received, shutting down");
            let _ = shutdown_tx.send(true);
            let _ = join_set.join_all().await;
            return Ok(());
        }
    }
}
