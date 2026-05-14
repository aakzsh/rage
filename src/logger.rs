use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use tokio::sync::mpsc;

/// The structure for our JSONL log entries.
/// event_type "metric" = periodic system health snapshots
/// event_type "request" = sampled individual API call results
#[derive(Serialize)]
pub struct LogEvent {
    pub timestamp: u64,
    pub event_type: String,
    pub endpoint: Option<String>,
    pub response_time_ms: Option<u128>,
    pub status: Option<bool>,
    pub tps: f64,
    pub cpu_usage: f32,
    pub mem_mb: u64,
    pub active_users: u32, // <--- Add this field
}

/// Spawns a background task that listens for log events and writes them to disk.
/// Returns a sender handle that can be cloned and passed to workers.
pub fn spawn_logger(test_name: String) -> mpsc::UnboundedSender<LogEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<LogEvent>();
    let filename = format!("reports/{}_log.jsonl", test_name);

    tokio::spawn(async move {
        // Open file in append mode, create if it doesn't exist
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&filename)
            .expect("Failed to create log file");

        println!("📝 Logger initialized. Writing to {}", filename);

        while let Some(event) = rx.recv().await {
            // Serialize the event to a single line of JSON
            if let Ok(json) = serde_json::to_string(&event) {
                // We use writeln! to ensure each entry is on its own line
                if let Err(e) = writeln!(file, "{}", json) {
                    eprintln!("Failed to write to log file: {}", e);
                }
            }
        }
    });

    tx
}