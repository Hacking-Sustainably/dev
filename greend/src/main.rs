//! greend daemon for greenb.
//! periodically samples system wide energy usage,
//! writes it to the greenb database for later processing

use greend::InternalError;
use greend::LocalTimer;
use greend::SAMPLE_BUFFER_SIZE;
use greend::db;
use greend::get_database_path;
use greend::idle;
use greend::subprocess;
use greend::wait_for_signal;
use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::error;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), InternalError> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                if cfg!(debug_assertions) {
                    "greend=trace".into()
                } else {
                    "greend=info".into()
                }
            }),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true)
                .with_timer(LocalTimer)
                .with_level(true), // .with_thread_ids(true),
        )
        .init();

    info!("greend daemon starting up");

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

    let db_path = get_database_path()?;
    info!(?db_path, "database path initialized");

    // metrics task:
    // - spawns powermetrics as subprocess
    // - keep reading output until '\0xc' or </plist> (?)
    // - use serde and plist to deserialise output into PowermetricsSample
    // - convert PowermetricsSample into EnergySample
    // - send over spsc channel

    info!("spawning metrics task");
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

    // database writing is a synchronous operation and should always be on 1 thread,
    // regardless of the runtime used.
    let local = tokio::task::LocalSet::new();
    info!("spawning writer task");
    let shutdown = shutdown_rx.clone();
    let mut writer_handle = local.spawn_local(async move {
        db::writer_task(sample_receiver, &db_path, session_tx, shutdown).await
    });

    info!("spawning idle detection task");
    join_set.spawn(async move { idle::idle_task(sample_rate_tx, shutdown_rx).await });

    // signal handler:
    // - process SIGTERM
    // - flush database and update MonitoringSession
    info!("setting up signal handler for SIGINT/SIGTERM");
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    info!("all tasks spawned, entering main event loop");
    local
        .run_until(async {
            tokio::select! {
                res = &mut writer_handle => {
                    error!("writer task exited unexpectedly, initiating shutdown");
                    let _ = shutdown_tx.send(true);
                    let _ = join_set.join_all().await;
                    match res {
                        Ok(Ok(())) => Err(InternalError::TaskError),
                        Ok(Err(e)) => Err(e), // propagate writer error
                        Err(join_err) => Err(InternalError::TaskJoinError(join_err)),
                    }
                }
                Some(join_result) = join_set.join_next() => {
                    error!("unexpected task termination, initiating shutdown");
                    let _ = shutdown_tx.send(true);
                    let _ = join_set.join_all().await;
                    let _ = writer_handle.await;
                    match join_result {
                        Ok(Ok(())) => {
                            error!("some task exited for no reason");
                            Err(InternalError::TaskError)
                        }
                        Ok(Err(err)) => {
                            error!("task returned error: {err}");
                            Err(err)
                        }
                        Err(err) => {
                            error!("task join error: {err}");
                            Err(err.into())
                        }
                    }
                }
                _ = wait_for_signal(&mut sigterm, &mut sigint) => {
                    info!("SIGINT/SIGTERM received, initiating graceful shutdown");
                    let _ = shutdown_tx.send(true);
                    let _ = join_set.join_all().await;
                    let _ = writer_handle.await;
                    info!("graceful shutdown complete");
                    Ok(())
                }
            }
        })
        .await
}
