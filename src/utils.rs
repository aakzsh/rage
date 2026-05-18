// src/utils.rs
use std::collections::HashMap;
use std::borrow::Cow;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct CsvDataCache {
    pub headers: Vec<String>,
    pub records: Vec<Vec<String>>,
}

impl CsvDataCache {
    /// Reads and parses a CSV data file exactly once during engine boot.
    pub fn load(path: &str) -> Option<Self> {
        if path == "none" || path.is_empty() {
            return None;
        }

        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        
        // Extract column headers from Row 0
        let headers_line = lines.next()?.ok()?;
        let headers: Vec<String> = headers_line
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        
        let mut records = Vec::new();
        for line in lines {
            if let Ok(row_text) = line {
                if !row_text.trim().is_empty() {
                    let row: Vec<String> = row_text
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect();
                    records.push(row);
                }
            }
        }

        if records.is_empty() { 
            None 
        } else { 
            Some(CsvDataCache { headers, records }) 
        }
    }
}

/// Highly optimized token interpolator.
/// Skips memory allocations completely if the target string contains no variable placeholders.
pub fn resolve_variables<'a>(
    text: &'a str, 
    context: &HashMap<String, String>,
    csv_cache: &Option<Arc<CsvDataCache>>,
    csv_row_index: usize,
) -> Cow<'a, str> {
    // Zero-allocation fallback path
    if !text.contains('$') {
        return Cow::Borrowed(text);
    }

    let mut resolved = text.to_string();

    // 1. Process Runtime Captured Response Tokens ($token)
    for (key, value) in context {
        let placeholder = format!("${}", key);
        if resolved.contains(&placeholder) {
            resolved = resolved.replace(&placeholder, value);
        }
    }

    // 2. Process File-Muxed Dynamic Data Column Targets ($csv.email)
    if let Some(cache) = csv_cache {
        if resolved.contains("$csv.") {
            // Wraps around matrix allocations without crashing if data lines are exhausted
            let row_idx = csv_row_index % cache.records.len();
            let row_data = &cache.records[row_idx];

            for (col_idx, col_name) in cache.headers.iter().enumerate() {
                let placeholder = format!("$csv.{}", col_name);
                if resolved.contains(&placeholder) {
                    if let Some(cell_value) = row_data.get(col_idx) {
                        resolved = resolved.replace(&placeholder, cell_value);
                    }
                }
            }
        }
    }

    Cow::Owned(resolved)
}