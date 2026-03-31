//! retrieve system energy usage samples from `powermetrics` and
//! convert to [`EnergySample`] structs for the database.
use std::process::Stdio;
use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use sysinfo::ProcessRefreshKind;
use sysinfo::ProcessesToUpdate;
use sysinfo::System;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use tokio::sync::watch;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

use crate::schema::EnergySample;
use crate::schema::Timestamp;

#[derive(Debug, Deserialize)]
pub struct PowermetricsSample {
    timestamp: String,
    elapsed_ns: u64,
    tasks: Vec<ProcessSample>,

    pub processor: ProcessorMetrics,
}

#[derive(Debug, Deserialize)]
pub struct ProcessorMetrics {
    pub cpu_power: f64,
    pub gpu_power: f64,
    pub combined_power: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ProcessSample {
    pid: i32,
    name: String,
    cputime_ms_per_s: f64,

    intr_wakeups: i64,
    intr_wakeups_per_s: f64,
    idle_wakeups: i64,
    idle_wakeups_per_s: f64,

    timer_wakeups: Vec<TimerWakeup>,

    #[serde(default)]
    diskio_bytesread: i64,
    #[serde(default)]
    diskio_byteswritten: i64,

    #[serde(default)]
    pageins: i64,
    #[serde(default)]
    pageins_per_s: f64,

    #[serde(default)]
    bytes_received: i64,
    #[serde(default)]
    bytes_sent: i64,

    #[serde(default)]
    energy_impact: f64,

    #[serde(default)]
    gputime_ms_per_s: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct TimerWakeup {
    interval_ns: f64,
    wakeups: i64,
    wakeups_per_s: f64,
}

pub async fn spawn_powermetrics(
    tx: Sender<EnergySample>,
    session_id: i64,
    mut shutdown: watch::Receiver<bool>,
    mut interval_ms: watch::Receiver<u64>,
) -> std::io::Result<()> {
    info!(session_id, "powermetrics: starting subprocess");
    let (mut child, stdout) = start_child(*interval_ms.borrow_and_update())?;
    info!(
        session_id,
        pid = child.id(),
        "powermetrics: process spawned"
    );

    let mut sys = System::new();

    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::with_capacity(512 * 1024); // 512KB initial

    let mut sample_buf = Vec::with_capacity(256);
    let mut current_interval = *interval_ms.borrow_and_update();
    let mut sample_count = 0u64;

    'outer: loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!(session_id, "powermetrics: shutdown signal received");
                    break 'outer;
                }
            },
            _ = interval_ms.changed() => {
                debug!(session_id, "powermetrics: interval change detected, waiting 10s before restart");
                tokio::time::sleep(Duration::from_secs(10)).await;
                // then drain any further changes before restarting
                while interval_ms.has_changed().unwrap_or(false) {
                    interval_ms.mark_unchanged();
                }
                let new_interval = *interval_ms.borrow_and_update();
                if new_interval != current_interval {
                    info!(session_id, old_interval = current_interval, new_interval, "powermetrics: restarting with new interval");
                    child.kill().await?;
                    let (new_child, new_stdout) = start_child(new_interval)?;
                    child = new_child;
                    reader = BufReader::new(new_stdout);
                    buf.clear();
                    current_interval = new_interval;
                }
            }
            x = reader.read_until(0, &mut buf) => {
                match x {
                    Ok(0) => {
                        warn!(session_id, "powermetrics: stream closed unexpectedly");
                        break 'outer;
                    },
                    Ok(_) => {
                        let sample = match parse_sample(&buf) {
                            Ok(s) => s,
                            Err(e) => {
                                error!(session_id, error = %e, "powermetrics: parse error");
                                debug!(session_id, "powermetrics: parse error: {e}");
                                trace!("buffer: \n{:?}", String::from_utf8_lossy(&buf));
                                buf.clear();
                                continue;
                            }
                        };
                        trace!(session_id, "powermetrics: parsed sample successfully");
                        buf.clear();
                        buf.shrink_to(512);

                        convert_samples(&mut sys, session_id, sample, &mut sample_buf);
                        let sample_count_batch = sample_buf.len();

                        for sample in sample_buf.drain(..) {
                            if let Err(e) = tx.send(sample).await {
                                // channel closed
                                warn!(session_id, error = %e, "powermetrics: send error, channel closed");
                                break 'outer;
                            }
                        }

                        sample_count += sample_count_batch as u64;
                        trace!(session_id, sample_count, "powermetrics: samples sent");
                    }
                    Err(e) => {
                        buf.clear();
                        error!(session_id, error = %e, "powermetrics: read error");
                        return Err(e);
                    }
                }
            }
        }
    }

    info!(
        session_id,
        total_samples = sample_count,
        "powermetrics: shutting down"
    );
    child.kill().await
}

