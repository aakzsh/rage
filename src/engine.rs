use std::collections::HashMap;
use reqwest::Client;
use serde_yaml::Value;

pub async fn execute_request(
    client: &Client, 
    host: &str, 
    method: &str, 
    endpoint: &str, 
    details: &Value,
    common_headers: &Option<HashMap<String, String>>
) -> bool {
    let url = format!("{}{}", host.trim_end_matches('/'), endpoint);
    
    let mut rb = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url).json(&details.get("body").unwrap_or(&Value::Null)),
        _ => return false,
    };

    if let Some(headers) = common_headers {
        for (k, v) in headers {
            rb = rb.header(k, v);
        }
    }

    if let Some(step_headers) = details.get("headers").and_then(|h| h.as_mapping()) {
        for (k, v) in step_headers {
            if let (Some(key), Some(val)) = (k.as_str(), v.as_str()) {
                rb = rb.header(key, val);
            }
        }
    }

    rb.send().await.map(|r| r.status().is_success()).unwrap_or(false)
}