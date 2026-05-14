import argparse
import json
from collections import defaultdict, Counter
from statistics import mean, median, stdev, quantiles
from datetime import datetime
from math import sqrt
from pathlib import Path

def generate_report():
    parser = argparse.ArgumentParser(description="Generate Dashboard from Rust Load Test JSONL.")
    parser.add_argument("json_file", type=Path, help="Path to the .jsonl result file")
    parser.add_argument("--output", type=Path, help="Path to output HTML", required=True)
    args = parser.parse_args()

    # Data Structures for Endpoints
    endpoint_data = defaultdict(lambda: {"durations": [], "failures": 0, "total": 0})
    
    # Data Structures for Time-Series Graphs
    metric_ts = []
    tps_values = []
    cpu_values = []
    mem_values = []
    active_users_values = []

    start_ts = None
    end_ts = None

    print(f"📖 Processing {args.json_file}...")

    duration_seconds = (end_ts - start_ts) if (end_ts and start_ts) else 1
    
    with open(args.json_file, "r") as f:
        for line in f:
            try:
                record = json.loads(line)
                ts = record.get("timestamp", 0)
                
                if start_ts is None: start_ts = ts
                end_ts = ts

                # Handle Request Samples
                if record.get("event_type") == "request":
                    ep = record.get("endpoint", "Unknown")
                    dur = record.get("response_time_ms", 0)
                    success = record.get("status", False)
                    
                    endpoint_data[ep]["durations"].append(dur)
                    endpoint_data[ep]["total"] += 1
                    duration_seconds = record.get("timestamp")
                    if not success:
                        endpoint_data[ep]["failures"] += 1

                # Handle System Metrics
                elif record.get("event_type") == "metric":
                    metric_ts.append(ts)
                    tps_values.append(record.get("tps", 0))
                    cpu_values.append(record.get("cpu_usage", 0))
                    mem_values.append(record.get("mem_mb", 0))
                    active_users_values.append(record.get("active_users", 0))
            except Exception as e:
                continue

    # --- Calculations for Table ---
    # duration_seconds = (end_ts - start_ts) if (end_ts and start_ts) else 1
    html_rows = ""
    all_durations = []
    total_samples_all = 0
    total_failed_global = 0

    for endpoint, values in sorted(endpoint_data.items()):
        durations = values["durations"]
        if not durations: continue
        
        total = values["total"]
        failed = values["failures"]
        error_pct = (failed / total) * 100
        avg = mean(durations)
        med = median(durations)
        
        if len(durations) > 1:
            p = quantiles(durations, n=100)
            p90, p95, p99 = p[89], p[94], p[98]
            std_dev = stdev(durations)
        else:
            p90 = p95 = p99 = durations[0]
            std_dev = 0

        throughput = total / duration_seconds

        html_rows += f"""
        <tr>
            <td>{endpoint}</td>
            <td>{total}</td>
            <td>{avg:.2f}</td>
            <td>{med:.2f}</td>
            <td>{p90:.2f}</td>
            <td>{p95:.2f}</td>
            <td>{p99:.2f}</td>
            <td>{min(durations):.2f}</td>
            <td>{max(durations):.2f}</td>
            <td class="{'text-danger fw-bold' if error_pct > 0 else 'text-success'}">{error_pct:.2f}%</td>
            <td>{throughput:.2f}</td>
            <td>{std_dev:.2f}</td>
        </tr>"""
        
        total_samples_all += total
        total_failed_global += failed
        all_durations.extend(durations)

    # Global Aggregate Row
    if all_durations:
        g_avg = mean(all_durations)
        g_med = median(all_durations)
        g_p = quantiles(all_durations, n=100) if len(all_durations) > 1 else [all_durations[0]]*100
        g_err = (total_failed_global / total_samples_all) * 100
        
        total_row = f"""
        <tr class="table-primary fw-bold">
            <td>TOTAL (Aggregated)</td>
            <td>{total_samples_all}</td>
            <td>{g_avg:.2f}</td>
            <td>{g_med:.2f}</td>
            <td>{g_p[89]:.2f}</td>
            <td>{g_p[94]:.2f}</td>
            <td>{g_p[98]:.2f}</td>
            <td>{min(all_durations):.2f}</td>
            <td>{max(all_durations):.2f}</td>
            <td>{g_err:.2f}%</td>
            <td>{total_samples_all/duration_seconds:.2f}</td>
            <td>-</td>
        </tr>"""
        html_rows = total_row + html_rows

    # --- HTML TEMPLATE ---
    html_content = f"""
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <title>Rage Test Dashboard</title>
        <link href="https://cdn.jsdelivr.net/npm/bootstrap@5.3.0/dist/css/bootstrap.min.css" rel="stylesheet">
        <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
        <style>
            body {{ background-color: #f4f7f9; font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; padding: 25px; }}
            .card {{ border: none; box-shadow: 0 4px 12px rgba(0,0,0,0.08); border-radius: 12px; margin-bottom: 25px; }}
            .section-title {{ color: #0d6efd; font-weight: 700; margin-bottom: 20px; text-transform: uppercase; letter-spacing: 1px; }}
            .chart-container {{ position: relative; height: 300px; width: 100%; }}
            .table-sm td, .table-sm th {{ font-size: 0.9rem; }}
        </style>
    </head>
    <body>
        <div class="container-fluid">
            <div class="row mb-4">
                <div class="col-12 text-center">
                    <h1 class="display-5 fw-bold text-primary">Rage Test Dashboard</h1>
                    <p class="text-muted">Test Duration: <b>{duration_seconds}s</b> | Total Requests Logged: <b>{total_samples_all}</b></p>
                </div>
            </div>

            <div class="card p-4">
                <h4 class="section-title">Endpoint Statistics</h4>
                <div class="table-responsive">
                    <table class="table table-hover table-sm align-middle">
                        <thead class="table-dark">
                            <tr>
                                <th>Label</th><th>Samples</th><th>Avg (ms)</th><th>Med (ms)</th>
                                <th>90%</th><th>95%</th><th>99%</th><th>Min</th><th>Max</th>
                                <th>Error %</th><th>Throughput (req/s)</th><th>Std Dev</th>
                            </tr>
                        </thead>
                        <tbody>{html_rows}</tbody>
                    </table>
                </div>
            </div>

            <div class="row">
                <div class="col-lg-6">
                    <div class="card p-3">
                        <h5 class="text-center text-muted">Active Users Over Time</h5>
                        <div class="chart-container"><canvas id="vusChart"></canvas></div>
                    </div>
                </div>
                <div class="col-lg-6">
                    <div class="card p-3">
                        <h5 class="text-center text-muted">Throughput (Total TPS)</h5>
                        <div class="chart-container"><canvas id="tpsChart"></canvas></div>
                    </div>
                </div>
                <div class="col-lg-6">
                    <div class="card p-3">
                        <h5 class="text-center text-muted">CPU Utilization (%)</h5>
                        <div class="chart-container"><canvas id="cpuChart"></canvas></div>
                    </div>
                </div>
                <div class="col-lg-6">
                    <div class="card p-3">
                        <h5 class="text-center text-muted">Memory Usage (MB)</h5>
                        <div class="chart-container"><canvas id="memChart"></canvas></div>
                    </div>
                </div>
            </div>
        </div>

        <script>
            const timeLabels = {json.dumps(metric_ts)};
            
            // Shared Chart Config
            const options = {{ 
                responsive: true, 
                maintainAspectRatio: false,
                plugins: {{ legend: {{ display: false }} }},
                scales: {{ x: {{ grid: {{ display: false }} }} }}
            }};

            new Chart(document.getElementById('vusChart'), {{
                type: 'line',
                data: {{ labels: timeLabels, datasets: [{{ label: 'Users', data: {json.dumps(active_users_values)}, borderColor: '#198754', backgroundColor: 'rgba(25, 135, 84, 0.1)', fill: true, tension: 0.3 }}] }},
                options: options
            }});

            new Chart(document.getElementById('tpsChart'), {{
                type: 'line',
                data: {{ labels: timeLabels, datasets: [{{ label: 'TPS', data: {json.dumps(tps_values)}, borderColor: '#fd7e14', backgroundColor: 'rgba(253, 126, 20, 0.1)', fill: true, tension: 0.3 }}] }},
                options: options
            }});

            new Chart(document.getElementById('cpuChart'), {{
                type: 'line',
                data: {{ labels: timeLabels, datasets: [{{ label: 'CPU %', data: {json.dumps(cpu_values)}, borderColor: '#0d6efd', tension: 0.3 }}] }},
                options: options
            }});

            new Chart(document.getElementById('memChart'), {{
                type: 'line',
                data: {{ labels: timeLabels, datasets: [{{ label: 'RAM MB', data: {json.dumps(mem_values)}, borderColor: '#6f42c1', tension: 0.3 }}] }},
                options: options
            }});
        </script>
    </body>
    </html>
    """

    with open(args.output, "w", encoding="utf-8") as f:
        f.write(html_content)
    print(f"✅ Dashboard generated successfully: {args.output}")

