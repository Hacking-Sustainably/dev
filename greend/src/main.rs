//! greend daemon for greenb.
//! periodically samples system wide energy usage,
//! writes it to the greenb database for later processing

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;

use greend::BUFFER_SIZE;
use greend::InternalError;
use greend::SHUTDOWN;
use greend::db;
use greend::subprocess;
use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

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

    let (error_sender, mut error_receiver) = mpsc::channel(2);
    let (session_tx, session_rx) = oneshot::channel();

    // hardcoded for now
    let db_path = "../instance/energy_monitor.db";

    let (sample_sender, sample_receiver) = mpsc::channel(BUFFER_SIZE * 4);

    // metrics task:
    // - spawns powermetrics as subprocess
    // - keep reading output until '\0xc' or </plist> (?)
    // - use serde and plist to deserialise output into PowermetricsSample
    // - convert PowermetricsSample into EnergySample
    // - send over spsc channel

    let es = error_sender.clone();
    tokio::spawn(async move {
        if let Err(e) = subprocess::metrics_task(sample_sender, session_rx).await {
            es.send(e).await.ok();
        }
    });

    // writer task:
    // - on spawn, write a new MonitoringSession to the database
    // - read from channel
    // - buffer EnergySamples
    // - flush to database
    // - on quit, update end time in MonitoringSession

    tokio::spawn(async move {
        if let Err(e) = db::writer_task(sample_receiver, db_path, session_tx).await {
            error_sender.send(e).await.ok();
        }
    });

    // signal handler:
    // - process SIGTERM
    // - flush database and update MonitoringSession
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        Some(err) = error_receiver.recv() => {
            SHUTDOWN.store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(err);
        }
        _ = sigterm.recv() => {
            tracing::info!("SIGTERM received, shutting down");
            SHUTDOWN.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    Ok(())
}
