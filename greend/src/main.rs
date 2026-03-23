//! greend daemon for greenb.
//! periodically samples system wide energy usage,
//! writes it to the greenb database for later processing 

pub mod subprocess;
pub mod db;
pub mod schema;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // todo:
    // - create spsc for EnergySample(s)
    // - spawn writer task with consumer
    // - spawn metrics task with producer
    
    // metrics task:
    // - spawns powermetrics as subprocess
    // - keep reading output until '\0xc' or </plist> (?)
    // - use serde and plist to deserialise output into PowermetricsSample
    // - convert PowermetricsSample into EnergySample
    // - send over spsc channel
    
    // writer task:
    // - on spawn, write a new MonitoringSession to the database
    // - read from channel
    // - buffer EnergySamples
    // - flush to database
    // - on quit, update end time in MonitoringSession
    
    // signal handler:
    // - process SIGTERM
    // - flush database and update MonitoringSession
}
