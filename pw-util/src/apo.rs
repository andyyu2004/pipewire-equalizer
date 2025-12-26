use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;

#[derive(Debug, Clone, PartialEq)]
pub enum FilterType {
    Peaking,
    LowShelf,
    HighShelf,
}

impl FilterType {
    pub fn to_pipewire_label(&self) -> &str {
        match self {
            FilterType::Peaking => "bq_peaking",
            FilterType::LowShelf => "bq_lowshelf",
            FilterType::HighShelf => "bq_highshelf",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub number: u32,
    pub enabled: bool,
    pub filter_type: FilterType,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub preamp: Option<f32>,
    pub filters: Vec<Filter>,
}

/// Parse an AutoEQ .apo file
pub async fn parse_file(path: impl AsRef<Path>) -> Result<Config> {
    let content = fs::read_to_string(path.as_ref())
        .await
        .context("Failed to read .apo file")?;

    parse(&content)
}

/// Parse AutoEQ .apo format from a string
pub fn parse(content: &str) -> Result<Config> {
    let mut preamp = None;
    let mut filters = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse preamp line: "Preamp: -1.9 dB"
        if line.starts_with("Preamp:") {
            if let Some(value_str) = line.split(':').nth(1) {
                let value_str = value_str
                    .trim()
                    .trim_end_matches("dB")
                    .trim_end_matches("db")
                    .trim();
                preamp = Some(
                    value_str
                        .parse()
                        .context(format!("Invalid preamp value: {}", value_str))?,
                );
            }
            continue;
        }

        // Parse filter line: "Filter 1: ON PK Fc 46 Hz Gain 0.8 dB Q 2.9"
        if line.starts_with("Filter")
            && let Some(filter) = parse_filter_line(line)?
        {
            filters.push(filter);
        }
    }

    Ok(Config { preamp, filters })
}

fn parse_filter_line(line: &str) -> Result<Option<Filter>> {
    // Split by ':'
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() < 2 {
        return Ok(None);
    }

    // Extract filter number from "Filter 1"
    let number_str = parts[0].trim().trim_start_matches("Filter").trim();
    let number: u32 = number_str
        .parse()
        .context(format!("Invalid filter number: {number_str}"))?;

    // Parse the rest: "ON PK Fc 46 Hz Gain 0.8 dB Q 2.9"
    let params = parts[1].trim();
    let tokens: Vec<&str> = params.split_whitespace().collect();

    // Check if enabled (ON/OFF)
    let enabled = tokens.first().map(|&s| s == "ON").unwrap_or(false);
    if !enabled {
        return Ok(None);
    }

    // Parse filter type (PK, LSC, HSC, etc.)
    let filter_type = match tokens.get(1) {
        Some(&"PK") => FilterType::Peaking,
        Some(&"LSC") | Some(&"LS") => FilterType::LowShelf,
        Some(&"HSC") | Some(&"HS") => FilterType::HighShelf,
        _ => return Ok(None),
    };

    // Parse parameters: Fc 46 Hz Gain 0.8 dB Q 2.9
    let mut freq = 1000.0;
    let mut gain = 0.0;
    let mut q = 1.0;

    let mut i = 2;
    while i < tokens.len() {
        match tokens[i] {
            "Fc" => {
                if let Some(&value_str) = tokens.get(i + 1) {
                    freq = value_str
                        .parse()
                        .context(format!("Invalid frequency: {}", value_str))?;
                    i += 3; // Skip "Fc 46 Hz"
                } else {
                    i += 1;
                }
            }
            "Gain" => {
                if let Some(&value_str) = tokens.get(i + 1) {
                    gain = value_str
                        .parse()
                        .context(format!("Invalid gain: {}", value_str))?;
                    i += 3; // Skip "Gain 0.8 dB"
                } else {
                    i += 1;
                }
            }
            "Q" => {
                if let Some(&value_str) = tokens.get(i + 1) {
                    q = value_str
                        .parse()
                        .context(format!("Invalid Q: {value_str}"))?;
                    i += 2; // Skip "Q 2.9"
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    Ok(Some(Filter {
        number,
        enabled,
        filter_type,
        freq,
        gain,
        q,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filter_line() {
        let line = "Filter 1: ON PK Fc 46 Hz Gain 0.8 dB Q 2.9";
        let filter = parse_filter_line(line).unwrap().unwrap();

        assert_eq!(
            filter,
            Filter {
                number: 1,
                enabled: true,
                filter_type: FilterType::Peaking,
                freq: 46.0,
                gain: 0.8,
                q: 2.9,
            }
        );
    }

    #[test]
    fn test_parse_lowshelf() {
        let line = "Filter 3: ON LSC Fc 105 Hz Gain -0.3 dB Q 0.6666667";
        let filter = parse_filter_line(line).unwrap().unwrap();

        assert_eq!(
            filter,
            Filter {
                number: 3,
                enabled: true,
                filter_type: FilterType::LowShelf,
                freq: 105.0,
                gain: -0.3,
                q: 0.6666667,
            }
        );
    }
}