if __name__ == "__main__":
    generate_report()

# ========================

# import argparse
# import json
# from collections import defaultdict
# from statistics import mean, median, stdev, quantiles
# from pathlib import Path
# from datetime import datetime

# # =========================================
# #          RAGE LOAD TEST REPORT
# # =========================================

# def safe_quantiles(values):
#     if len(values) <= 1:
#         return values[0], values[0], values[0]

#     q = quantiles(values, n=100)
#     return q[89], q[94], q[98]


# def format_ts(ts):
#     try:
#         return datetime.fromtimestamp(ts).strftime("%H:%M:%S")
#     except:
#         return str(ts)


# def generate_report():
#     parser = argparse.ArgumentParser(
#         description="Generate RAGE Dark Mode Dashboard"
#     )

#     parser.add_argument(
#         "json_file",
#         type=Path,
#         help="Path to JSONL file"
#     )

#     parser.add_argument(
#         "--output",
#         type=Path,
#         required=True,
#         help="Output HTML file"
#     )

#     args = parser.parse_args()

#     endpoint_data = defaultdict(
#         lambda: {
#             "durations": [],
#             "failures": 0,
#             "total": 0
#         }
#     )

#     metric_ts = []
#     tps_values = []
#     cpu_values = []
#     mem_values = []
#     active_users_values = []

