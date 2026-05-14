use serde::Deserialize;
use std::collections::HashMap;
use serde_yaml::Value;

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

    /// The sequence of API calls to execute
    pub steps: Vec<HashMap<String, HashMap<String, Value>>>,

    /// Optional sleep time between steps (only used if peak_tps is not set)
    pub sleep: Option<u64>,

    pub session_duration: Option<u64>,
}

/// Default value for max_cores if omitted from YAML
fn default_max_cores() -> i32 {
    -1
}