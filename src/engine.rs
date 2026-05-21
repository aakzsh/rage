// src/engine.rs

pub async fn execute_request(
    client: &reqwest::Client,
    host: &str,
    method: &str,
    endpoint: &str,
    body: &serde_yaml::Value,
    headers: &Option<std::collections::HashMap<String, String>>,
) -> Result<String, String> {
    
    let url = format!("{}{}", host, endpoint);
    
    // Map HTTP methods...
    let http_method = match method {
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        _ => reqwest::Method::GET,
    };

    let mut req_builder = client.request(http_method, &url);

    // Apply headers if present
    if let Some(hdrs) = headers {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (k, v) in hdrs {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(val) = reqwest::header::HeaderValue::from_str(v) {
                    header_map.insert(name, val);
                }
            }
        }
        req_builder = req_builder.headers(header_map);
    }

    // Attach body if it's not Null
    if *body != serde_yaml::Value::Null {
        if let Ok(json_body) = serde_json::to_value(body) {
            req_builder = req_builder.json(&json_body);
        }
    }

    // Execute the request
    match req_builder.send().await {
        Ok(res) => {
            let status = res.status();
            // Fetch the raw body string
            let text = res.text().await.unwrap_or_default();
            
            if status.is_success() {
                Ok(text) 
            } else {
                Err(format!("HTTP Status Error: {}", status)) 
            }
        }
        Err(e) => Err(e.to_string()), 
    }
}