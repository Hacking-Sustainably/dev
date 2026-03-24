//! retrieve system energy usage samples from `powermetrics` and
//! convert to [`EnergySample`] structs for the database.
use std::process::Stdio;

use serde::Deserialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

use crate::SHUTDOWN;
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

pub async fn spawn_powermetrics(tx: Sender<EnergySample>, session_id: i64) -> std::io::Result<()> {
    info!("spawning powermetrics child");
    let mut child = Command::new("sudo")
        .args([
            "powermetrics",
            "--samplers",
            "cpu_power,gpu_power,tasks",
            "--format",
            "plist",
            "-i",
            "5000",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::with_capacity(128 * 1024); // 128KB initial

    let mut sample_buf = Vec::with_capacity(32);

    'read_loop: loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            break 'read_loop;
        }
        // read until null byte
        reader.read_until(0, &mut buf).await?;

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
                break 'read_loop;
            }
        }
    }

    info!("killing powermetrics child");
    child.kill().await
}

fn parse_sample(buf: &[u8]) -> Result<PowermetricsSample, plist::Error> {
    plist::from_bytes(buf)
}

fn convert_samples(session_id: i64, value: PowermetricsSample, buf: &mut [EnergySample]) {
    todo!()
}
