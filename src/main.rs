// src/main.rs
mod config;
mod stats;
mod engine;
mod logger;
mod report;
mod report_modern;
mod utils; 

use std::{fs, sync::{Arc, Mutex}, sync::atomic::{AtomicU64, Ordering}, time::{Duration, Instant}};
use tokio::time::{sleep};
use tokio_util::sync::CancellationToken; 
use config::TestConfig;
use stats::GlobalStats;
use rlimit::{getrlimit, setrlimit, Resource};

fn tune_system_limits() -> Result<(), Box<dyn std::error::Error>> {
    let (soft, hard) = getrlimit(Resource::NOFILE)?;
    println!("Current ulimit -n: soft={}, hard={}", soft, hard);

    let target_limit = 65536;
    if soft < target_limit {
        let new_soft = target_limit.min(hard);
        setrlimit(Resource::NOFILE, new_soft, hard)?;
        println!("🚀 ulimit -n updated to {}", new_soft);
    }
    Ok(())
}

fn main() {
    let yaml_content = fs::read_to_string("test.yaml").expect("File not found");
    let config: TestConfig = serde_yaml::from_str(&yaml_content).expect("Invalid YAML");

    if let Err(e) = tune_system_limits() {
        eprintln!("⚠️ Warning: Could not increase ulimit: {}", e);
    }

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

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(target_cores)
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime");

    println!("🚀 Engine initialized on {} worker threads.", target_cores);
    runtime.block_on(run_engine(config, target_cores));
}

