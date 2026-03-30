//! retrieve system energy usage samples from `powermetrics` and
//! convert to [`EnergySample`] structs for the database.
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use tokio::sync::watch;
use tracing::error;
use tracing::info;    use chrono::{DateTime, Utc};

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

#[derive(Debug, Deserialize)]
struct ProcessSample {
    pid: u32,
    name: String,
    cputime_ms_per_s: f64,
    #[serde(default)]
    gputime_ms_per_s: f64,
}

pub async fn spawn_powermetrics(
    tx: Sender<EnergySample>,
    session_id: i64,
    mut shutdown: watch::Receiver<bool>,
    mut interval_ms: watch::Receiver<u64>,
) -> std::io::Result<()> {
    let (mut child, stdout) = start_child(*interval_ms.borrow_and_update())?;

    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::with_capacity(128 * 1024); // 128KB initial

    let mut sample_buf = Vec::with_capacity(32);
    let mut current_interval = *interval_ms.borrow_and_update();
    'outer: loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break 'outer;
                }
            },
            _ = interval_ms.changed() => {
                tokio::time::sleep(Duration::from_secs(10)).await;
                // then drain any further changes before restarting
                while interval_ms.has_changed().unwrap_or(false) {
                    interval_ms.mark_unchanged();
                }
                let new_interval = *interval_ms.borrow_and_update();
                if new_interval != current_interval {
                    info!("interval_ms changed to {new_interval}");
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
                        warn!("stream closed?");
                    },
                    Ok(_) => {
                        let sample = match parse_sample(&buf) {
                            Ok(s) => s,
                            Err(e) => {
                                buf.clear();
                                error!("powermetrics parse error: {e}");
                                continue;
                            }
                        };
                        trace!("parsed sample: {sample:?}");
                        buf.clear();

                        convert_samples(session_id, sample, &mut sample_buf);
                        for sample in sample_buf.drain(..) {
                            if let Err(e) = tx.send(sample).await {
                                // channel closed
                                warn!("channel closed for powermetrics task: {e}");
                                break 'outer;
                            }
                        }
                    }
                    Err(e) => {
                        buf.clear();
                        return Err(e);
                    }
                }
            }
        }
    }

    child.kill().await
}

fn start_child(interval_ms: u64) -> std::io::Result<(Child, ChildStdout)> {
    let mut child = Command::new("sudo")
        .args([
            "powermetrics",
            "--samplers",
            "cpu_power,gpu_power,tasks",
            "--format",
            "plist",
            "-i",
            &interval_ms.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    Ok((child, stdout))
}

fn parse_sample(buf: &[u8]) -> Result<PowermetricsSample, plist::Error> {
    plist::from_bytes(buf)
}

fn convert_samples(session_id: i64, value: PowermetricsSample, buf: &mut Vec<EnergySample>) {
    
    let timestamp: Timestamp = DateTime::parse_from_rfc3339(&value.timestamp)
        .expect("invalid timestamp")
        .with_timezone(&Utc);
    
    let duration_s = value.elapsed_ns as f64 / 1_000_000_000.0;
    
    let total_cpu: f64 = value.tasks
        .iter()
        .map(|t| t.cputime_ms_per_s)
        .sum();
    
    let total_gpu: f64 = value.tasks
        .iter()
        .map(|t| t.gputime_ms_per_s)
        .sum();
    
    let cpu_power = value.processor.cpu_power;
    let gpu_power = value.processor.gpu_power;
    let total_power: f64 = value.processor.combined_power;
    
    for proc in value.tasks {
        if total_cpu == 0.0 {
            continue;
        }
        
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
        
        let process_power =
            cpu_power * cpu_ratio +
            gpu_power * gpu_ratio;
        
        let energy_joules = process_power * duration_s;
        
        let cpu_percent = proc.cputime_ms_per_s / 1000.0 * 100.0;
        
        buf.push(EnergySample {
            id: None,
            session_id,
            timestamp: timestamp.clone(),
            app_name: proc.name.clone(),
            pid: Some(proc.pid as u32),
            
            power_watts: Some(process_power),
            energy_joules: Some(energy_joules),
            cpu_percent: Some(cpu_percent),
            
            memory_mb: None,
            gpu_percent: None,
            disk_read_mb: None,
            disk_write_mb: None,
            network_sent_mb: None,
            network_recv_mb: None,
            
            category: None,
            is_background: true,
            
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
        let sample_input =
            std::fs::read_to_string(PathBuf::from_str("../test_samples/sample-output.xml").unwrap()).unwrap();
        let sample = parse_sample(sample_input.as_bytes()).unwrap();
        println!("{sample:?}");
    }
}
