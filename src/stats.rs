use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};
use tokio::time::sleep;
use tokio::sync::mpsc;
use comfy_table::{Table, presets::UTF8_FULL, Cell, Color, Attribute};
use std::fs::File;
use std::io::Write;
use sysinfo::{System, Networks}; 
use crate::logger::LogEvent;

#[derive(Clone)]
pub struct EndpointStats {
    pub requests: u64,
    pub success: u64,
    pub failure: u64,
    pub total_response_time: Duration,
    pub min_response_time: Duration,
    pub max_response_time: Duration,
}

impl Default for EndpointStats {
    fn default() -> Self {
        Self {
            requests: 0,
            success: 0,
            failure: 0,
            total_response_time: Duration::from_secs(0),
            min_response_time: Duration::from_secs(999),
            max_response_time: Duration::from_secs(0),
        }
    }
}

pub struct GlobalStats {
    pub endpoints: HashMap<String, EndpointStats>,
    pub start_time: Instant,
    pub active_users: u32,
}

pub fn spawn_reporter(
    stats: Arc<Mutex<GlobalStats>>, 
    total_runtime: u64,
    log_tx: mpsc::UnboundedSender<LogEvent>,
    max_users: u32,
    allocated_cores: usize, 
) {
    let stats_clone = Arc::clone(&stats);
    let mut user_history: Vec<u32> = Vec::new();
    let graph_width = 70; 

    tokio::spawn(async move {
        let mut sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();
        let total_hardware_cores = sys.cpus().len();

        loop {
            sleep(Duration::from_secs(2)).await;
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            networks.refresh();
            
            let s = stats_clone.lock().unwrap();
            let elapsed = s.start_time.elapsed().as_secs();
            let active_users = s.active_users;
            
            user_history.push(active_users);
            if user_history.len() > graph_width {
                user_history.remove(0);
            }

            let cpu_usage = sys.global_cpu_info().cpu_usage();
            let total_mem = sys.total_memory() / 1024 / 1024; 
            let used_mem = sys.used_memory() / 1024 / 1024;   
            let mem_pct = if total_mem > 0 { (used_mem as f64 / total_mem as f64) * 100.0 } else { 0.0 };

            let mut total_rx_bytes = 0;
            let mut total_tx_bytes = 0;
            for (_interface_name, network) in &networks {
                total_rx_bytes += network.received();
                total_tx_bytes += network.transmitted();
            }
            let rx_kbps = (total_rx_bytes as f64 / 1024.0) / 2.0;
            let tx_kbps = (total_tx_bytes as f64 / 1024.0) / 2.0;

            let total_reqs: u64 = s.endpoints.values().map(|e| e.requests).sum();
            let current_tps = if elapsed > 0 { total_reqs as f64 / elapsed as f64 } else { 0.0 };
            
            // Fixed LogEvent by explicitly setting all fields to avoid Default error
            let _ = log_tx.send(LogEvent {
                timestamp: elapsed,
                event_type: "metric".to_string(),
                endpoint: None,
                response_time_ms: None,
                status: None,
                tps: current_tps,
                cpu_usage,
                mem_mb: used_mem,
            });

            print!("{esc}c", esc = 27 as char); 
            
            println!("🚀 Global Load Test: {}s / {}s | Active Users: {}", elapsed, total_runtime, active_users);
            println!(
                "💻 CPU: {:.1}% ({}/{} Cores) | MEM: {}/{} MB ({:.2}%)",
                cpu_usage, allocated_cores, total_hardware_cores, used_mem, total_mem, mem_pct
            );
            println!("🌐 NET: ↓ {:.2} KB/s  ↑ {:.2} KB/s", rx_kbps, tx_kbps);

            let mut table = Table::new();
            table.load_preset(UTF8_FULL).set_header(vec![
                "Method", "Endpoint", "# Req", "Error %", "TPS", "Avg (ms)", "Min (ms)", "Max (ms)"
            ]);

            let mut total_req = 0;
            let mut total_fail = 0;
            let mut total_time = Duration::from_secs(0);

            for (name, estats) in &s.endpoints {
                let tps = if elapsed > 0 { estats.requests as f64 / elapsed as f64 } else { 0.0 };
                let err_pct = if estats.requests > 0 { (estats.failure as f64 / estats.requests as f64) * 100.0 } else { 0.0 };
                let avg_lat = if estats.requests > 0 { estats.total_response_time.as_millis() as f64 / estats.requests as f64 } else { 0.0 };
                
                total_req += estats.requests;
                total_fail += estats.failure;
                total_time += estats.total_response_time;

                let parts: Vec<&str> = name.splitn(2, ' ').collect();
                table.add_row(vec![
                    parts.get(0).unwrap_or(&"-").to_string(),
                    parts.get(1).unwrap_or(&"-").to_string(),
                    estats.requests.to_string(),
                    format!("{:.1}%", err_pct),
                    format!("{:.2}", tps),
                    format!("{:.1}", avg_lat),
                    format!("{}", if estats.requests > 0 { estats.min_response_time.as_millis() } else { 0 }),
                    format!("{}", estats.max_response_time.as_millis()),
                ]);
            }

            let total_tps = if elapsed > 0 { total_req as f64 / elapsed as f64 } else { 0.0 };
            let total_err_pct = if total_req > 0 { (total_fail as f64 / total_req as f64) * 100.0 } else { 0.0 };
            let total_avg_lat = if total_req > 0 { total_time.as_millis() as f64 / total_req as f64 } else { 0.0 };

            table.add_row(vec![
                Cell::new("TOTAL").add_attribute(Attribute::Bold).fg(Color::Cyan),
                Cell::new("All").add_attribute(Attribute::Italic),
                Cell::new(total_req).add_attribute(Attribute::Bold),
                Cell::new(format!("{:.1}%", total_err_pct)).fg(if total_err_pct > 0.0 { Color::Red } else { Color::Green }),
                Cell::new(format!("{:.2}", total_tps)).fg(Color::Yellow),
                Cell::new(format!("{:.1}", total_avg_lat)).add_attribute(Attribute::Bold),
                Cell::new("-"),
                Cell::new("-"),
            ]);

            println!("{}", table);

            println!("\n📈 User Concurrency Trend");
            let graph_height = 8;
            for r in (0..graph_height).rev() {
                let label_val = (max_users as f32 * (r as f32 / (graph_height - 1) as f32)) as u32;
                let mut line = format!("{:>6} │", label_val);

                for &val in &user_history {
                    let normalized = (val as f32 / max_users as f32) * graph_height as f32;
                    let diff = normalized - r as f32;

                    if diff >= 0.75 {
                        line.push('█'); 
                    } else if diff >= 0.5 {
                        line.push('▆'); 
                    } else if diff >= 0.25 {
                        line.push('▄'); 
                    } else {
                        line.push(' ');
                    }
                }
                println!("{}", line);
            }
            println!("       └{}", "─".repeat(user_history.len()));
            println!("       {:>width$}", "Time (Recent Snapshots) →", width = user_history.len());
            
            if elapsed >= total_runtime { break; }
        }
    });
}

