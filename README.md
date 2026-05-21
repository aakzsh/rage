# RAGE

A high-performance load testing engine written in Rust, focused on maximizing throughput and handling extreme concurrency without scripting overhead.

![Rage](images/RAGE_NEW.png)

***

## Overview

Most load testing tools start breaking once you push into very high concurrency or TPS ranges, mainly due to runtime overhead.

RAGE keeps things simple:

* minimal abstractions
* direct execution loops
* predictable pacing

***

## Features

* High concurrency support (100k+ users)
* Controlled TPS execution
* Multi-core utilization
* CSV-based data input
* Dynamic variable handling (token chaining)
* Support for REST APIs (GET/POST/DELETE, headers, body, params)
* Reporting (CLI, CSV, HTML)

***

## Benchmark

### Tool Comparison

| Users | TPS | Locust | k6 | RAGE |
| ----- | --- | ------ | -- | ---- |
| 1k    | 100 | ✔      | ✔  | ✔    |
| 10k   | 1k  | ✔      | ✔  | ✔    |
| 100k  | 10k | ✖      | ✖  | ✔    |
| 200k  | 20k | ✖      | ✖  | ⚠    |
| 200k  | 10k | ✖      | ✖  | ✔    |
| 300k  | 13k | ✖      | ✖  | ✔    |
| 400k  | 10k | ✖      | ✖  | ✔    |
| 420k  | 10k | ✖      | ✖  | ⚠    |
| 10k   | 20k | ⚠      | ✔  | ✔    |
| 10k   | 30k | ✖      | ✔  | ✔    |
| 10k   | 40k | ✖      | ✔  | ⚠    |
| 10k   | 50k | ✖      | ✖  | ✖    |

Legend:

* ✔ supported / successful execution
* ⚠ partial / hit bottleneck
* ✖ failed

<b>Note</b>: A test execution is considered successful only if throughout the test the CPU/Memory utilisations were strictly under 80%. And these numbers are bound to vary across different machines. The aim of benchmarking was to only compare the different tools on the exact same machine infra. These tests were run on Apple M4 Pro chip with 24GB RAM and 512GB total storage.
***

## Benchmark Summary

* \~400k concurrent users reached
* Stable around 10k TPS at scale
* Peak \~30k TPS observed
* Handles scenarios where Locust and k6 fail

***

## Benchmark Reports

* users scaling → `reports/400k_users_benchmarking.html`
* TPS benchmarking → `reports/30k_tps_benchmarking.html`

***

## Runtime View

![Realtime Report](images/realtime-report.png)

***

## Final HTML Report

![Final Report](images/aftertest-report.png)

***

## Setup

```bash
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env

cargo run --release
```

***

## Config Example

```yaml
host: https://api.example.com
users: 100000
rampup: 60
runtime: 300
sleep: 100

steps:
  - get:
      url: /health
```

***

## Use Cases

* High concurrency testing (100k+ users)
* TPS-bound testing
* Finding backend limits under sustained load