fn start_child(interval_ms: u64) -> std::io::Result<(Child, ChildStdout)> {
    debug!(
        "powermetrics: spawning 'sudo powermetrics' with interval {}ms",
        interval_ms
    );
    let mut child = Command::new("sudo")
        .args([
            "powermetrics",
            "--samplers",
            "default",
            "--show-process-gpu",
            "--show-process-netstats",
            "--show-process-energy",
            "--format",
            "plist",
            "-i",
            &interval_ms.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    debug!(pid = child.id(), "powermetrics: child process started");
    Ok((child, stdout))
}

fn parse_sample(buf: &[u8]) -> Result<PowermetricsSample, plist::Error> {
    let data = buf.strip_suffix(&[0]).unwrap_or(buf);
    plist::from_bytes(data)
}

fn convert_samples(
    system: &mut System,
    session_id: i64,
    sample: PowermetricsSample,
    buf: &mut Vec<EnergySample>,
) {
    trace!(
        session_id,
        sample_count = sample.tasks.len(),
        "converting powermetrics sample"
    );
    let timestamp: Timestamp = DateTime::parse_from_rfc3339(&sample.timestamp)
        .expect("invalid timestamp")
        .with_timezone(&Utc);

    let duration_s = sample.elapsed_ns as f64 / 1_000_000_000.0;

    let total_cpu: f64 = sample.tasks.iter().map(|t| t.cputime_ms_per_s).sum();

    let total_gpu: f64 = sample.tasks.iter().map(|t| t.gputime_ms_per_s).sum();

    let cpu_power = sample.processor.cpu_power;
    let gpu_power = sample.processor.gpu_power;
    let _total_power: f64 = sample.processor.combined_power;

    let pids: Vec<sysinfo::Pid> = sample
        .tasks
        .iter()
        .map(|t| sysinfo::Pid::from(t.pid as usize))
        .collect();

    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );

    for proc in sample.tasks {
        if total_cpu == 0.0 {
            continue;
        }

        // query process metrics from proc_pidinfo
        let pid = sysinfo::Pid::from(proc.pid as usize);
        let memory_mb = system
            .process(pid)
            .map(|p| p.memory() as f64 / 1024.0 / 1024.0);

        let cpu_ratio = if total_cpu > 0.0 {
            proc.cputime_ms_per_s / total_cpu
        } else {
            0.0
        };

        let gpu_ratio = if total_gpu > 0.0 {
            proc.gputime_ms_per_s / total_gpu
        } else {
            0.0
        };

        let total_wakeups = proc.intr_wakeups + proc.idle_wakeups;

        let _idle_wakeup_ratio = if total_wakeups > 0 {
            proc.idle_wakeups as f64 / total_wakeups as f64
        } else {
            0.0
        };

        let _intr_wakeup_ratio = if total_wakeups > 0 {
            proc.intr_wakeups as f64 / total_wakeups as f64
        } else {
            0.0
        };

        let process_power = cpu_power * cpu_ratio + gpu_power * gpu_ratio;

        let energy_joules = process_power * duration_s;

        let cpu_percent = proc.cputime_ms_per_s / 1000.0 * 100.0;
        let gpu_percent = proc.gputime_ms_per_s / 1000.0 * 100.0;

        buf.push(EnergySample {
            id: None,
            session_id,
            timestamp,
            app_name: proc.name.clone(),
            pid: Some(proc.pid),

            power_watts: Some(process_power),
            energy_joules: Some(energy_joules),
            cpu_percent: Some(cpu_percent),

            memory_mb,
            gpu_percent: Some(gpu_percent),
            disk_read_mb: Some(proc.diskio_bytesread as f64 / 1024.0 / 1024.0),
            disk_write_mb: Some(proc.diskio_byteswritten as f64 / 1024.0 / 1024.0),
            network_sent_mb: Some(proc.bytes_sent as f64 / 1024.0 / 1024.0),
            network_recv_mb: Some(proc.bytes_received as f64 / 1024.0 / 1024.0),

            category: None,
            is_background: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_parse_sample() {
        let sample_input = std::fs::read_to_string(
            PathBuf::from_str("../test_samples/sample-output.xml").unwrap(),
        )
        .unwrap();
        let sample = parse_sample(sample_input.as_bytes()).unwrap();
        println!("{sample:?}");
    }
}
