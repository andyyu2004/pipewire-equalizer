use anyhow::{Context, Result};
use std::{fmt, path::Path, str::FromStr};
use tokio::fs;

pub use crate::module::FilterType;

#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub number: u32,
    pub enabled: bool,
    pub filter_type: FilterType,
    pub frequency: f64,
    pub gain: f64,
    pub q: f64,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub preamp: f64,
    pub filters: Vec<Filter>,
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Preamp: {:.1} dB", self.preamp)?;
        for filter in &self.filters {
            writeln!(
                f,
                "Filter {}: {} {} Fc {:.1} Hz Gain {:.1} dB Q {:.6}",
                filter.number,
                if filter.enabled { "ON" } else { "OFF" },
                filter.filter_type,
                filter.frequency,
                filter.gain,
                filter.q
            )?;
        }
        Ok(())
    }
}

impl FromStr for Config {
    type Err = anyhow::Error;

    fn from_str(content: &str) -> Result<Self> {
        let mut preamp = 0.0;
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
                    preamp = value_str
                        .parse()
                        .context(format!("Invalid preamp value: {}", value_str))?;
                }
                continue;
            }

            // Parse filter line: "Filter 1: ON PK Fc 46 Hz Gain 0.8 dB Q 2.9"
            if line.starts_with("Filter") {
                let filter = parse_filter_line(line)?;
                filters.push(filter);
            }
        }

        Ok(Config { preamp, filters })
    }
}

impl Config {
    /// Parse an AutoEQ .apo file
    pub async fn parse_file(path: impl AsRef<Path>) -> Result<Config> {
        fs::read_to_string(path.as_ref())
            .await
            .context("Failed to read apo format file")?
            .parse()
    }
}

fn parse_filter_line(line: &str) -> Result<Filter> {
    // Split by ':'
    let parts: Vec<&str> = line.split(':').collect();
    anyhow::ensure!(parts.len() >= 2, "Invalid filter line format: {}", line);

    // Extract filter number from "Filter 1"
    let number_str = parts[0].trim().trim_start_matches("Filter").trim();
    let number: u32 = number_str
        .parse()
        .context(format!("Invalid filter number: {number_str}"))?;

    // Parse the rest: "ON PK Fc 46 Hz Gain 0.8 dB Q 2.9"
    let params = parts[1].trim();
    let tokens: Vec<&str> = params.split_whitespace().collect();

    // Check if enabled (ON/OFF)
    let enabled = match tokens.first() {
        Some(&"ON") => true,
        Some(&"OFF") => false,
        other => anyhow::bail!("Expected ON/OFF, got {:?}", other),
    };

    // Parse filter type - only support second-order filters with Q
    let filter_type = match tokens.get(1) {
        Some(&"LS") => anyhow::bail!("Use LSC instead of LS"),
        Some(&"LP") => anyhow::bail!("Use LPQ instead of LP"),
        Some(&"HP") => anyhow::bail!("Use HPQ instead of HP"),
        Some(&"HS") => anyhow::bail!("Use HSC instead of HS"),
        Some(&"LSC") => FilterType::LowShelf,
        Some(&"LPQ") => FilterType::LowPass,
        Some(&"PK") => FilterType::Peaking,
        Some(&"BP") => FilterType::BandPass,
        Some(&"NO") => FilterType::Notch,
        Some(&"HPQ") => FilterType::HighPass,
        Some(&"HSC") => FilterType::HighShelf,
        Some(other) => anyhow::bail!("unknown filter type: {other}"),
        None => anyhow::bail!("Missing filter type"),
    };

    // Parse parameters: Fc 46 Hz Gain 0.8 dB Q 2.9
    let mut frequency = 1000.0;
    let mut gain = 0.0;
    let mut q = 1.0;

    let mut i = 2;
    while i < tokens.len() {
        match tokens[i] {
            "Fc" => {
                if let Some(&value_str) = tokens.get(i + 1) {
                    frequency = value_str
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

    Ok(Filter {
        number,
        enabled,
        filter_type,
        frequency,
        gain,
        q,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filter_line() {
        let line = "Filter 1: ON PK Fc 46 Hz Gain 0.8 dB Q 2.9";
        let filter = parse_filter_line(line).unwrap();

        assert_eq!(
            filter,
            Filter {
                number: 1,
                enabled: true,
                filter_type: FilterType::Peaking,
                frequency: 46.0,
                gain: 0.8,
                q: 2.9,
            }
        );
    }

    #[test]
    fn test_parse_lowshelf() {
        let line = "Filter 3: ON LSC Fc 105 Hz Gain -0.3 dB Q 0.6666667";
        let filter = parse_filter_line(line).unwrap();

        assert_eq!(
            filter,
            Filter {
                number: 3,
                enabled: true,
                filter_type: FilterType::LowShelf,
                frequency: 105.0,
                gain: -0.3,
                q: 0.6666667,
            }
        );
    }
}
