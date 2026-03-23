//! deutche bahn (sqlite)

use rusqlite::Connection;
use tokio::sync::mpsc::Receiver;

use crate::schema::EnergySample;


pub async fn writer_task(mut rx: Receiver<EnergySample>, db_path: &str) {
    let conn = Connection::open(db_path).unwrap();
    
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        PRAGMA cache_size=-8000; 
    ").unwrap();

    // todo: write a new instance of MonitoringSession to the db
    
    let mut buffer: Vec<EnergySample> = Vec::with_capacity(12); // ~60s at 5s intervals

    for sample in rx.recv().await {
        buffer.push(sample);
        if buffer.len() >= 12 {
            flush(&conn, &mut buffer);
        }
    }
    
    // flush remainder on shutdown
    flush(&conn, &mut buffer);
    // todo: update current MonitoringSession with an end time
}

fn flush(conn: &Connection, buffer: &mut Vec<EnergySample>) {
    let tx = conn.unchecked_transaction().unwrap();
    for s in buffer.drain(..) {
        // write EnergySample to database
        // todo
    }
    tx.commit().unwrap();
}