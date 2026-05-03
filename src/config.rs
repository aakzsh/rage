use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct TestConfig {
    pub testname: String, // Add this
    pub host: String,
    pub users: u32,
    pub rampup: u64,
    pub runtime: u64,
    pub sleep: Option<u64>,
    #[serde(rename = "peak-tps")]
    pub peak_tps: Option<u32>,
    #[serde(rename = "common-headers")]
    pub common_headers: Option<HashMap<String, String>>,
    pub steps: Vec<HashMap<String, serde_yaml::Value>>,
}