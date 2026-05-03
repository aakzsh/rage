use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};
use tokio::time::sleep;
use comfy_table::{Table, presets::UTF8_FULL, Cell, Color, Attribute};
use std::fs::File;
use std::io::Write;

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
            min_response_time: Duration::from_secs(999), // High default for min
            max_response_time: Duration::from_secs(0),
        }
    }
}

pub struct GlobalStats {
    pub endpoints: HashMap<String, EndpointStats>,
    pub start_time: Instant,
    pub active_users: u32,
}

pub fn save_to_csv(stats: Arc<Mutex<GlobalStats>>, test_name: &str) {
    let s = stats.lock().unwrap();
    let elapsed = s.start_time.elapsed().as_secs_f64();
    let filename = format!("{}.csv", test_name);
    let mut file = File::create(&filename).expect("Could not create CSV file");

    // Header
    writeln!(file, "Method,Endpoint,# Requests,Error %,TPS,Avg (ms),Min (ms),Max (ms)").unwrap();

    for (name, estats) in &s.endpoints {
        let tps = if elapsed > 0.0 { estats.requests as f64 / elapsed } else { 0.0 };
        let err_pct = if estats.requests > 0 { (estats.failure as f64 / estats.requests as f64) * 100.0 } else { 0.0 };
        let avg_lat = if estats.requests > 0 { estats.total_response_time.as_millis() as f64 / estats.requests as f64 } else { 0.0 };
        
        let parts: Vec<&str> = name.splitn(2, ' ').collect();
        let method = parts.get(0).unwrap_or(&"-");
        let endpoint = parts.get(1).unwrap_or(&"-");

        writeln!(
            file,
            "{},{},{},{:.2}%,{:.2},{:.2},{},{}",
            method, endpoint, estats.requests, err_pct, tps, avg_lat,
            estats.min_response_time.as_millis(),
            estats.max_response_time.as_millis()
        ).unwrap();
    }
    println!("📊 Final results saved to {}", filename);
}

pub fn spawn_reporter(stats: Arc<Mutex<GlobalStats>>, total_runtime: u64) {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let s = stats.lock().unwrap();
            let elapsed = s.start_time.elapsed().as_secs();
            
            print!("{esc}c", esc = 27 as char);
            let mut table = Table::new();
            table.load_preset(UTF8_FULL).set_header(vec![
                "Method", "Endpoint", "# Req", "Error %", "TPS", "Avg (ms)", "Min (ms)", "Max (ms)"
            ]);

            let mut total_req = 0;
            // let mut total_success = 0;
            let mut total_fail = 0;
            let mut total_time = Duration::from_secs(0);

            for (name, estats) in &s.endpoints {
                let tps = if elapsed > 0 { estats.requests as f64 / elapsed as f64 } else { 0.0 };
                let err_pct = if estats.requests > 0 { (estats.failure as f64 / estats.requests as f64) * 100.0 } else { 0.0 };
                let avg_lat = if estats.requests > 0 { estats.total_response_time.as_millis() as f64 / estats.requests as f64 } else { 0.0 };
                
                total_req += estats.requests;
                // total_success += estats.success;
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
                    format!("{}", estats.min_response_time.as_millis()),
                    format!("{}", estats.max_response_time.as_millis()),
                ]);
            }

            // TOTALS CALCULATION
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

            println!("🚀 Global Load Test: {}s / {}s | Active Users: {}", elapsed, total_runtime, s.active_users);
            println!("{}", table);
            if elapsed >= total_runtime { break; }
        }
    });
}