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
use tracing::info;
use tracing::trace;
use tracing::warn;

use crate::schema::EnergySample;

#[derive(Debug, Deserialize)]
struct PowermetricsSample {
    elapsed_ns: u64,
    cpu_power: Option<f64>,
    gpu_power: Option<f64>,
    combined_power: Option<f64>,
    tasks: TasksWrapper,
}

#[derive(Debug, Deserialize)]
struct TasksWrapper {
    tasks: Vec<ProcessSample>,
}

#[derive(Debug, Deserialize)]
struct ProcessSample {
    pid: u32,
    name: String,
    cpu_ms_per_s: f64,
    #[serde(default)]
    gpu_ms_per_s: f64,
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
    todo!()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_parse_sample() {
        let sample_input =
            std::fs::read_to_string(PathBuf::from_str("../sample-output.xml").unwrap()).unwrap();
        let sample = parse_sample(sample_input.as_bytes()).unwrap();
        println!("{sample:?}");
    }
}
