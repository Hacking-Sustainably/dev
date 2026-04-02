//! retrieve system energy usage samples from `powermetrics` and
//! convert to [`EnergySample`] structs for the database.
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::OnceLock;
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
static PROCESS_APP_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

fn process_app_map() -> &'static HashMap<&'static str, &'static str> {
    PROCESS_APP_MAP.get_or_init(|| {
        let mut m = HashMap::new();

        // ── Discord ──────────────────────────────────────────────────────────
        m.insert("Discord Helper", "Discord");
        m.insert("Discord Helper (Renderer)", "Discord");
        m.insert("Discord Helper (GPU)", "Discord");
        m.insert("Discord Helper (Plugin)", "Discord");

        // ── Slack ────────────────────────────────────────────────────────────
        m.insert("Slack Helper", "Slack");
        m.insert("Slack Helper (Renderer)", "Slack");
        m.insert("Slack Helper (GPU)", "Slack");
        m.insert("Slack Helper (Plugin)", "Slack");

        // ── Microsoft Teams ──────────────────────────────────────────────────
        m.insert("Teams Helper", "Microsoft Teams");
        m.insert("Teams Helper (Renderer)", "Microsoft Teams");
        m.insert("Teams Helper (GPU)", "Microsoft Teams");
        m.insert("Microsoft Teams Helper", "Microsoft Teams");
        m.insert("Microsoft Teams Helper (Renderer)", "Microsoft Teams");
        m.insert("Microsoft Teams Helper (GPU)", "Microsoft Teams");

        // ── Zoom ─────────────────────────────────────────────────────────────
        m.insert("zoom.us", "Zoom");
        m.insert("ZoomAudioDevice", "Zoom");
        m.insert("ZoomOpener", "Zoom");

        // ── Google Chrome ────────────────────────────────────────────────────
        m.insert("Google Chrome Helper", "Google Chrome");
        m.insert("Google Chrome Helper (Renderer)", "Google Chrome");
        m.insert("Google Chrome Helper (GPU)", "Google Chrome");
        m.insert("Google Chrome Helper (Plugin)", "Google Chrome");
        m.insert("Google Chrome Helper (Alerts)", "Google Chrome");

        // ── Chromium ─────────────────────────────────────────────────────────
        m.insert("Chromium Helper", "Chromium");
        m.insert("Chromium Helper (Renderer)", "Chromium");
        m.insert("Chromium Helper (GPU)", "Chromium");

        // ── Mozilla Firefox ──────────────────────────────────────────────────
        m.insert("firefox", "Firefox");
        m.insert("plugin-container", "Firefox");
        m.insert("RDD Process", "Firefox");
        m.insert("Web Content", "Firefox");

        // ── Safari ───────────────────────────────────────────────────────────
        m.insert("com.apple.WebKit.WebContent", "Safari");
        m.insert("com.apple.WebKit.Networking", "Safari");
        m.insert("com.apple.WebKit.GPU", "Safari");
        m.insert("SafariServices", "Safari");

        // ── Arc Browser ──────────────────────────────────────────────────────
        m.insert("Arc Helper", "Arc");
        m.insert("Arc Helper (Renderer)", "Arc");
        m.insert("Arc Helper (GPU)", "Arc");
        m.insert("Arc Helper (Plugin)", "Arc");

        // ── Brave Browser ────────────────────────────────────────────────────
        m.insert("Brave Browser Helper", "Brave Browser");
        m.insert("Brave Browser Helper (Renderer)", "Brave Browser");
        m.insert("Brave Browser Helper (GPU)", "Brave Browser");

        // ── Microsoft Edge ───────────────────────────────────────────────────
        m.insert("Microsoft Edge Helper", "Microsoft Edge");
        m.insert("Microsoft Edge Helper (Renderer)", "Microsoft Edge");
        m.insert("Microsoft Edge Helper (GPU)", "Microsoft Edge");

        // ── Opera ────────────────────────────────────────────────────────────
        m.insert("Opera Helper", "Opera");
        m.insert("Opera Helper (Renderer)", "Opera");
        m.insert("Opera Helper (GPU)", "Opera");

        // ── Visual Studio Code ───────────────────────────────────────────────
        m.insert("Code Helper", "Code");
        m.insert("Code Helper (Renderer)", "Code");
        m.insert("Code Helper (GPU)", "Code");
        m.insert("Code Helper (Plugin)", "Code");

        // ── Cursor ───────────────────────────────────────────────────────────
        m.insert("Cursor Helper", "Cursor");
        m.insert("Cursor Helper (Renderer)", "Cursor");
        m.insert("Cursor Helper (GPU)", "Cursor");
        m.insert("Cursor Helper (Plugin)", "Cursor");

        // ── Windsurf ─────────────────────────────────────────────────────────
        m.insert("Windsurf Helper", "Windsurf");
        m.insert("Windsurf Helper (Renderer)", "Windsurf");
        m.insert("Windsurf Helper (GPU)", "Windsurf");

        // ── Xcode ────────────────────────────────────────────────────────────
        m.insert("com.apple.dt.Xcode", "Xcode");
        m.insert("XCBBuildService", "Xcode");
        m.insert("IBAgent-x86_64", "Xcode");
        m.insert("sourcekit-lsp", "Xcode");
        m.insert("clangd", "Xcode");

        // ── JetBrains IDEs ───────────────────────────────────────────────────
        m.insert("idea", "IntelliJ IDEA");
        m.insert("idea_c", "IntelliJ IDEA");
        m.insert("pycharm", "PyCharm");
        m.insert("pycharm_c", "PyCharm");
        m.insert("webstorm", "WebStorm");
        m.insert("webstorm_c", "WebStorm");
        m.insert("goland", "GoLand");
        m.insert("goland_c", "GoLand");
        m.insert("clion", "CLion");
        m.insert("clion_c", "CLion");
        m.insert("datagrip", "DataGrip");
        m.insert("datagrip_c", "DataGrip");
        m.insert("rider", "Rider");
        m.insert("rider_c", "Rider");
        m.insert("rubymine", "RubyMine");

        // ── Spotify ──────────────────────────────────────────────────────────
        m.insert("Spotify Helper", "Spotify");
        m.insert("Spotify Helper (Renderer)", "Spotify");
        m.insert("Spotify Helper (GPU)", "Spotify");
        m.insert("SpotifyNotificationService", "Spotify");
        m.insert("SpotifyWebHelper", "Spotify");

        // ── Signal ───────────────────────────────────────────────────────────
        m.insert("Signal Helper", "Signal");
        m.insert("Signal Helper (Renderer)", "Signal");
        m.insert("Signal Helper (GPU)", "Signal");
        m.insert("Signal Helper (Plugin)", "Signal");

        // ── Telegram ─────────────────────────────────────────────────────────
        m.insert("Telegram Helper", "Telegram");
        m.insert("Telegram Helper (Renderer)", "Telegram");
        m.insert("Telegram Helper (GPU)", "Telegram");

        // ── WhatsApp ─────────────────────────────────────────────────────────
        m.insert("WhatsApp Helper", "WhatsApp");
        m.insert("WhatsApp Helper (Renderer)", "WhatsApp");
        m.insert("WhatsApp Helper (GPU)", "WhatsApp");

        // ── Steam ────────────────────────────────────────────────────────────
        m.insert("steam_osx", "Steam");
        m.insert("Steam Helper", "Steam");
        m.insert("Steam Helper (Renderer)", "Steam");
        m.insert("Steam Helper (GPU)", "Steam");
        m.insert("steamwebhelper", "Steam");
        m.insert("SteamService", "Steam");

        // ── Epic Games ───────────────────────────────────────────────────────
        m.insert("EpicGamesLauncher", "Epic Games Launcher");
        m.insert("EpicWebHelper", "Epic Games Launcher");

        // ── Battle.net ───────────────────────────────────────────────────────
        m.insert("Battle.net Helper", "Battle.net");
        m.insert("Agent.exe", "Battle.net");

        // ── 1Password ────────────────────────────────────────────────────────
        m.insert("1Password 7 - Password Manager", "1Password");
        m.insert("1Password Extension Helper", "1Password");
        m.insert("1Password Safari", "1Password");
        m.insert("op", "1Password");

        // ── Bitwarden ────────────────────────────────────────────────────────
        m.insert("Bitwarden Helper", "Bitwarden");
        m.insert("Bitwarden Helper (Renderer)", "Bitwarden");

        // ── Dropbox ──────────────────────────────────────────────────────────
        m.insert("DropboxHelper", "Dropbox");
        m.insert("dbcrash", "Dropbox");
        m.insert("dbfseventsd", "Dropbox");
        m.insert("dbxosd", "Dropbox");

        // ── OneDrive ─────────────────────────────────────────────────────────
        m.insert("OneDriveStandaloneUpdater", "OneDrive");
        m.insert("OneDriveHelper", "OneDrive");

        // ── Google Drive ─────────────────────────────────────────────────────
        m.insert("Google Drive File Stream", "Google Drive");
        m.insert("googledrivesync", "Google Drive");
        m.insert("GoogleDriveFSHelper", "Google Drive");

        // ── iCloud / Apple services ──────────────────────────────────────────
        m.insert("bird", "iCloud");
        m.insert("cloudd", "iCloud");
        m.insert("cloudpaird", "iCloud");
        m.insert("com.apple.iCloudHelper", "iCloud");
        m.insert("Photos Library Helper", "Photos");

        // ── Microsoft Office ─────────────────────────────────────────────────
        m.insert("Microsoft Word", "Word");
        m.insert("Microsoft Excel", "Excel");
        m.insert("Microsoft PowerPoint", "PowerPoint");
        m.insert("Microsoft Outlook", "Outlook");
        m.insert("Microsoft OneNote", "OneNote");
        m.insert("MicrosoftAutoupdate", "Microsoft AutoUpdate");

        // ── Notion ───────────────────────────────────────────────────────────
        m.insert("Notion Helper", "Notion");
        m.insert("Notion Helper (Renderer)", "Notion");
        m.insert("Notion Helper (GPU)", "Notion");

        // ── Figma ────────────────────────────────────────────────────────────
        m.insert("Figma Helper", "Figma");
        m.insert("Figma Helper (Renderer)", "Figma");
        m.insert("Figma Helper (GPU)", "Figma");

        // ── Docker ───────────────────────────────────────────────────────────
        m.insert("com.docker.backend", "Docker");
        m.insert("com.docker.vmnetd", "Docker");
        m.insert("com.docker.hyperkit", "Docker");
        m.insert("Docker Desktop Helper", "Docker");
        m.insert("Docker Desktop Helper (Renderer)", "Docker");
        m.insert("docker", "Docker");
        m.insert("dockerd", "Docker");
        m.insert("vpnkit", "Docker");

        // ── Terminal emulators ───────────────────────────────────────────────
        m.insert("iTerm2", "iTerm2");
        m.insert("com.googlecode.iterm2", "iTerm2");
        m.insert("wezterm-gui", "WezTerm");

        // ── Alfred / Raycast / Spotlight ─────────────────────────────────────
        m.insert("com.runningwithcrayons.Alfred", "Alfred");
        m.insert("Alfred Helper", "Alfred");

        // ── OBS Studio ───────────────────────────────────────────────────────
        m.insert("obs", "OBS Studio");
        m.insert("OBS Helper", "OBS Studio");
        m.insert("OBS Helper (Renderer)", "OBS Studio");
        m.insert("OBS Helper (GPU)", "OBS Studio");

        // ── VLC ──────────────────────────────────────────────────────────────
        m.insert("VLC media player", "VLC");
        m.insert("VLC Helper", "VLC");

        // ── Plex ─────────────────────────────────────────────────────────────
        m.insert("Plex Media Server", "Plex");
        m.insert("PlexMediaServer", "Plex");
        m.insert("Plex Helper", "Plex");

        // ── Adobe apps ───────────────────────────────────────────────────────
        m.insert("Adobe Photoshop 2024", "Photoshop");
        m.insert("Adobe Photoshop 2025", "Photoshop");
        m.insert("Adobe Illustrator 2024", "Illustrator");
        m.insert("Adobe Illustrator 2025", "Illustrator");
        m.insert("Adobe Premiere Pro 2024", "Premiere Pro");
        m.insert("Adobe After Effects 2024", "After Effects");
        m.insert("Adobe Lightroom", "Lightroom");
        m.insert("Adobe Acrobat", "Acrobat");

        // ── Bartender / system UI helpers ────────────────────────────────────
        m.insert("Bartender 4 Helper", "Bartender");
        m.insert("Bartender 5 Helper", "Bartender");

        // ── macOS system processes that are worth naming nicely ───────────────
        m.insert("WindowServer", "WindowServer");
        m.insert("kernel_task", "kernel_task");
        m.insert("launchd", "launchd");
        m.insert("mds", "Spotlight");
        m.insert("mds_stores", "Spotlight");
        m.insert("mdworker_shared", "Spotlight");
        m.insert("mdworker", "Spotlight");
        m.insert("com.apple.CoreSimulator.CoreSimulatorService", "Simulator");
        m.insert("Simulator", "Simulator");
        m.insert("SimulatorBridge", "Simulator");
        m.insert("mediaanalysisd", "Media Analysis");
        m.insert("mediaremoted", "Media Remote");

        m
    })
}

