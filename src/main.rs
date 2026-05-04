mod config;
mod stats;
mod engine;
mod logger;

use std::{fs, sync::{Arc, Mutex}, sync::atomic::{AtomicU64, Ordering}, time::{Duration, Instant}};
use tokio::time::{sleep, timeout};
use config::TestConfig;
use stats::GlobalStats;

fn main() {
    // 1. Load config synchronously to read core settings
    let yaml_content = fs::read_to_string("test.yaml").expect("File not found");
    let config: TestConfig = serde_yaml::from_str(&yaml_content).expect("Invalid YAML");

    // 2. Determine Core Count
    let available_cores = num_cpus::get();
    let target_cores = match config.max_cores {
        -1 => available_cores,
        n if n <= 0 => {
            println!("⚠️  Invalid max_cores ({}). Using all cores ({}).", n, available_cores);
            available_cores
        }
        n if n as usize > available_cores => {
            println!("⚠️  Requested cores ({}) > hardware ({}). Capping at {}.", n, available_cores, available_cores);
            available_cores
        }
        n => n as usize,
    };

    // 3. Build Runtime with requested Cores
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(target_cores)
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime");

    println!("🚀 Engine initialized on {} worker threads.", target_cores);

    // 4. Block on the async engine - passing target_cores into the async context
    runtime.block_on(run_engine(config, target_cores));
}

async fn run_engine(config: TestConfig, target_cores: usize) {
    // --- SETUP SHARED STATE ---
    let stats = Arc::new(Mutex::new(GlobalStats {
        endpoints: std::collections::HashMap::new(),
        start_time: Instant::now(),
        active_users: 0,
    }));

    let total_runtime = config.runtime;
    let start_instant = stats.lock().unwrap().start_time;

    // --- INITIALIZE LOGGER ---
    let log_tx = logger::spawn_logger(config.testname.clone());

    // --- SHARED CLIENT (High Performance) ---
    // Using one client wrapped in Arc so all users share the same connection pool
    let client = Arc::new(reqwest::Client::builder()
        .tcp_nodelay(true)
        .pool_max_idle_per_host(2000) 
        .tcp_keepalive(Duration::from_secs(90))
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap());

    // --- START REPORTER ---
    stats::spawn_reporter(
        Arc::clone(&stats), 
        total_runtime, 
        log_tx.clone(), 
        config.users,
        target_cores
    );

    // --- USER SPAWNING ---
    let mut handles = vec![];
    let spawn_delay_ms = (config.rampup as f64 * 1000.0) / (config.users as f64).max(1.0);
    let global_ticker = Arc::new(AtomicU64::new(0));

    for i in 0..config.users {
        let cfg = config.clone();
        let stats_clone = Arc::clone(&stats);
        let ticker_clone = Arc::clone(&global_ticker);
        let client_ref = Arc::clone(&client);
        let worker_log_tx = log_tx.clone();

        let handle = tokio::spawn(async move {
            // Ramp-up delay
            let my_delay = Duration::from_millis((i as f64 * spawn_delay_ms) as u64);
            if !my_delay.is_zero() { sleep(my_delay).await; }

            { stats_clone.lock().unwrap().active_users += 1; }
            let mut req_counter = 0;

            while start_instant.elapsed().as_secs() < cfg.runtime {
                for step_map in &cfg.steps {
                    for (method, details) in step_map {
                        
                        // --- GLOBAL TPS PACING ---
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

                                let current = ticker_clone.load(Ordering::Relaxed);
                                if (current as f64) < expected_reqs {
                                    if ticker_clone.compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                                        break; 
                                    }
                                }
                                
                                // Yield or sleep depending on how far ahead we are
                                if (current as f64) - expected_reqs < 5.0 {
                                    tokio::task::yield_now().await;
                                } else {
                                    sleep(Duration::from_millis(1)).await;
                                }
                            }
                        }

                        let endpoint = details.get("endpoint").and_then(|v| v.as_str()).unwrap_or("/");
                        let method_upper = method.to_uppercase();
                        let req_start = Instant::now();
                        
                        // --- EXECUTE REQUEST ---
                        // Fixed type casting for engine call
                        let is_success = engine::execute_request(
                            &client_ref, 
                            &cfg.host, 
                            &method_upper, 
                            endpoint, 
                            &serde_yaml::to_value(details).unwrap(), 
                            &Some(cfg.common_headers.clone())
                        ).await;

                        let duration = req_start.elapsed();
                        req_counter += 1;

                        // --- SAMPLED REQUEST LOGGING (1%) ---
                        // This sends the individual request data to the JSONL logger
                        if req_counter % 100 == 0 {
                            let _ = worker_log_tx.send(logger::LogEvent {
                                timestamp: start_instant.elapsed().as_secs(),
                                event_type: "request".to_string(),
                                endpoint: Some(endpoint.to_string()),
                                response_time_ms: Some(duration.as_millis()),
                                status: Some(is_success),
                                tps: 0.0,
                                cpu_usage: 0.0,
                                mem_mb: 0,
                            });
                        }

                        // --- UPDATE GLOBAL STATS ---
                        {
                            let mut s = stats_clone.lock().unwrap();
                            let entry = s.endpoints.entry(format!("{} {}", method_upper, endpoint)).or_default();
                            
                            entry.requests += 1;
                            if is_success { entry.success += 1; } else { entry.failure += 1; }
                            
                            entry.total_response_time += duration;
                            if duration < entry.min_response_time { entry.min_response_time = duration; }
                            if duration > entry.max_response_time { entry.max_response_time = duration; }
                        }

                        // Sleep if no global pacing is defined
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

    // 5. Cleanup and wait for finish
    let _ = timeout(Duration::from_secs(total_runtime + 2), async {
        for h in handles { let _ = h.await; }
    }).await;

    stats::save_to_csv(Arc::clone(&stats), &config.testname);
    println!("\n🏁 Load Test Completed.");
}