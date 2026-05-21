use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};

use serde_json::Value;

// ============================================================
// DATA STRUCTURES
// ============================================================

#[derive(Default, Clone)]
struct EndpointStats {
    durations: Vec<f64>,
    failures: u64,
    total: u64,
}

// ============================================================
// HELPERS
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

fn safe_quantiles(v: &[f64]) -> (f64, f64, f64) {
    if v.len() <= 1 {
        let value = *v.first().unwrap_or(&0.0);
        return (value, value, value);
    }

    (
        percentile(v.to_vec(), 90.0),
        percentile(v.to_vec(), 95.0),
        percentile(v.to_vec(), 99.0),
    )
}

fn format_timestamp(ts: f64) -> String {
    use chrono::{DateTime, Local};

    let secs = ts as i64;

    match DateTime::from_timestamp(secs, 0) {
        Some(dt) => {
            let local_dt: DateTime<Local> = DateTime::from(dt);
            local_dt.format("%H:%M:%S").to_string()
        }
        None => format!("{:.0}", ts),
    }
}

// ============================================================
// REPORT GENERATOR
// ============================================================

pub fn generate_report(input: &str, output: &str) {
    println!("🔥 Processing {} ...", input);

    let file = File::open(input).expect("Failed to open input file");

    let reader = BufReader::new(file);

    let mut endpoint_data: BTreeMap<String, EndpointStats> =
        BTreeMap::new();

    // charts
    let mut metric_ts = Vec::<String>::new();

    let mut tps_values = Vec::<f64>::new();
    let mut cpu_values = Vec::<f64>::new();
    let mut mem_values = Vec::<f64>::new();
    let mut active_users_values = Vec::<f64>::new();

    let mut start_ts: Option<f64> = None;
    let mut end_ts: Option<f64> = None;

    // ========================================================
    // PARSE
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
        // REQUEST EVENT
        // ====================================================

        if event_type == "request" {
            let endpoint = record
                .get("endpoint")
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN")
                .to_string();

            let duration = record
                .get("response_time_ms")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let success = record
                .get("status")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let entry = endpoint_data.entry(endpoint).or_default();

            entry.durations.push(duration);

            entry.total += 1;

            if !success {
                entry.failures += 1;
            }
        }

        // ====================================================
        // METRIC EVENT
        // ====================================================

        else if event_type == "metric" {
            metric_ts.push(format_timestamp(ts));

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

    // ========================================================
    // DURATION
    // ========================================================

    let duration_seconds = match (start_ts, end_ts) {
        (Some(s), Some(e)) if e > s => (e - s).max(1.0),
        _ => 1.0,
    };

    // ========================================================
    // TABLE
    // ========================================================

    let mut html_rows = String::new();

    let mut total_samples_all = 0u64;
    let mut total_failed_global = 0u64;

    let mut all_durations = Vec::<f64>::new();

    for (endpoint, values) in &endpoint_data {
        let durations = &values.durations;

        if durations.is_empty() {
            continue;
        }

        let total = values.total;
        let failed = values.failures;

        let avg = mean(durations);

        let med = median(durations.clone());

        let (p90, p95, p99) = safe_quantiles(durations);

        let std = if durations.len() > 1 {
            std_dev(durations)
        } else {
            0.0
        };

        let err_pct = failed as f64 / total as f64 * 100.0;

        let throughput = total as f64 / duration_seconds;

        let mut health = "healthy";
        let mut health_text = "STABLE";

        if err_pct > 5.0 {
            health = "critical";
            health_text = "CRITICAL";
        } else if err_pct > 0.0 {
            health = "warning";
            health_text = "UNSTABLE";
        }

        html_rows.push_str(&format!(
            r#"
<tr>
    <td class="endpoint">{}</td>
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
    <td class="{}">{}</td>
</tr>
"#,
            endpoint,
            total,
            avg,
            med,
            p90,
            p95,
            p99,
            durations
                .iter()
                .cloned()
                .fold(f64::INFINITY, f64::min),
            durations
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max),
            health,
            err_pct,
            throughput,
            std,
            health,
            health_text
        ));

        total_samples_all += total;

        total_failed_global += failed;

        all_durations.extend(durations.clone());
    }

    // ========================================================
    // GLOBAL METRICS
    // ========================================================

    let overall_avg = if !all_durations.is_empty() {
        mean(&all_durations)
    } else {
        0.0
    };

    let overall_err = if total_samples_all > 0 {
        total_failed_global as f64 / total_samples_all as f64 * 100.0
    } else {
        0.0
    };

    let peak_tps = tps_values
        .iter()
        .cloned()
        .fold(0.0, f64::max);

    let peak_users = active_users_values
        .iter()
        .cloned()
        .fold(0.0, f64::max);

    let peak_cpu = cpu_values
        .iter()
        .cloned()
        .fold(0.0, f64::max);

    let peak_mem = mem_values
        .iter()
        .cloned()
        .fold(0.0, f64::max);

    // ========================================================
    // HTML
    // ========================================================

    let html = format!(
        r#"
<!DOCTYPE html>
<html lang="en">

<head>

<meta charset="UTF-8">

<title>RAGE DASHBOARD | MODERN</title>

<meta name="viewport" content="width=device-width, initial-scale=1.0">

<link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;800&family=Inter:wght@300;400;500;600&display=swap" rel="stylesheet">

<script src="https://cdn.jsdelivr.net/npm/chart.js"></script>

<style>

:root {{
    --bg: #050816;
    --card: rgba(17, 25, 40, 0.75);
    --border: rgba(255,255,255,0.08);
    --text: #e5e7eb;
    --muted: #94a3b8;
    --glow1: #ff004c;
    --glow2: #7c3aed;
    --glow3: #00e5ff;
    --success: #00ff99;
    --danger: #ff4d6d;
    --warning: #ffcc00;
}}

* {{
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}}

body {{
    background:
        radial-gradient(circle at top left, rgba(124,58,237,0.25), transparent 30%),
        radial-gradient(circle at bottom right, rgba(255,0,76,0.2), transparent 30%),
        #050816;

    color: var(--text);

    font-family: 'Inter', sans-serif;

    overflow-x: hidden;

    padding: 30px;
}}

body::before {{
    content: "";
    position: fixed;
    inset: 0;

    background-image:
        linear-gradient(rgba(255,255,255,0.03) 1px, transparent 1px),
        linear-gradient(90deg, rgba(255,255,255,0.03) 1px, transparent 1px);

    background-size: 50px 50px;

    pointer-events: none;

    z-index: -1;
}}

.hero {{
    margin-bottom: 40px;
}}

.title {{
    font-family: 'Orbitron', sans-serif;

    font-size: 4rem;

    font-weight: 800;

    letter-spacing: 6px;

    background: linear-gradient(
        90deg,
        #ff004c,
        #7c3aed,
        #00e5ff
    );

    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;

    text-shadow:
        0 0 30px rgba(255,0,76,0.3),
        0 0 60px rgba(124,58,237,0.3);
}}

.subtitle {{
    margin-top: 10px;
    color: var(--muted);
    font-size: 1rem;
}}

.grid {{
    display: grid;

    grid-template-columns:
        repeat(auto-fit, minmax(260px, 1fr));

    gap: 22px;

    margin-bottom: 35px;
}}

.card {{
    position: relative;

    background: var(--card);

    backdrop-filter: blur(18px);

    border: 1px solid var(--border);

    border-radius: 22px;

    padding: 24px;

    overflow: hidden;

    box-shadow:
        0 10px 30px rgba(0,0,0,0.4),
        inset 0 1px rgba(255,255,255,0.05);

    transition: all 0.3s ease;
}}

.card:hover {{
    transform: translateY(-4px);
    border-color: rgba(255,255,255,0.16);
}}

.card::before {{
    content: "";

    position: absolute;

    inset: -1px;

    background: linear-gradient(
        135deg,
        rgba(255,0,76,0.25),
        rgba(124,58,237,0.18),
        rgba(0,229,255,0.18)
    );

    z-index: -1;

    filter: blur(25px);
}}

.metric-title {{
    color: var(--muted);
    font-size: 0.85rem;
    margin-bottom: 12px;
    letter-spacing: 1px;
}}

.metric-value {{
    font-size: 2.4rem;
    font-weight: 700;
    font-family: 'Orbitron', sans-serif;
}}

.metric-sub {{
    margin-top: 10px;
    color: #8b9bb5;
    font-size: 0.9rem;
}}

.section-title {{
    margin-bottom: 20px;
    font-size: 1.2rem;
    font-weight: 600;
    color: white;
    letter-spacing: 1px;
}}

.chart-card {{
    height: 420px;
}}

canvas {{
    margin-top: 15px;
}}

.table-card {{
    overflow-x: auto;
}}

table {{
    width: 100%;
    border-collapse: collapse;
    min-width: 1400px;
}}

thead {{
    background: rgba(255,255,255,0.04);
}}

th {{
    color: #dbeafe;
    font-size: 0.85rem;
    padding: 16px;
    text-align: left;
}}

td {{
    padding: 14px 16px;
    border-top: 1px solid rgba(255,255,255,0.05);
    color: #cbd5e1;
    font-size: 0.9rem;
}}

tr:hover {{
    background: rgba(255,255,255,0.03);
}}

.endpoint {{
    color: white;
    font-weight: 600;
}}

.healthy {{
    color: var(--success);
    font-weight: 700;
}}

.warning {{
    color: var(--warning);
    font-weight: 700;
}}

.critical {{
    color: var(--danger);
    font-weight: 700;
}}

.footer {{
    margin-top: 50px;
    text-align: center;
    color: #64748b;
    font-size: 0.9rem;
}}

.glow {{
    position: fixed;
    width: 500px;
    height: 500px;
    border-radius: 50%;
    filter: blur(120px);
    opacity: 0.08;
    pointer-events: none;
}}

.glow1 {{
    top: -100px;
    left: -100px;
    background: #ff004c;
}}

.glow2 {{
    bottom: -100px;
    right: -100px;
    background: #7c3aed;
}}

</style>

</head>

<body>

<div class="glow glow1"></div>
<div class="glow glow2"></div>

<div class="hero">

    <div class="title">RAGE</div>

    <div class="subtitle">
        SYSTEM LOAD REPORT
    </div>

</div>

<div class="grid">

    <div class="card">
        <div class="metric-title">TOTAL REQUESTS</div>
        <div class="metric-value">{}</div>
        <div class="metric-sub">
            Captured under extreme concurrency
        </div>
    </div>

    <div class="card">
        <div class="metric-title">AVG LATENCY</div>
        <div class="metric-value">{:.1}ms</div>
        <div class="metric-sub">
            Mean response delay
        </div>
    </div>

    <div class="card">
        <div class="metric-title">ERROR RATE</div>
        <div class="metric-value">{:.2}%</div>
        <div class="metric-sub">
            Failure ratio during assault
        </div>
    </div>

    <div class="card">
        <div class="metric-title">PEAK TPS</div>
        <div class="metric-value">{:.0}</div>
        <div class="metric-sub">
            Transactions per second
        </div>
    </div>

    <div class="card">
        <div class="metric-title">MAX USERS</div>
        <div class="metric-value">{:.0}</div>
        <div class="metric-sub">
            Concurrent virtual entities
        </div>
    </div>

    <div class="card">
        <div class="metric-title">DURATION</div>
        <div class="metric-value">{:.0}s</div>
        <div class="metric-sub">
            Chaos sustained
        </div>
    </div>

    <div class="card">
        <div class="metric-title">PEAK CPU</div>
        <div class="metric-value">{:.1}%</div>
        <div class="metric-sub">
            Maximum processor load
        </div>
    </div>

    <div class="card">
        <div class="metric-title">PEAK MEMORY</div>
        <div class="metric-value">{:.0}MB</div>
        <div class="metric-sub">
            Highest RAM consumption
        </div>
    </div>

</div>

<div class="grid">

    <div class="card chart-card">
        <div class="section-title">
            ACTIVE ENTITIES
        </div>

        <canvas id="usersChart"></canvas>
    </div>

    <div class="card chart-card">
        <div class="section-title">
            THROUGHPUT MATRIX
        </div>

        <canvas id="tpsChart"></canvas>
    </div>

    <div class="card chart-card">
        <div class="section-title">
            CPU MELTDOWN
        </div>

        <canvas id="cpuChart"></canvas>
    </div>

    <div class="card chart-card">
        <div class="section-title">
            MEMORY CONSUMPTION
        </div>

        <canvas id="memChart"></canvas>
    </div>

</div>

<div class="card table-card">

    <div class="section-title">
        ENDPOINT WAR REPORT
    </div>

    <table>

        <thead>

            <tr>
                <th>ENDPOINT</th>
                <th>SAMPLES</th>
                <th>AVG</th>
                <th>MED</th>
                <th>P90</th>
                <th>P95</th>
                <th>P99</th>
                <th>MIN</th>
                <th>MAX</th>
                <th>ERROR</th>
                <th>TPS</th>
                <th>STD DEV</th>
                <th>STATUS</th>
            </tr>

        </thead>

        <tbody>
            {}
        </tbody>

    </table>

</div>

<div class="footer">
    RAGE | 2026
</div>

<script>

const labels = {};

Chart.defaults.color = '#94a3b8';

const commonOptions = {{

    responsive: true,

    maintainAspectRatio: false,

    interaction: {{
        mode: 'index',
        intersect: false
    }},

    plugins: {{
        legend: {{
            display: false
        }}
    }},

    scales: {{

        x: {{
            grid: {{
                color: 'rgba(255,255,255,0.03)'
            }}
        }},

        y: {{
            grid: {{
                color: 'rgba(255,255,255,0.04)'
            }}
        }}
    }}
}};

function createChart(id, data, border, bg) {{

    new Chart(document.getElementById(id), {{

        type: 'line',

        data: {{
            labels: labels,

            datasets: [{{
                data: data,
                borderColor: border,
                backgroundColor: bg,
                fill: true,
                borderWidth: 2,
                tension: 0.35,
                pointRadius: 0
            }}]
        }},

        options: commonOptions
    }});
}}

createChart(
    "usersChart",
    {},
    '#00e5ff',
    "rgba(0,229,255,0.12)"
);

createChart(
    "tpsChart",
    {},
    '#ff004c',
    "rgba(255,0,76,0.12)"
);

createChart(
    "cpuChart",
    {},
    '#7c3aed',
    "rgba(124,58,237,0.12)"
);

createChart(
    "memChart",
    {},
    '#00ff99',
    "rgba(0,255,153,0.12)"
);

</script>

</body>
</html>
"#,
        total_samples_all,
        overall_avg,
        overall_err,
        peak_tps,
        peak_users,
        duration_seconds,
        peak_cpu,
        peak_mem,
        html_rows,
        serde_json::to_string(&metric_ts).unwrap(),
        serde_json::to_string(&active_users_values).unwrap(),
        serde_json::to_string(&tps_values).unwrap(),
        serde_json::to_string(&cpu_values).unwrap(),
        serde_json::to_string(&mem_values).unwrap(),
    );

    fs::write(output, html).expect("Failed to write report");

    println!("✅ RAGE dashboard generated: {}", output);
}