async fn run_engine(config: TestConfig, target_cores: usize) {
    let cancel_token = CancellationToken::new();
    let stats = Arc::new(Mutex::new(GlobalStats {
        endpoints: std::collections::HashMap::new(),
        start_time: Instant::now(),
        active_users: 0,
    }));

    let total_runtime = config.runtime;
    let start_instant = stats.lock().unwrap().start_time;
    let log_tx = logger::spawn_logger(config.testname.clone());

    let client = Arc::new(reqwest::Client::builder()
        .tcp_nodelay(true)
        .pool_max_idle_per_host(5000) 
        .tcp_keepalive(Duration::from_secs(90))
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap());

    stats::spawn_reporter(
        Arc::clone(&stats), 
        total_runtime, 
        log_tx.clone(), 
        config.users,
        target_cores
    );

    // --- CSV BOOT LOADING PHASE ---
    let csv_container = if config.csv_config != "none" {
        println!("📂 Parsing data targets from source file: {}...", config.csv_config);
        utils::CsvDataCache::load(&config.csv_config).map(Arc::new)
    } else {
        None
    };

    let mut handles = vec![];
    let spawn_delay_ms = (config.rampup as f64 * 1000.0) / (config.users as f64).max(1.0);
    let global_ticker = Arc::new(AtomicU64::new(0));
    
    // Shared row sequence cursor distributed across worker threads
    let global_csv_row_counter = Arc::new(AtomicU64::new(0));

    // --- WORKER SPAWNING ---
    for i in 0..config.users {
        let cfg = config.clone();
        let stats_clone = Arc::clone(&stats);
        let ticker_clone = Arc::clone(&global_ticker);
        let client_ref = Arc::clone(&client);
        let worker_log_tx = log_tx.clone();
        let token = cancel_token.clone();
        
        let csv_cache_ref = csv_container.clone();
        let csv_counter_ref = Arc::clone(&global_csv_row_counter);

        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => return,
                _ = sleep(Duration::from_millis((i as f64 * spawn_delay_ms) as u64)) => {}
            }
        
            { stats_clone.lock().unwrap().active_users += 1; }
        
            let global_elapsed = start_instant.elapsed();
            let global_remaining = Duration::from_secs(cfg.runtime).saturating_sub(global_elapsed);
            
            let time_limit = if let Some(limit_secs) = cfg.session_duration {
                std::cmp::min(global_remaining, Duration::from_secs(limit_secs))
            } else {
                global_remaining
            };
        
            let mut user_context: std::collections::HashMap<String, String> = std::collections::HashMap::with_capacity(4);
            
            let worker_logic = async {
                let mut req_counter = 0;
                loop {
                    if token.is_cancelled() { break; }

                    for step_map in &cfg.steps {
                        for (method, details) in step_map {
                            
                            // --- INTEGRAL-BASED TRAPEZOIDAL PACING ---
                            if let Some(peak) = cfg.peak_tps {
                                let peak = peak as f64;
                                let ramp = cfg.rampup as f64;
                                let run = cfg.runtime as f64;

                                loop {
                                    if token.is_cancelled() { break; }
                                    let now = start_instant.elapsed().as_secs_f64();
                                    
                                    let expected_total = if now < ramp {
                                        0.5 * now * (now / ramp * peak)
                                    } else if now < (run - ramp) {
                                        let ramp_area = 0.5 * ramp * peak;
                                        let plateau_area = (now - ramp) * peak;
                                        ramp_area + plateau_area
                                    } else {
                                        let ramp_area = 0.5 * ramp * peak;
                                        let plateau_duration = run - (2.0 * ramp);
                                        let plateau_area = plateau_duration * peak;
                                        let time_in_down = (now - (run - ramp)).min(ramp);
                                        let current_down_speed = (1.0 - (time_in_down / ramp)) * peak;
                                        let ramp_down_area = 0.5 * (peak + current_down_speed) * time_in_down;
                                        ramp_area + plateau_area + ramp_down_area
                                    };

                                    let current_total = ticker_clone.load(Ordering::Relaxed);
                                    if (current_total as f64) < expected_total {
                                        if ticker_clone.compare_exchange(current_total, current_total + 1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                                            break; 
                                        }
                                    } else {
                                        sleep(Duration::from_millis(1)).await;
                                    }
                                    if now >= run { break; }
                                }
                            }
        
                            // Fetch an atomic row sequence index for this request step execution
                            let my_request_row = csv_counter_ref.fetch_add(1, Ordering::Relaxed) as usize;

                            // --- 1. RESOLVE THE ENDPOINT WITH DYNAMIC EVALUATION ---
                            let raw_endpoint = details.get("endpoint").and_then(|v| v.as_str()).unwrap_or("/");
                            let resolved_endpoint = crate::utils::resolve_variables(
                                raw_endpoint, 
                                &user_context,
                                &csv_cache_ref,
                                my_request_row
                            ).into_owned();
                            let method_upper = method.to_uppercase();

                            // --- 2. COMPILE HEADERS MATRIX AND SWAP CACHED TOKENS ---
                            let mut merged_headers = std::collections::HashMap::new();
                            for (k, v) in &cfg.common_headers {
                                merged_headers.insert(k.clone(), v.clone());
                            }

                            if let Some(step_headers_val) = details.get("headers") {
                                if let Some(step_headers) = step_headers_val.as_mapping() {
                                    for (k, v) in step_headers {
                                        if let (Some(k_str), Some(v_str)) = (k.as_str(), v.as_str()) {
                                            let resolved_val = crate::utils::resolve_variables(
                                                v_str, 
                                                &user_context,
                                                &csv_cache_ref,
                                                my_request_row
                                            ).into_owned();
                                            merged_headers.insert(k_str.to_string(), resolved_val);
                                        }
                                    }
                                }
                            }

                            // --- 3. EXPORT AND TRANSLATE PAYLOAD STRINGS FOR INLINE PARSING ---
                            let raw_body_yaml = details.get("body")
                                .map(|v| serde_yaml::to_string(v).unwrap_or_default())
                                .unwrap_or_default();
                            
                            let resolved_body_yaml = crate::utils::resolve_variables(
                                &raw_body_yaml, 
                                &user_context,
                                &csv_cache_ref,
                                my_request_row
                            ).into_owned();

                            let request_body = serde_yaml::from_str::<serde_yaml::Value>(&resolved_body_yaml)
                                .unwrap_or(serde_yaml::Value::Null);

                            let req_start = Instant::now();
                            
                            // --- 4. EXECUTE TARGET REQUEST ---
                            let response_result = engine::execute_request(
                                &client_ref, 
                                &cfg.host, 
                                &method_upper, 
                                &resolved_endpoint, 
                                &request_body, 
                                &Some(merged_headers)
                            ).await;
        
                            let is_success = response_result.is_ok();
                            let duration = req_start.elapsed();
                            req_counter += 1;
        
                            // --- 5. ENGINE DYNAMIC VARIABLE CAPTURE (CORRELATION) ---
                            if let Ok(raw_response_body) = &response_result {
                                if let Some(capture_val) = details.get("capture") {
                                    if let Some(capture_rules) = capture_val.as_mapping() {
                                        if let Ok(json_tree) = serde_json::from_str::<serde_json::Value>(raw_response_body) {
                                            for (var_name_key, json_path_key) in capture_rules {
                                                if let (Some(var_name), Some(json_path)) = (var_name_key.as_str(), json_path_key.as_str()) {
                                                    let field = json_path.replace("json.", "");
                                                    if let Some(extracted_val) = json_tree.get(&field) {
                                                        if let Some(val_str) = extracted_val.as_str() {
                                                            user_context.insert(var_name.to_string(), val_str.to_string());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // --- METRICS RECORDING ---
                            let current_active = {
                                let mut s = stats_clone.lock().unwrap();
                                let entry = s.endpoints.entry(format!("{} {}", method_upper, resolved_endpoint)).or_default();
                                entry.requests += 1;
                                if is_success { entry.success += 1; } else { entry.failure += 1; }
                                entry.total_response_time += duration;
                                if duration < entry.min_response_time { entry.min_response_time = duration; }
                                if duration > entry.max_response_time { entry.max_response_time = duration; }
                                s.active_users 
                            };
        
                            if req_counter % 10 >= 0 {
                                let _ = worker_log_tx.send(logger::LogEvent {
                                    timestamp: start_instant.elapsed().as_secs(),
                                    event_type: "request".to_string(),
                                    endpoint: Some(resolved_endpoint),
                                    response_time_ms: Some(duration.as_millis()),
                                    status: Some(is_success),
                                    tps: 0.0,
                                    cpu_usage: 0.0,
                                    mem_mb: 0,
                                    active_users: current_active, 
                                });
                            }
        
                            if cfg.peak_tps.is_none() {
                                if let Some(sl) = cfg.sleep { sleep(Duration::from_secs_f64(sl)).await; }
                            }
                        }
                    }
                }
            };
        
            tokio::select! {
                _ = token.cancelled() => {}
                _ = tokio::time::timeout(time_limit, worker_logic) => {}
            }
            { stats_clone.lock().unwrap().active_users -= 1; }
        });
        handles.push(handle);
    }

    // --- MAIN MONITORING LOOP ---
    tokio::select! {
        _ = sleep(Duration::from_secs(total_runtime)) => {
            println!("\n✅ Test time limit reached.");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\n🛑 Manual Abort (Ctrl+C). Shutting down...");
        }
    }

    cancel_token.cancel(); 
    println!("⏳ Flushing logs and waiting for workers...");
    sleep(Duration::from_secs(3)).await;

    stats::save_to_csv(Arc::clone(&stats), &config.testname);
    
    println!("📊 Generating report...");
    report::generate_report(
        &format!("reports/{}_log.jsonl", config.testname),
        &format!("reports/{}_report.html", config.testname),
    );
    println!("\n🏁 Done.");

    println!("📊 Generating modern version of report...");
    report_modern::generate_report(
        &format!("reports/{}_log.jsonl", config.testname),
        &format!("reports/{}_report_modern.html", config.testname),
    );
    println!("\n🏁 Done.");
}