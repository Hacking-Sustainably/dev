//! the child process for data collection.
//! this module is a wrapper around the system-specific implementation

use tokio::sync::mpsc::Sender;

use crate::schema::EnergySample;

mod macos;


pub async fn metrics_task(tx: Sender<EnergySample>) {
    #[cfg(target_os = "macos")]
    macos::spawn_powermetrics(tx).await;
    
    #[cfg(target_os = "linux")]
    panic!("linux not supported yet");
    
    #[cfg(target_os = "windows")]
    panic!("windows not supported yet");
}