mod config;
mod stats;
mod engine;

use std::{fs, sync::{Arc, Mutex}, sync::atomic::{AtomicU64, Ordering}, time::{Duration, Instant}};
use tokio::time::{sleep, timeout};
use config::TestConfig;
use stats::{GlobalStats};

#[tokio::main]
async fn main() {
    // 1. Load Configuration
    let yaml_content = fs::read_to_string("test.yaml").expect("File test.yaml not found");
    let config: TestConfig = serde_yaml::from_str(&yaml_content).expect("Invalid YAML format");
    
    // 2. Initialize Shared State
    let stats = Arc::new(Mutex::new(GlobalStats {
        endpoints: std::collections::HashMap::new(),
        start_time: Instant::now(),
        active_users: 0,
    }));

    let total_runtime = config.runtime;
    let start_instant = stats.lock().unwrap().start_time;

    // 3. Start the Reporter Task (from stats.rs)
    stats::spawn_reporter(Arc::clone(&stats), total_runtime);

    // 4. Prepare User Spawning Logic
    let mut handles = vec![];
    // Calculate delay between spawning users to respect rampup
    let spawn_delay_ms = (config.rampup as f64 * 1000.0) / (config.users as f64).max(1.0);
    
    // Global Atomic counter for TPS pacing across all threads
    let global_ticker = Arc::new(AtomicU64::new(0));

    for i in 0..config.users {
        let cfg = config.clone();
        let stats_clone = Arc::clone(&stats);
        let ticker_clone = Arc::clone(&global_ticker);

        let handle = tokio::spawn(async move {
            // Precise Ramp-up: staggering user start times
            let my_delay = Duration::from_millis((i as f64 * spawn_delay_ms) as u64);
            sleep(my_delay).await;

            { stats_clone.lock().unwrap().active_users += 1; }

            // One client per user (reuses connections internally)
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();

            while start_instant.elapsed().as_secs() < cfg.runtime {
                for step_map in &cfg.steps {
                    for (method, details) in step_map {
                        
                        // --- GLOBAL TPS PACING (Linear Ramp Support) ---
                        if let Some(peak) = cfg.peak_tps {
                            loop {
                                let elapsed = start_instant.elapsed().as_secs_f64();
                                if elapsed >= cfg.runtime as f64 { break; }

                                // Calculate the "Target Area" under the TPS curve
                                let expected_reqs = if elapsed < cfg.rampup as f64 {
                                    (elapsed.powi(2) / (2.0 * cfg.rampup as f64)) * peak as f64
                                } else {
                                    let ramp_area = (cfg.rampup as f64 * peak as f64) / 2.0;
                                    let constant_area = (elapsed - cfg.rampup as f64) * peak as f64;
                                    ramp_area + constant_area
                                };

                                let current = ticker_clone.load(Ordering::Relaxed);
                                if (current as f64) < expected_reqs {
                                    if ticker_clone.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                                        break; 
                                    }
                                }
                                // Back-off slightly to reduce CPU spinning
                                sleep(Duration::from_millis(15)).await;
                            }
                        }

                        let endpoint = details.get("endpoint").and_then(|v| v.as_str()).unwrap_or("/");
                        let method_upper = method.to_uppercase();
                        
                        // --- MEASURE LATENCY ---
                        let req_start = Instant::now();
                        
                        let is_success = engine::execute_request(
                            &client, 
                            &cfg.host, 
                            &method_upper, 
                            endpoint, 
                            details, 
                            &cfg.common_headers
                        ).await;

                        let duration = req_start.elapsed();

                        // --- UPDATE STATS ---
                        {
                            let mut s = stats_clone.lock().unwrap();
                            let entry = s.endpoints.entry(format!("{} {}", method_upper, endpoint)).or_default();
                            
                            entry.requests += 1;
                            if is_success { entry.success += 1; } else { entry.failure += 1; }
                            
                            // Latency aggregation
                            entry.total_response_time += duration;
                            if duration < entry.min_response_time { entry.min_response_time = duration; }
                            if duration > entry.max_response_time { entry.max_response_time = duration; }
                        }

                        // Fallback sleep if no global pacing is defined
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

    // 5. Hard Runtime Cutoff
    // We wait for all handles to finish, but force exit after runtime + 1s buffer
    // let _ = timeout(Duration::from_secs(total_runtime + 1), async {
    //     for h in handles { let _ = h.await; }
    // }).await;
    let _ = timeout(Duration::from_secs(total_runtime + 1), async {
        for h in handles { let _ = h.await; }
    }).await;

    stats::save_to_csv(Arc::clone(&stats), &config.testname);

    println!("\n🏁 Test Finished.");
}