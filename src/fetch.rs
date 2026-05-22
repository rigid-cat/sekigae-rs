use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

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

#[derive(Clone, Debug, Default)]
pub struct FetchedStudentPreference {
    pub close_to: Vec<u16>,
    pub avoid: Vec<u16>,
    pub seat_targets_raw: String,
    pub forced_seat_targets_raw: String,
}

#[derive(Deserialize)]
pub struct PreferencesConfig {
    pub preferences_url: String,
    pub seat_preferences: HashMap<String, SeatRange>,
}

pub fn load_preferences_config(path: impl AsRef<Path>) -> Result<PreferencesConfig, String> {
    log::info!("loading preferences config: {}", path.as_ref().display());
    let text = std::fs::read_to_string(path.as_ref()).map_err(|err| {
        format!(
            "{} の読み込みに失敗しました: {}",
            path.as_ref().display(),
            err
        )
    })?;

    serde_json::from_str(&text)
        .map_err(|err| format!("{} の形式が不正です: {}", path.as_ref().display(), err))
}

pub fn parse_targets(
    pref: &str,
    rows: usize,
    cols: usize,
    seat_preferences: &HashMap<String, SeatRange>,
) -> Vec<usize> {
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
            if let Some((r_str, c_str)) = coord.split_once('-')
                && let (Ok(r), Ok(c)) =
                    (r_str.trim().parse::<usize>(), c_str.trim().parse::<usize>())
                && r > 0
                && c > 0
                && r <= rows
                && c <= cols
            {
                targets.push((r - 1) * cols + (c - 1));
            }
        }
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

fn parse_number_columns(row: &[String], range: std::ops::Range<usize>) -> Vec<u16> {
    let start = range.start;
    range
        .filter_map(|i| row.get(i).map(|value| (i, value)))
        .flat_map(|(i, value)| {
            let tens_place = ((i - start) as u16) * 10;
            value
                .split(',')
                .filter_map(move |num_str| num_str.trim().parse::<u16>().ok())
                .map(move |num| tens_place + num)
        })
        .collect()
}

pub fn fetch_student_preferences(
    url: &str,
) -> Result<HashMap<u16, FetchedStudentPreference>, Box<dyn std::error::Error>> {
    log::info!("fetching student preferences");
    let client = Client::new();
    let response: SheetsResponse = client.get(url).send()?.json()?;

    let mut preferences = HashMap::new();

    // Skip header row
    for row in response.values.iter().skip(1) {
        if row.len() < 2 {
            continue;
        }
        let number: u16 = row[1].parse()?;
        let close_to = parse_number_columns(row, 2..6);
        let avoid = parse_number_columns(row, 6..10);

        let seat_targets_raw = row.get(10).cloned().unwrap_or_default();
        let forced_seat_targets_raw = row.get(11).cloned().unwrap_or_default();

        preferences.insert(
            number,
            FetchedStudentPreference {
                close_to,
                avoid,
                seat_targets_raw,
                forced_seat_targets_raw,
            },
        );
    }

    log::info!("fetched student preferences: {} rows", preferences.len());
    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_targets_supports_named_range_and_coordinates() {
        let mut seat_preferences = HashMap::new();
        seat_preferences.insert(
            "前".to_string(),
            SeatRange {
                start_row: Some(0),
                end_row: Some(1),
                start_col: Some(0),
                end_col: Some(2),
            },
        );

        let named = parse_targets("前", 4, 5, &seat_preferences);
        assert_eq!(named, vec![0, 1, 2, 5, 6, 7]);

        let coords = parse_targets("1-1,2-3", 4, 5, &seat_preferences);
        assert_eq!(coords, vec![0, 7]);
    }

    #[test]
    fn fetched_student_preference_defaults_to_empty_raw_targets() {
        let pref = FetchedStudentPreference::default();
        assert!(pref.close_to.is_empty());
        assert!(pref.avoid.is_empty());
        assert!(pref.seat_targets_raw.is_empty());
        assert!(pref.forced_seat_targets_raw.is_empty());
    }
}
