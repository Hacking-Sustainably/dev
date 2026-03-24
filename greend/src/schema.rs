//! the data structures corresponding to database fields

use chrono::DateTime;
use chrono::Utc;

pub type Timestamp = DateTime<Utc>;

#[derive(Debug)]
pub struct MonitoringSession {
    pub id: Option<i64>,
    pub name: String,
    pub device_name: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

#[derive(Debug)]
pub struct EnergySample {
    pub id: Option<i64>,
    pub session_id: i64,

    pub timestamp: Timestamp,
    pub app_name: String,
    pub pid: Option<u32>,

    // Energy metrics
    /// instantaneous power draw (W)
    pub power_watts: Option<f64>,
    /// energy consumed in interval (J)
    pub energy_joules: Option<f64>,
    /// CPU utilisation %
    pub cpu_percent: Option<f64>,
    /// memory usage in MB
    pub memory_mb: Option<f64>,
    /// GPU utilisation %
    pub gpu_percent: Option<f64>,
    /// disk read in MB
    pub disk_read_mb: Option<f64>,
    /// disk write in MB
    pub disk_write_mb: Option<f64>,
    /// network sent in MB
    pub network_sent_mb: Option<f64>,
    /// network received in MB
    pub network_recv_mb: Option<f64>,

    // Classification
    /// e.g. "browser", "ide", "game"
    pub category: Option<String>,
    pub is_background: bool,
}

// #[derive(Debug)]
// pub struct EnergyRating {
//     pub id: Option<i64>,
//     pub app_name: String,
//     pub category: Option<String>,

//     pub total_energy_joules: f64,
//     pub avg_power_watts: f64,
//     pub avg_cpu_percent: f64,
//     pub avg_memory_mb: f64,
//     pub total_monitoring_seconds: f64,
//     pub sample_count: i64,

//     // A–F comparative rating
//     pub rating: Option<String>,
//     pub last_updated: SystemTime,
// }