#     start_ts = None
#     end_ts = None

#     print(f"🔥 Processing {args.json_file} ...")

#     with open(args.json_file, "r", encoding="utf-8") as f:
#         for line in f:
#             try:
#                 record = json.loads(line)

#                 ts = record.get("timestamp", 0)

#                 if start_ts is None:
#                     start_ts = ts

#                 end_ts = ts

#                 if record.get("event_type") == "request":

#                     ep = record.get("endpoint", "UNKNOWN")
#                     dur = record.get("response_time_ms", 0)
#                     success = record.get("status", False)

#                     endpoint_data[ep]["durations"].append(dur)
#                     endpoint_data[ep]["total"] += 1

#                     if not success:
#                         endpoint_data[ep]["failures"] += 1

#                 elif record.get("event_type") == "metric":

#                     metric_ts.append(format_ts(ts))
#                     tps_values.append(record.get("tps", 0))
#                     cpu_values.append(record.get("cpu_usage", 0))
#                     mem_values.append(record.get("mem_mb", 0))
#                     active_users_values.append(record.get("active_users", 0))

#             except:
#                 continue

#     duration_seconds = max((end_ts - start_ts), 1)

#     html_rows = ""

#     total_samples_all = 0
#     total_failed_global = 0
#     all_durations = []

#     for endpoint, values in sorted(endpoint_data.items()):

#         durations = values["durations"]

#         if not durations:
#             continue

#         total = values["total"]
#         failed = values["failures"]

#         avg = mean(durations)
#         med = median(durations)

#         p90, p95, p99 = safe_quantiles(durations)

#         std_dev = stdev(durations) if len(durations) > 1 else 0

#         err_pct = (failed / total) * 100

#         throughput = total / duration_seconds

