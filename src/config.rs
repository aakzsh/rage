
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use serde_yaml::Value;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct StepDetails {
    pub endpoint: String,
    // Using serde(default) ensures that if a step doesn't have headers/body/capture, 
    // it won't throw a parsing error; it will just default to None.
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>, 
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    #[serde(default)]
    pub capture: Option<HashMap<String, String>>, 
}

#[derive(Debug, Deserialize, Clone)]
pub struct TestConfig {
    /// The name of the test, used for filenames (CSV and JSONL)
    pub testname: String,

    /// The target base URL (e.g., "http://localhost:8080")
    pub host: String,

    /// Maximum number of concurrent virtual users
    pub users: u32,

    /// Time in seconds to ramp up from 0 to target users/TPS
    pub rampup: u64,

    /// Total duration of the test in seconds
    pub runtime: u64,

    /// Target Transactions Per Second (TPS). If None, users will fire requests as fast as possible.
    pub peak_tps: Option<u32>,

    /// Specific number of CPU cores to use. 
    /// -1 or None: Use all available cores.
    /// Positive integer: Limit to that specific number of threads.
    #[serde(default = "default_max_cores")]
    pub max_cores: i32,

    /// Global headers applied to every request
    pub common_headers: HashMap<String, String>,

    /// CHANGED: Use your custom StepDetails struct instead of a generic nested HashMap
    pub steps: Vec<HashMap<String, Value>>,

    /// Optional sleep time between steps (only used if peak_tps is not set)
    pub sleep: Option<u64>,

    pub session_duration: Option<u64>,
    pub csv_config: String,
}

/// Default value for max_cores if omitted from YAML
fn default_max_cores() -> i32 {
    -1
}