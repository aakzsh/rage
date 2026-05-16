use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};

use serde_json::Value;

#[derive(Default, Clone)]
struct EndpointStats {
    durations: Vec<f64>,
    failures: u64,
    total: u64,
}

// ============================================================
// STATS HELPERS
// ============================================================

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }

    v.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let n = v.len();

    if n % 2 == 0 {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    } else {
        v[n / 2]
    }
}

fn percentile(mut v: Vec<f64>, percentile: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }

    v.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let rank = percentile / 100.0 * (v.len() - 1) as f64;

    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;

    if low == high {
        v[low]
    } else {
        let weight = rank - low as f64;
        v[low] * (1.0 - weight) + v[high] * weight
    }
}

fn std_dev(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }

    let avg = mean(v);

    let variance = v
        .iter()
        .map(|x| (x - avg).powi(2))
        .sum::<f64>()
        / (v.len() as f64 - 1.0);

    variance.sqrt()
}

// ============================================================
// MAIN REPORT GENERATOR
// ============================================================

pub fn generate_report(input: &str, output: &str) {
    println!("📖 Processing {}...", input);

    let file = File::open(input).expect("Failed to open input file");
    let reader = BufReader::new(file);

    // BTreeMap keeps endpoints sorted
    let mut endpoint_data: BTreeMap<String, EndpointStats> = BTreeMap::new();

    // charts
    let mut metric_ts = Vec::<f64>::new();
    let mut tps_values = Vec::<f64>::new();
    let mut cpu_values = Vec::<f64>::new();
    let mut mem_values = Vec::<f64>::new();
    let mut active_users_values = Vec::<f64>::new();

    let mut start_ts: Option<f64> = None;
    let mut end_ts: Option<f64> = None;

    // ========================================================
    // PARSE JSONL
    // ========================================================

    for line in reader.lines() {
        let line = match line {
            Ok(v) => v,
            Err(_) => continue,
        };

        let record: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ts = record
            .get("timestamp")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if start_ts.is_none() {
            start_ts = Some(ts);
        }

        end_ts = Some(ts);

        let event_type = record
            .get("event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // ====================================================
        // REQUEST EVENTS
        // ====================================================

        if event_type == "request" {
            let endpoint = record
                .get("endpoint")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();

            let response_time = record
                .get("response_time_ms")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let success = record
                .get("status")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let entry = endpoint_data.entry(endpoint).or_default();

            entry.durations.push(response_time);
            entry.total += 1;

            if !success {
                entry.failures += 1;
            }
        }

        // ====================================================
        // METRIC EVENTS
        // ====================================================

        else if event_type == "metric" {
            metric_ts.push(ts);

            tps_values.push(
                record
                    .get("tps")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            );

            cpu_values.push(
                record
                    .get("cpu_usage")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            );

            mem_values.push(
                record
                    .get("mem_mb")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            );

            active_users_values.push(
                record
                    .get("active_users")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            );
        }
    }

    let duration_seconds = match (start_ts, end_ts) {
        (Some(s), Some(e)) if e > s => e - s,
        _ => 1.0,
    };

    // ========================================================
    // TABLE BUILD
    // ========================================================

    let mut html_rows = String::new();

    let mut all_durations = Vec::<f64>::new();

    let mut total_samples_all = 0u64;
    let mut total_failed_all = 0u64;

    for (endpoint, stats) in &endpoint_data {
        if stats.durations.is_empty() {
            continue;
        }

        let avg = mean(&stats.durations);

        let med = median(stats.durations.clone());

        let p90 = percentile(stats.durations.clone(), 90.0);

        let p95 = percentile(stats.durations.clone(), 95.0);

        let p99 = percentile(stats.durations.clone(), 99.0);

        let min = stats
            .durations
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);

        let max = stats
            .durations
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);

        let std = std_dev(&stats.durations);

        let error_pct = stats.failures as f64 / stats.total as f64 * 100.0;

        let throughput = stats.total as f64 / duration_seconds;

        let error_class = if error_pct > 0.0 {
            "text-danger fw-bold"
        } else {
            "text-success"
        };

        html_rows.push_str(&format!(
            r#"
<tr>
    <td>{}</td>
    <td>{}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td class="{}">{:.2}%</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
</tr>
"#,
            endpoint,
            stats.total,
            avg,
            med,
            p90,
            p95,
            p99,
            min,
            max,
            error_class,
            error_pct,
            throughput,
            std
        ));

        total_samples_all += stats.total;
        total_failed_all += stats.failures;

        all_durations.extend(stats.durations.clone());
    }

    // ========================================================
    // TOTAL AGGREGATE ROW
    // ========================================================

    if !all_durations.is_empty() {
        let g_avg = mean(&all_durations);

        let g_med = median(all_durations.clone());

        let g_p90 = percentile(all_durations.clone(), 90.0);

        let g_p95 = percentile(all_durations.clone(), 95.0);

        let g_p99 = percentile(all_durations.clone(), 99.0);

        let g_error =
            total_failed_all as f64 / total_samples_all as f64 * 100.0;

        let total_row = format!(
            r#"
<tr class="table-primary fw-bold">
    <td>TOTAL (Aggregated)</td>
    <td>{}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}</td>
    <td>{:.2}%</td>
    <td>{:.2}</td>
    <td>-</td>
</tr>
"#,
            total_samples_all,
            g_avg,
            g_med,
            g_p90,
            g_p95,
            g_p99,
            all_durations
                .iter()
                .cloned()
                .fold(f64::INFINITY, f64::min),
            all_durations
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max),
            g_error,
            total_samples_all as f64 / duration_seconds
        );

        html_rows = format!("{}{}", total_row, html_rows);
    }

    // ========================================================
    // HTML
    // ========================================================

    let html = format!(
        r#"
<!DOCTYPE html>
<html lang="en">

<head>
<meta charset="UTF-8">

<title>Rage Test Dashboard</title>

<link href="https://cdn.jsdelivr.net/npm/bootstrap@5.3.0/dist/css/bootstrap.min.css" rel="stylesheet">

<script src="https://cdn.jsdelivr.net/npm/chart.js"></script>

<style>

body {{
    background-color: #f4f7f9;
    font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
    padding: 25px;
}}

.card {{
    border: none;
    box-shadow: 0 4px 12px rgba(0,0,0,0.08);
    border-radius: 12px;
    margin-bottom: 25px;
}}

.section-title {{
    color: #0d6efd;
    font-weight: 700;
    margin-bottom: 20px;
    text-transform: uppercase;
    letter-spacing: 1px;
}}

.chart-container {{
    position: relative;
    height: 300px;
    width: 100%;
}}

.table-sm td,
.table-sm th {{
    font-size: 0.9rem;
}}

</style>
</head>

<body>

<div class="container-fluid">

<div class="row mb-4">
<div class="col-12 text-center">

<h1 class="display-5 fw-bold text-primary">
Rage Test Dashboard
</h1>

<p class="text-muted">
Test Duration:
<b>{:.2}s</b>
|
Total Requests Logged:
<b>{}</b>
</p>

</div>
</div>

<div class="card p-4">

<h4 class="section-title">
Endpoint Statistics
</h4>

<div class="table-responsive">

<table class="table table-hover table-sm align-middle">

<thead class="table-dark">
<tr>
<th>Label</th>
<th>Samples</th>
<th>Avg (ms)</th>
<th>Med (ms)</th>
<th>90%</th>
<th>95%</th>
<th>99%</th>
<th>Min</th>
<th>Max</th>
<th>Error %</th>
<th>Throughput (req/s)</th>
<th>Std Dev</th>
</tr>
</thead>

<tbody>
{}
</tbody>

</table>

</div>
</div>

<div class="row">

<div class="col-lg-6">
<div class="card p-3">

<h5 class="text-center text-muted">
Active Users Over Time
</h5>

<div class="chart-container">
<canvas id="vusChart"></canvas>
</div>

</div>
</div>

<div class="col-lg-6">
<div class="card p-3">

<h5 class="text-center text-muted">
Throughput (Total TPS)
</h5>

<div class="chart-container">
<canvas id="tpsChart"></canvas>
</div>

</div>
</div>

<div class="col-lg-6">
<div class="card p-3">

<h5 class="text-center text-muted">
CPU Utilization (%)
</h5>

<div class="chart-container">
<canvas id="cpuChart"></canvas>
</div>

</div>
</div>

<div class="col-lg-6">
<div class="card p-3">

<h5 class="text-center text-muted">
Memory Usage (MB)
</h5>

<div class="chart-container">
<canvas id="memChart"></canvas>
</div>

</div>
</div>

</div>
</div>

<script>

const timeLabels = {};

const options = {{
    responsive: true,
    maintainAspectRatio: false,

    plugins: {{
        legend: {{
            display: false
        }}
    }},

    scales: {{
        x: {{
            grid: {{
                display: false
            }}
        }}
    }}
}};

// ======================================================
// ACTIVE USERS
// ======================================================

new Chart(document.getElementById('vusChart'), {{
    type: 'line',

    data: {{
        labels: timeLabels,

        datasets: [{{
            label: 'Users',
            data: {},
            borderColor: '#198754',
            backgroundColor: 'rgba(25, 135, 84, 0.1)',
            fill: true,
            tension: 0.3
        }}]
    }},

    options: options
}});

// ======================================================
// TPS
// ======================================================

new Chart(document.getElementById('tpsChart'), {{
    type: 'line',

    data: {{
        labels: timeLabels,

        datasets: [{{
            label: 'TPS',
            data: {},
            borderColor: '#fd7e14',
            backgroundColor: 'rgba(253, 126, 20, 0.1)',
            fill: true,
            tension: 0.3
        }}]
    }},

    options: options
}});

// ======================================================
// CPU
// ======================================================

new Chart(document.getElementById('cpuChart'), {{
    type: 'line',

    data: {{
        labels: timeLabels,

        datasets: [{{
            label: 'CPU',
            data: {},
            borderColor: '#0d6efd',
            tension: 0.3
        }}]
    }},

    options: options
}});

// ======================================================
// MEMORY
// ======================================================

new Chart(document.getElementById('memChart'), {{
    type: 'line',

    data: {{
        labels: timeLabels,

        datasets: [{{
            label: 'Memory',
            data: {},
            borderColor: '#6f42c1',
            tension: 0.3
        }}]
    }},

    options: options
}});

</script>

</body>
</html>
"#,
        duration_seconds,
        total_samples_all,
        html_rows,
        serde_json::to_string(&metric_ts).unwrap(),
        serde_json::to_string(&active_users_values).unwrap(),
        serde_json::to_string(&tps_values).unwrap(),
        serde_json::to_string(&cpu_values).unwrap(),
        serde_json::to_string(&mem_values).unwrap(),
    );

    fs::write(output, html).expect("Failed to write report");

    println!("✅ Dashboard generated successfully: {}", output);
}