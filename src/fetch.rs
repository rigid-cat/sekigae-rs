use reqwest::blocking::Client;
use serde::{Deserialize};
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
struct SheetsResponse {
    values: Vec<Vec<String>>,
}

#[derive(Deserialize, Clone)]
pub struct SeatRange {
    pub start_row: Option<usize>,
    pub end_row: Option<usize>,
    pub start_col: Option<usize>,
    pub end_col: Option<usize>,
}

pub fn parse_targets(pref: &str, rows: usize, cols: usize, seat_preferences: &HashMap<String, SeatRange>) -> Vec<usize> {
    let mut targets = Vec::new();
    if let Some(range) = seat_preferences.get(pref.trim()) {
        for r in range.start_row.unwrap_or(0)..=range.end_row.unwrap_or(0) {
            for c in range.start_col.unwrap_or(0)..=range.end_col.unwrap_or(0) {
                if r < rows && c < cols {
                    targets.push(r * cols + c);
                }
            }
        }
    } else {
        // 座標としてパース、例: "1-2,2-3"
        for coord in pref.split(',') {
            let coord = coord.trim();
            if let Some((r_str, c_str)) = coord.split_once('-') {
                if let (Ok(r), Ok(c)) = (r_str.trim().parse::<usize>(), c_str.trim().parse::<usize>()) {
                    if r > 0 && c > 0 && r <= rows && c <= cols {
                        targets.push((r - 1) * cols + (c - 1));
                    }
                }
            }
        }
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

pub fn fetch_student_preferences(url: &str) -> Result<HashMap<u16, (Vec<u16>, Vec<u16>, String)>, Box<dyn std::error::Error>> {
    let client = Client::new();
    let response: SheetsResponse = client.get(url).send()?.json()?;

    let mut preferences = HashMap::new();

    // Skip header row
    for row in response.values.iter().skip(1) {
        if row.len() < 2 {
            continue;
        }
        let number: u16 = row[1].parse()?;
        let mut close_to = Vec::new();
        let mut avoid = Vec::new();

        // Close to: columns 2-5
        for i in 2..6 {
            if row.len() > i && !row[i].is_empty() {
                for num_str in row[i].split(',') {
                    if let Ok(num) = num_str.trim().parse::<u16>() {
                        close_to.push(num);
                    }
                }
            }
        }

        // Avoid: columns 6-9
        for i in 6..10 {
            if row.len() > i && !row[i].is_empty() {
                for num_str in row[i].split(',') {
                    if let Ok(num) = num_str.trim().parse::<u16>() {
                        avoid.push(num);
                    }
                }
            }
        }

        // Targets: column 0, but we need rows and cols, assume 4x5 for now? Wait, we don't have rows/cols here.
        // For simplicity, parse as coordinates or "前"
        let targets_str = if (row.len() > 10) { row[10].clone() } else { "".parse()? };

        preferences.insert(number, (close_to, avoid, targets_str));
    }

    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch() {
        // Placeholder test - replace with actual URL for testing
        let url = "https://sheets.googleapis.com/v4/spreadsheets/YOUR_SPREADSHEET_ID/values/main?key=YOUR_API_KEY";
        match fetch_student_preferences(url) {
            Ok(_) => println!("Fetch test passed"),
            Err(e) => println!("Fetch test failed: {}", e),
        }
    }
}
