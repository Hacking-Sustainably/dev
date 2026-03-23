//! 
use std::process::Stdio;

use tokio::process::Command;
use tokio::io::{AsyncReadExt, BufReader};
use serde::Deserialize;
use tokio::sync::mpsc::Sender;

use crate::schema::EnergySample;

#[derive(Deserialize)]
struct PowermetricsSample {
    elapsed_ns: u64,
    cpu_power: Option<f64>,
    gpu_power: Option<f64>,
    combined_power: Option<f64>,
    tasks: TasksWrapper,
}

#[derive(Deserialize)]
struct TasksWrapper {
    tasks: Vec<ProcessSample>,
}

#[derive(Deserialize)]
struct ProcessSample {
    pid: u32,
    name: String,
    cpu_ms_per_s: f64,
    #[serde(default)]
    gpu_ms_per_s: f64,
}

pub async fn spawn_powermetrics(tx: Sender<EnergySample>) -> std::io::Result<()> {
    let mut child = Command::new("sudo")
        .args(["powermetrics",
               "--samplers", "cpu_power,gpu_power,tasks",
               "--output-format", "json",
               "-i", "5000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::with_capacity(128 * 1024); // 128KB initial
    
    loop {
        let mut byte = [0u8; 1];
        // read until form-feed
        loop {
            reader.read_exact(&mut byte).await?;
            if byte[0] == b'\x0c' { break; }
            buf.push(byte[0]);
        }
    
        let sample = parse_sample(&buf).ok_or_else(|| todo!("handle parse error somehow"))?;
        buf.clear();
    
        if let Err(e) = tx.send(sample.into()).await {
            // channel closed
            break;
        }
    }
    
    child.kill().await
}

fn parse_sample(buf: &[u8]) -> Option<PowermetricsSample> {
    todo!()
}

impl From<PowermetricsSample> for EnergySample {
    fn from(value: PowermetricsSample) -> Self {
        todo!()
    }
}