#         health = "healthy"
#         health_text = "STABLE"

#         if err_pct > 5:
#             health = "critical"
#             health_text = "CRITICAL"
#         elif err_pct > 0:
#             health = "warning"
#             health_text = "UNSTABLE"

#         html_rows += f"""
#         <tr>
#             <td class="endpoint">{endpoint}</td>
#             <td>{total}</td>
#             <td>{avg:.2f}</td>
#             <td>{med:.2f}</td>
#             <td>{p90:.2f}</td>
#             <td>{p95:.2f}</td>
#             <td>{p99:.2f}</td>
#             <td>{min(durations):.2f}</td>
#             <td>{max(durations):.2f}</td>
#             <td class="{health}">{err_pct:.2f}%</td>
#             <td>{throughput:.2f}</td>
#             <td>{std_dev:.2f}</td>
#             <td class="{health}">{health_text}</td>
#         </tr>
#         """

#         total_samples_all += total
#         total_failed_global += failed
#         all_durations.extend(durations)

#     overall_avg = mean(all_durations) if all_durations else 0
#     overall_err = (
#         (total_failed_global / total_samples_all) * 100
#         if total_samples_all else 0
#     )

#     peak_tps = max(tps_values) if tps_values else 0
#     peak_users = max(active_users_values) if active_users_values else 0
#     peak_cpu = max(cpu_values) if cpu_values else 0
#     peak_mem = max(mem_values) if mem_values else 0

#     html_content = f"""
# <!DOCTYPE html>
# <html lang="en">

# <head>

# <meta charset="UTF-8">

# <title>RAGE // SYSTEM EXECUTION REPORT</title>

# <meta name="viewport" content="width=device-width, initial-scale=1.0">

# <link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;800&family=Inter:wght@300;400;500;600&display=swap" rel="stylesheet">

# <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>

# <style>

# :root {{
#     --bg: #050816;
#     --card: rgba(17, 25, 40, 0.75);
#     --border: rgba(255,255,255,0.08);
#     --text: #e5e7eb;
#     --muted: #94a3b8;
#     --glow1: #ff004c;
#     --glow2: #7c3aed;
#     --glow3: #00e5ff;
#     --success: #00ff99;
#     --danger: #ff4d6d;
#     --warning: #ffcc00;
# }}

# * {{
#     margin: 0;
#     padding: 0;
#     box-sizing: border-box;
# }}

# body {{
#     background:
#         radial-gradient(circle at top left, rgba(124,58,237,0.25), transparent 30%),
#         radial-gradient(circle at bottom right, rgba(255,0,76,0.2), transparent 30%),
#         #050816;

#     color: var(--text);

#     font-family: 'Inter', sans-serif;

#     overflow-x: hidden;

#     padding: 30px;
# }}

# body::before {{
#     content: "";
#     position: fixed;
#     inset: 0;
#     background-image:
#         linear-gradient(rgba(255,255,255,0.03) 1px, transparent 1px),
#         linear-gradient(90deg, rgba(255,255,255,0.03) 1px, transparent 1px);

#     background-size: 50px 50px;

#     pointer-events: none;

#     z-index: -1;
# }}

# .hero {{
#     margin-bottom: 40px;
# }}

# .title {{
#     font-family: 'Orbitron', sans-serif;
#     font-size: 4rem;
#     font-weight: 800;
#     letter-spacing: 6px;

#     background: linear-gradient(90deg, #ff004c, #7c3aed, #00e5ff);

#     -webkit-background-clip: text;
#     -webkit-text-fill-color: transparent;

#     text-shadow:
#         0 0 30px rgba(255,0,76,0.3),
#         0 0 60px rgba(124,58,237,0.3);
# }}

# .subtitle {{
#     margin-top: 10px;
#     color: var(--muted);
#     font-size: 1rem;
# }}

# .grid {{
#     display: grid;
#     grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
#     gap: 22px;
#     margin-bottom: 35px;
# }}

# .card {{
#     position: relative;

#     background: var(--card);

#     backdrop-filter: blur(18px);

#     border: 1px solid var(--border);

#     border-radius: 22px;

