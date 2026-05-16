mod config;
mod stats;
mod engine;
mod logger;
mod report;
mod report_modern;

use std::{fs, sync::{Arc, Mutex}, sync::atomic::{AtomicU64, Ordering}, time::{Duration, Instant}};
use tokio::time::{sleep};
use tokio_util::sync::CancellationToken; // Ensure tokio-util is in Cargo.toml
use config::TestConfig;
use stats::GlobalStats;
use rlimit::{getrlimit, setrlimit, Resource};


fn tune_system_limits() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Get the current limits
    // Soft limit is what's currently enforced; Hard limit is the maximum allowed.
    let (soft, hard) = getrlimit(Resource::NOFILE)?;
    println!("Current ulimit -n: soft={}, hard={}", soft, hard);

    // 2. Define your target (e.g., 64k or 100k)
    let target_limit = 65536;

    // 3. Set the new limit
    // We try to set the soft limit to our target, but we cannot exceed the hard limit.
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

// fn find_python() -> &'static str {
//     if std::process::Command::new("python").arg("--version").output().is_ok() {
//         "python"
//     } else {
//         "python3"
//     }
// }

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

    // Higher connection pool for M4 Pro performance
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

    let mut handles = vec![];
    let spawn_delay_ms = (config.rampup as f64 * 1000.0) / (config.users as f64).max(1.0);
    let global_ticker = Arc::new(AtomicU64::new(0));

    // --- WORKER SPAWNING ---
    for i in 0..config.users {
        let cfg = config.clone();
        let stats_clone = Arc::clone(&stats);
        let ticker_clone = Arc::clone(&global_ticker);
        let client_ref = Arc::clone(&client);
        let worker_log_tx = log_tx.clone();
        let token = cancel_token.clone();

        let handle = tokio::spawn(async move {
            // Ramp-up delay with cancellation check
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
        
                            let endpoint = details.get("endpoint").and_then(|v| v.as_str()).unwrap_or("/");
                            let method_upper = method.to_uppercase();
                            let req_start = Instant::now();
                            
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
        
                            let current_active = {
                                let mut s = stats_clone.lock().unwrap();
                                let entry = s.endpoints.entry(format!("{} {}", method_upper, endpoint)).or_default();
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
                                    endpoint: Some(endpoint.to_string()),
                                    response_time_ms: Some(duration.as_millis()),
                                    status: Some(is_success),
                                    tps: 0.0,
                                    cpu_usage: 0.0,
                                    mem_mb: 0,
                                    active_users: current_active, 
                                });
                            }
        
                            if cfg.peak_tps.is_none() {
                                if let Some(sl) = cfg.sleep { sleep(Duration::from_secs(sl)).await; }
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

    cancel_token.cancel(); // Signal all workers to drop work
    println!("⏳ Flushing logs and waiting for workers...");
    sleep(Duration::from_secs(3)).await;

    stats::save_to_csv(Arc::clone(&stats), &config.testname);
    
    // println!("📊 Generating report...");
    // let python = find_python();
    
    // let _ = std::process::Command::new(python)
    //     .arg("src/report.py")
    //     .arg(format!("reports/{}_log.jsonl", config.testname))
    //     .arg("--output")
    //     .arg(format!("reports/{}_report.html", config.testname))
    //     .status();

    // println!("\n🏁 Done.");

    // rust's version 


    println!("📊 Generating report...");

    report::generate_report(
        &format!("reports/{}_log.jsonl", config.testname),
        &format!("reports/{}_report.html", config.testname),
    );
    
    println!("\n🏁 Done.");
    


    println!("📊 Generating modern version of report...");
    // let python = find_python();
    report_modern::generate_report(
        &format!("reports/{}_log.jsonl", config.testname),
        &format!("reports/{}_report_modern.html", config.testname),
    );
    
    // let _ = std::process::Command::new(python)
    //     .arg("src/report_modern.py")
    //     .arg(format!("reports/{}_log.jsonl", config.testname))
    //     .arg("--output")
    //     .arg(format!("reports/{}_report_modern.html", config.testname))
    //     .status();

    println!("\n🏁 Done.");


}