pub fn save_to_csv(stats: Arc<Mutex<GlobalStats>>, test_name: &str) {
    let s = stats.lock().unwrap();
    let elapsed = s.start_time.elapsed().as_secs_f64();
    let filename = format!("{}.csv", test_name);
    let mut file = File::create(&filename).expect("Could not create CSV file");

    writeln!(file, "Method,Endpoint,# Requests,Success,Failure,Error %,TPS,Avg Latency (ms),Min Latency (ms),Max Latency (ms)").unwrap();

    for (name, estats) in &s.endpoints {
        let tps = if elapsed > 0.0 { estats.requests as f64 / elapsed } else { 0.0 };
        let err_pct = if estats.requests > 0 { (estats.failure as f64 / estats.requests as f64) * 100.0 } else { 0.0 };
        let avg_lat = if estats.requests > 0 { estats.total_response_time.as_millis() as f64 / estats.requests as f64 } else { 0.0 };
        
        let parts: Vec<&str> = name.splitn(2, ' ').collect();
        let method = parts.get(0).unwrap_or(&"-");
        let endpoint = parts.get(1).unwrap_or(&"-");

        writeln!(
            file,
            "{},{},{},{},{},{:.2}%,{:.2},{:.2},{},{}",
            method, endpoint, estats.requests, estats.success, estats.failure, 
            err_pct, tps, avg_lat,
            if estats.requests > 0 { estats.min_response_time.as_millis() } else { 0 },
            estats.max_response_time.as_millis()
        ).unwrap();
    }
}