#[derive(Debug, Deserialize)]
pub struct PowermetricsSample {
    pub timestamp: String,
    pub elapsed_ns: u64,
    pub coalitions: Vec<CoalitionSample>,

    pub processor: ProcessorMetrics,
}

#[derive(Debug, Deserialize)]
pub struct ProcessorMetrics {
    pub cpu_power: f64,
    pub gpu_power: f64,
    #[allow(dead_code)]
    pub combined_power: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CoalitionSample {
    #[serde(default)]
    pub id: Option<u64>,

    pub name: String, // bundle ID, e.g. "com.apple.mail"

    pub cputime_ms_per_s: f64,
    pub energy_impact: f64,

    #[serde(default)]
    pub intr_wakeups: i64,
    #[serde(default)]
    pub idle_wakeups: i64,
    #[serde(default)]
    pub diskio_bytesread: i64,
    #[serde(default)]
    pub diskio_byteswritten: i64,

    #[serde(default)]
    pub gputime_ms_per_s: f64,

    pub tasks: Vec<ProcessSample>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ProcessSample {
    pub pid: i32,
    pub name: String,
    pub cputime_ms_per_s: f64,

    pub intr_wakeups: i64,
    pub intr_wakeups_per_s: f64,
    pub idle_wakeups: i64,
    pub idle_wakeups_per_s: f64,

    pub timer_wakeups: Vec<TimerWakeup>,

    #[serde(default)]
    pub diskio_bytesread: i64,
    #[serde(default)]
    pub diskio_byteswritten: i64,

    #[serde(default)]
    pub pageins: i64,
    #[serde(default)]
    pub pageins_per_s: f64,

    #[serde(default)]
    pub bytes_received: i64,
    #[serde(default)]
    pub bytes_sent: i64,

    #[serde(default)]
    pub energy_impact: f64,

    #[serde(default)]
    pub gputime_ms_per_s: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TimerWakeup {
    pub interval_ns: f64,
    pub wakeups: i64,
    pub wakeups_per_s: f64,
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
            "--show-process-coalition",
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
        sample_count = sample.coalitions.len(),
        "converting powermetrics sample"
    );
    let timestamp: Timestamp = DateTime::parse_from_rfc3339(&sample.timestamp)
        .expect("invalid timestamp")
        .with_timezone(&Utc);

    let map = process_app_map();
    let duration_s = sample.elapsed_ns as f64 / 1_000_000_000.0;

    let total_cpu: f64 = sample.coalitions.iter().map(|t| t.cputime_ms_per_s).sum();

    let total_gpu: f64 = sample.coalitions.iter().map(|t| t.gputime_ms_per_s).sum();

    let cpu_power = sample.processor.cpu_power;
    let gpu_power = sample.processor.gpu_power;
    let _total_power: f64 = sample.processor.combined_power;

    let pids: Vec<sysinfo::Pid> = sample
        .coalitions
        .iter()
        .flat_map(|c| c.tasks.iter())
        .map(|t| sysinfo::Pid::from(t.pid as usize))
        .collect();

    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );

    for coal in sample.coalitions {
        // try to figure out the actual app name
        let app_name = coal.name.split('.').next_back();

        let category = if coal.name.starts_with("com.apple.") {
            Some("system".to_string())
        } else if coal.name.contains("Safari")
            || coal.name.contains("Chrome")
            || coal.name.contains("Firefox")
        {
            Some("browser".to_string())
        } else {
            None
        };

        for proc in coal.tasks {
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

            let app_name = map
                .get(proc.name.as_str())
                .copied()
                .unwrap_or_else(|| app_name.unwrap_or(proc.name.as_str()))
                .to_string();

            buf.push(EnergySample {
                id: None,
                session_id,
                timestamp,
                app_name: app_name.clone(),

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

                category: category.clone(),
                is_background: false,
            });
        }
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