#     padding: 24px;

#     overflow: hidden;

#     box-shadow:
#         0 10px 30px rgba(0,0,0,0.4),
#         inset 0 1px rgba(255,255,255,0.05);

#     transition: all 0.3s ease;
# }}

# .card:hover {{
#     transform: translateY(-4px);
#     border-color: rgba(255,255,255,0.16);
# }}

# .card::before {{
#     content: "";
#     position: absolute;
#     inset: -1px;
#     background: linear-gradient(
#         135deg,
#         rgba(255,0,76,0.25),
#         rgba(124,58,237,0.18),
#         rgba(0,229,255,0.18)
#     );
#     z-index: -1;
#     filter: blur(25px);
# }}

# .metric-title {{
#     color: var(--muted);
#     font-size: 0.85rem;
#     margin-bottom: 12px;
#     letter-spacing: 1px;
# }}

# .metric-value {{
#     font-size: 2.4rem;
#     font-weight: 700;
#     font-family: 'Orbitron', sans-serif;
# }}

# .metric-sub {{
#     margin-top: 10px;
#     color: #8b9bb5;
#     font-size: 0.9rem;
# }}

# .section-title {{
#     margin-bottom: 20px;
#     font-size: 1.2rem;
#     font-weight: 600;
#     color: white;
#     letter-spacing: 1px;
# }}

# .chart-card {{
#     height: 420px;
# }}

# canvas {{
#     margin-top: 15px;
# }}

# .table-card {{
#     overflow-x: auto;
# }}

# table {{
#     width: 100%;
#     border-collapse: collapse;
#     min-width: 1400px;
# }}

# thead {{
#     background: rgba(255,255,255,0.04);
# }}

# th {{
#     color: #dbeafe;
#     font-size: 0.85rem;
#     padding: 16px;
#     text-align: left;
# }}

# td {{
#     padding: 14px 16px;
#     border-top: 1px solid rgba(255,255,255,0.05);
#     color: #cbd5e1;
#     font-size: 0.9rem;
# }}

# tr:hover {{
#     background: rgba(255,255,255,0.03);
# }}

# .endpoint {{
#     color: white;
#     font-weight: 600;
# }}

# .healthy {{
#     color: var(--success);
#     font-weight: 700;
# }}

# .warning {{
#     color: var(--warning);
#     font-weight: 700;
# }}

# .critical {{
#     color: var(--danger);
#     font-weight: 700;
# }}

# .footer {{
#     margin-top: 50px;
#     text-align: center;
#     color: #64748b;
#     font-size: 0.9rem;
# }}

# .glow {{
#     position: fixed;
#     width: 500px;
#     height: 500px;
#     border-radius: 50%;
#     filter: blur(120px);
#     opacity: 0.08;
#     pointer-events: none;
# }}

# .glow1 {{
#     top: -100px;
#     left: -100px;
#     background: #ff004c;
# }}

# .glow2 {{
#     bottom: -100px;
#     right: -100px;
#     background: #7c3aed;
# }}

# </style>

# </head>

# <body>

# <div class="glow glow1"></div>
# <div class="glow glow2"></div>

# <div class="hero">

#     <div class="title">RAGE</div>

#     <div class="subtitle">
#         SYSTEM ANNIHILATION REPORT · LIVE LOAD WARFARE ANALYTICS
#     </div>

# </div>

# <div class="grid">

#     <div class="card">
#         <div class="metric-title">TOTAL REQUESTS</div>
#         <div class="metric-value">{total_samples_all:,}</div>
#         <div class="metric-sub">Captured under extreme concurrency</div>
#     </div>

#     <div class="card">
#         <div class="metric-title">AVG LATENCY</div>
#         <div class="metric-value">{overall_avg:.1f}ms</div>
#         <div class="metric-sub">Mean response delay</div>
#     </div>

#     <div class="card">
#         <div class="metric-title">ERROR RATE</div>
#         <div class="metric-value">{overall_err:.2f}%</div>
#         <div class="metric-sub">Failure ratio during assault</div>
#     </div>

