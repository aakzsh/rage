use serde::Deserialize;
use std::{collections::HashMap, fs, sync::{Arc, Mutex}, sync::atomic::{AtomicU64, Ordering}, time::{Duration, Instant}};
use tokio::time::{sleep, timeout};
use comfy_table::{Table, presets::UTF8_FULL, Cell, Color, Attribute};

#[derive(Debug, Deserialize, Clone)]
struct TestConfig {
    host: String,
    users: u32,
    rampup: u64,
    runtime: u64,
    sleep: Option<u64>,
    #[serde(rename = "peak-tps")]
    peak_tps: Option<u32>,
    steps: Vec<HashMap<String, serde_yaml::Value>>,
}

#[derive(Default, Clone)]
struct EndpointStats {
    requests: u64,
    success: u64,
    failure: u64,
}

struct GlobalStats {
    endpoints: HashMap<String, EndpointStats>,
    start_time: Instant,
    active_users: u32,
}

#[tokio::main]
async fn main() {
    let yaml_content = fs::read_to_string("test.yaml").expect("File not found");
    let config: TestConfig = serde_yaml::from_str(&yaml_content).expect("Invalid YAML");
    
    let stats = Arc::new(Mutex::new(GlobalStats {
        endpoints: HashMap::new(),
        start_time: Instant::now(),
        active_users: 0,
    }));

    let total_runtime = config.runtime;
    let start_instant = stats.lock().unwrap().start_time;
    let stats_for_reporter = Arc::clone(&stats);

    // 1. REPORTER TASK
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let s = stats_for_reporter.lock().unwrap();
            let elapsed = s.start_time.elapsed().as_secs();
            
            print!("{esc}c", esc = 27 as char);
            let mut table = Table::new();
            table.load_preset(UTF8_FULL).set_header(vec!["Method", "Endpoint", "# Req", "# Success", "# Fail", "Avg TPS"]);

            let mut total_req = 0;
            let mut total_success = 0;
            let mut total_fail = 0;

            for (name, estats) in &s.endpoints {
                let tps = if elapsed > 0 { estats.requests as f64 / elapsed as f64 } else { 0.0 };
                let parts: Vec<&str> = name.splitn(2, ' ').collect();
                total_req += estats.requests;
                total_success += estats.success;
                total_fail += estats.failure;

                table.add_row(vec![
                    parts.get(0).unwrap_or(&"-").to_string(), parts.get(1).unwrap_or(&"-").to_string(),
                    estats.requests.to_string(), estats.success.to_string(), estats.failure.to_string(),
                    format!("{:.2}", tps),
                ]);
            }

            let total_tps = if elapsed > 0 { total_req as f64 / elapsed as f64 } else { 0.0 };
            table.add_row(vec![
                Cell::new("TOTAL").add_attribute(Attribute::Bold).fg(Color::Cyan),
                Cell::new("All Endpoints").add_attribute(Attribute::Italic),
                Cell::new(total_req).add_attribute(Attribute::Bold),
                Cell::new(total_success).fg(Color::Green),
                Cell::new(total_fail).fg(Color::Red),
                Cell::new(format!("{:.2}", total_tps)).add_attribute(Attribute::Bold).fg(Color::Yellow),
            ]);

            println!("🚀 Global Load Test: {}s / {}s | Active Users: {}", elapsed, total_runtime, s.active_users);
            println!("{}", table);
            if elapsed >= total_runtime { break; }
        }
    });

    // 2. USER EXECUTION LOGIC
    let mut handles = vec![];
    let spawn_delay_ms = (config.rampup as f64 * 1000.0) / (config.users as f64).max(1.0);

    // FIX: Using AtomicU64 instead of Mutex for the ticker
    let global_ticker = Arc::new(AtomicU64::new(0));

    for i in 0..config.users {
        let cfg = config.clone();
        let stats_clone = Arc::clone(&stats);
        let ticker_clone = Arc::clone(&global_ticker);

        let handle = tokio::spawn(async move {
            let my_delay = Duration::from_millis((i as f64 * spawn_delay_ms) as u64);
            sleep(my_delay).await;

            { stats_clone.lock().unwrap().active_users += 1; }

            let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap();

            while start_instant.elapsed().as_secs() < cfg.runtime {
                for step_map in &cfg.steps {
                    for (method, details) in step_map {
                        
                        // --- ACCURATE GLOBAL TPS PACING ---
                        if let Some(peak) = cfg.peak_tps {
                            loop {
                                let elapsed = start_instant.elapsed().as_secs_f64();
                                if elapsed >= cfg.runtime as f64 { break; }

                                let expected_reqs = if elapsed < cfg.rampup as f64 {
                                    (elapsed.powi(2) / (2.0 * cfg.rampup as f64)) * peak as f64
                                } else {
                                    let ramp_area = (cfg.rampup as f64 * peak as f64) / 2.0;
                                    let constant_area = (elapsed - cfg.rampup as f64) * peak as f64;
                                    ramp_area + constant_area
                                };

                                // Atomic check (No MutexGuard held across .await)
                                let current = ticker_clone.load(Ordering::Relaxed);
                                if (current as f64) < expected_reqs {
                                    // Try to increment. If someone else beat us to it, loop again.
                                    if ticker_clone.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                                        break; 
                                    }
                                }
                                sleep(Duration::from_millis(10)).await;
                            }
                        }

                        let endpoint = details.get("endpoint").and_then(|v| v.as_str()).unwrap_or("/");
                        let method_upper = method.to_uppercase();
                        let is_success = execute_request(&client, &cfg.host, &method_upper, endpoint, details).await;

                        {
                            let mut s = stats_clone.lock().unwrap();
                            let entry = s.endpoints.entry(format!("{} {}", method_upper, endpoint)).or_default();
                            entry.requests += 1;
                            if is_success { entry.success += 1; } else { entry.failure += 1; }
                        }

                        if cfg.peak_tps.is_none() {
                            if let Some(sl) = cfg.sleep { sleep(Duration::from_secs(sl)).await; }
                        }
                    }
                }
            }
            { stats_clone.lock().unwrap().active_users -= 1; }
        });
        handles.push(handle);
    }

    let _ = timeout(Duration::from_secs(total_runtime + 1), async {
        for h in handles { let _ = h.await; }
    }).await;

    println!("\n🏁 Test Finished.");
}

async fn execute_request(client: &reqwest::Client, host: &str, method: &str, endpoint: &str, details: &serde_yaml::Value) -> bool {
    let url = format!("{}{}", host.trim_end_matches('/'), endpoint);
    let rb = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url).json(&details.get("body").unwrap_or(&serde_yaml::Value::Null)),
        _ => return false,
    };
    rb.send().await.map(|r| r.status().is_success()).unwrap_or(false)
}