#     <div class="card">
#         <div class="metric-title">PEAK TPS</div>
#         <div class="metric-value">{peak_tps:.0f}</div>
#         <div class="metric-sub">Transactions per second</div>
#     </div>

#     <div class="card">
#         <div class="metric-title">MAX USERS</div>
#         <div class="metric-value">{peak_users:,}</div>
#         <div class="metric-sub">Concurrent virtual entities</div>
#     </div>

#     <div class="card">
#         <div class="metric-title">DURATION</div>
#         <div class="metric-value">{duration_seconds}s</div>
#         <div class="metric-sub">Chaos sustained</div>
#     </div>

# </div>

# <div class="grid">

#     <div class="card chart-card">
#         <div class="section-title">ACTIVE ENTITIES</div>
#         <canvas id="usersChart"></canvas>
#     </div>

#     <div class="card chart-card">
#         <div class="section-title">THROUGHPUT MATRIX</div>
#         <canvas id="tpsChart"></canvas>
#     </div>

#     <div class="card chart-card">
#         <div class="section-title">CPU MELTDOWN</div>
#         <canvas id="cpuChart"></canvas>
#     </div>

#     <div class="card chart-card">
#         <div class="section-title">MEMORY CONSUMPTION</div>
#         <canvas id="memChart"></canvas>
#     </div>

# </div>

# <div class="card table-card">

#     <div class="section-title">
#         ENDPOINT WAR REPORT
#     </div>

#     <table>

#         <thead>
#             <tr>
#                 <th>ENDPOINT</th>
#                 <th>SAMPLES</th>
#                 <th>AVG</th>
#                 <th>MED</th>
#                 <th>P90</th>
#                 <th>P95</th>
#                 <th>P99</th>
#                 <th>MIN</th>
#                 <th>MAX</th>
#                 <th>ERROR</th>
#                 <th>TPS</th>
#                 <th>STD DEV</th>
#                 <th>STATUS</th>
#             </tr>
#         </thead>

#         <tbody>
#             {html_rows}
#         </tbody>

#     </table>

# </div>

# <div class="footer">
#     RAGE | 2026
# </div>

# <script>

# const labels = {json.dumps(metric_ts)};

# Chart.defaults.color = "#94a3b8";

# const commonOptions = {{

#     responsive: true,

#     maintainAspectRatio: false,

#     interaction: {{
#         mode: 'index',
#         intersect: false
#     }},

#     plugins: {{
#         legend: {{
#             display: false
#         }}
#     }},

#     scales: {{

#         x: {{
#             grid: {{
#                 color: 'rgba(255,255,255,0.03)'
#             }}
#         }},

#         y: {{
#             grid: {{
#                 color: 'rgba(255,255,255,0.04)'
#             }}
#         }}
#     }}
# }};

# function createChart(id, data, border, bg) {{

#     new Chart(document.getElementById(id), {{

#         type: 'line',

#         data: {{
#             labels: labels,
#             datasets: [{{
#                 data: data,
#                 borderColor: border,
#                 backgroundColor: bg,
#                 fill: true,
#                 borderWidth: 2,
#                 tension: 0.35,
#                 pointRadius: 0
#             }}]
#         }},

#         options: commonOptions
#     }});
# }}

# createChart(
#     "usersChart",
#     {json.dumps(active_users_values)},
#     "#00e5ff",
#     "rgba(0,229,255,0.12)"
# );

# createChart(
#     "tpsChart",
#     {json.dumps(tps_values)},
#     "#ff004c",
#     "rgba(255,0,76,0.12)"
# );

# createChart(
#     "cpuChart",
#     {json.dumps(cpu_values)},
#     "#7c3aed",
#     "rgba(124,58,237,0.12)"
# );

# createChart(
#     "memChart",
#     {json.dumps(mem_values)},
#     "#00ff99",
#     "rgba(0,255,153,0.12)"
# );

# </script>

# </body>
# </html>
# """

#     with open(args.output, "w", encoding="utf-8") as f:
#         f.write(html_content)

#     print(f"✅ RAGE dashboard generated: {args.output}")


# if __name__ == "__main__":
#     generate_report()