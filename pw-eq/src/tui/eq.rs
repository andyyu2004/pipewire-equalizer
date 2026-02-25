use std::num::NonZero;

use pw_util::{
    apo::{self, FilterType},
    module::{
        self, Control, Module, ModuleArgs, NodeKind, ParamEqConfig, ParamEqFilter,
        RateAndBiquadCoefficients, RawNodeConfig,
    },
};
use strum::IntoEnumIterator;

use crate::{FilterId, UpdateFilter, filter::Filter};

use super::{Format, Rotation};

#[derive(Clone)]
pub struct Eq {
    pub name: String,
    pub filters: Vec<Filter>,
    pub selected_idx: usize,
    pub max_filters: usize,
    pub preamp: f64, // dB
    pub bypassed: bool,
}

impl Eq {
    // Check if EQ is effectively a no-op (no gain changes and preamp at 0 dB)
    pub fn is_noop(&self) -> bool {
        self.preamp.abs() < f64::EPSILON
            && self.filters.iter().all(|band| {
                band.gain.abs() < f64::EPSILON
                    && !matches!(
                        band.filter_type,
                        FilterType::BandPass
                            | FilterType::Notch
                            | FilterType::HighPass
                            | FilterType::LowPass
                    )
            })
    }

    pub fn new(name: impl Into<String>, filters: impl IntoIterator<Item = Filter>) -> Self {
        let filters = filters.into_iter().collect::<Vec<_>>();
        Self {
            name: name.into(),
            // Set initial preamp to max gain among bands to avoid clipping
            preamp: -filters
                .iter()
                .fold(0.0f64, |acc, band| acc.max(band.gain))
                .max(0.0),
            filters,
            selected_idx: 0,
            max_filters: 31,
            bypassed: false,
        }
    }

    pub fn add_filter(&mut self) {
        if self.filters.len() >= self.max_filters {
            return;
        }

        // Calculate new frequency between current and next band
        let new_freq = match self.filters.len() {
            0 => 100.0,
            len if self.selected_idx + 1 < len => {
                let current_band = &self.filters[self.selected_idx];
                let next_band = &self.filters[self.selected_idx + 1];
                // Geometric mean (better for logarithmic frequency scale)
                (current_band.frequency * next_band.frequency).sqrt()
            }
            _ => (self.filters.last().unwrap().frequency * 20000.0)
                .sqrt()
                .min(20000.0),
        };

        let new_filter = Filter {
            frequency: new_freq,
            gain: 0.0,
            q: 1.0,
            filter_type: FilterType::Peaking,
            muted: false,
        };

        if self.selected_idx < self.filters.len() {
            self.filters.insert(self.selected_idx + 1, new_filter);
        } else {
            self.filters.push(new_filter);
        }

        self.selected_idx += 1;
    }

    pub fn delete_selected_filter(&mut self) {
        if self.filters.len() > 1 {
            self.filters.remove(self.selected_idx);
            if self.selected_idx >= self.filters.len() {
                self.selected_idx = self.filters.len().saturating_sub(1);
            }
        }
    }

    pub fn select_next_filter(&mut self) {
        if self.selected_idx < self.filters.len().saturating_sub(1) {
            self.selected_idx += 1;
        }
    }

    pub fn select_prev_filter(&mut self) {
        self.selected_idx = self.selected_idx.saturating_sub(1);
    }

    pub fn adjust_freq(&mut self, f: impl FnOnce(f64) -> f64) {
        if let Some(band) = self.filters.get_mut(self.selected_idx) {
            band.frequency = f(band.frequency).clamp(20.0, 20000.0);
        }
    }

    pub fn adjust_gain(&mut self, f: impl FnOnce(f64) -> f64) {
        if let Some(band) = self.filters.get_mut(self.selected_idx) {
            band.gain = f(band.gain).clamp(-12.0, 12.0);
        }
    }

    pub fn adjust_q(&mut self, f: impl FnOnce(f64) -> f64) {
        if let Some(band) = self.filters.get_mut(self.selected_idx) {
            band.q = f(band.q).clamp(0.001, 10.0);
        }
    }

    pub fn cycle_filter_type(&mut self, rotation: Rotation) {
        let types = FilterType::iter().collect::<Vec<_>>();
        if let Some(band) = self.filters.get_mut(self.selected_idx) {
            let idx = types
                .iter()
                .position(|&t| t == band.filter_type)
                .expect("filter type must exist in enum");

            band.filter_type = match rotation {
                Rotation::Clockwise => types[(idx + 1) % types.len()],
                Rotation::CounterClockwise => types[(idx + types.len() - 1) % types.len()],
            };
        }
    }

    pub fn toggle_mute(&mut self) {
        if let Some(band) = self.filters.get_mut(self.selected_idx) {
            band.muted = !band.muted;
        }
    }

    pub fn adjust_preamp(&mut self, f: impl FnOnce(f64) -> f64) {
        self.preamp = f(self.preamp).clamp(-12.0, 12.0);
    }

    pub fn toggle_bypass(&mut self) {
        self.bypassed = !self.bypassed;
    }

    pub fn to_module_args(&self, rate: u32) -> ModuleArgs {
        Module::from_kinds(
            &format!("{}-{}", self.name, self.filters.len()),
            self.preamp,
            self.filters.iter().map(|band| NodeKind::Raw {
                config: RawNodeConfig {
                    coefficients: vec![RateAndBiquadCoefficients {
                        rate,
                        coefficients: band.biquad_coeffs(rate as f64),
                    }],
                },
            }),
        )
        .args
    }

    /// Save current EQ configuration to a PipeWire filter-chain config file using param_eq
    pub async fn save_config(
        &self,
        path: impl AsRef<std::path::Path>,
        format: Format,
    ) -> anyhow::Result<()> {
        let path = path.as_ref();
        let data = match format {
            Format::PwParamEq => {
                let config = module::Config::from_kinds(
                    &self.name,
                    self.preamp,
                    [NodeKind::ParamEq {
                        config: ParamEqConfig {
                            filters: self
                                .filters
                                .iter()
                                .map(|band| ParamEqFilter {
                                    ty: band.filter_type,
                                    control: Control {
                                        freq: band.frequency,
                                        q: band.q,
                                        gain: band.gain,
                                    },
                                })
                                .collect(),
                        },
                    }],
                );

                pw_util::to_spa_json(&config)
            }
            Format::Apo => apo::Config {
                preamp: self.preamp,
                filters: self
                    .filters
                    .iter()
                    .enumerate()
                    .map(|(i, filter)| apo::Filter {
                        number: (i + 1) as u32,
                        enabled: !filter.muted,
                        filter_type: filter.filter_type,
                        frequency: filter.frequency,
                        gain: filter.gain,
                        q: filter.q,
                    })
                    .collect(),
            }
            .to_string(),
        };

        if let Some(parent) = path.parent()
            && let Err(err) = tokio::fs::create_dir_all(parent).await
        {
            anyhow::bail!(
                "failed to create parent directories for {}: {err}",
                path.display()
            );
        }

        tokio::fs::write(path, data).await?;

        Ok(())
    }

    /// Build update for preamp
    pub fn build_preamp_update(&self) -> UpdateFilter {
        UpdateFilter {
            frequency: None,
            gain: Some(self.preamp),
            q: None,
            coeffs: None,
        }
    }

    pub fn build_filter_update(&self, filter_idx: usize, sample_rate: u32) -> UpdateFilter {
        // Locally copy the band to modify muted state based on bypass
        // This is necessary to get the correct biquad coefficients
        let mut band = self.filters[filter_idx];
        band.muted |= self.bypassed;
        let gain = if band.muted { 0.0 } else { band.gain };

        UpdateFilter {
            frequency: Some(band.frequency),
            gain: Some(gain),
            q: Some(band.q),
            coeffs: Some(band.biquad_coeffs(sample_rate as f64)),
        }
    }

    /// Generate frequency response curve data for visualization
    /// Returns Vec of (frequency, magnitude_db) pairs
    pub fn frequency_response_curve(&self, num_points: usize, sample_rate: f64) -> Vec<(f64, f64)> {
        // Generate logarithmically spaced frequency points from 20 Hz to 20 kHz
        let log_min = 20_f64.log10();
        let log_max = 20000_f64.log10();

        (0..num_points)
            .map(|i| {
                let t = i as f64 / (num_points - 1) as f64;
                let log_freq = log_min + t * (log_max - log_min);
                let freq = 10_f64.powf(log_freq);

                // Sum magnitude response from all bands
                let total_db: f64 = self
                    .filters
                    .iter()
                    .map(|band| band.magnitude_db_at(freq, sample_rate))
                    .sum();

                (freq, total_db)
            })
            .collect()
    }

    pub fn build_all_updates(&self, sample_rate: u32) -> Vec<(FilterId, UpdateFilter)> {
        let mut updates = Vec::with_capacity(self.filters.len() + 1);

        updates.push((FilterId::Preamp, self.build_preamp_update()));

        for idx in 0..self.filters.len() {
            let id = FilterId::Index(NonZero::new(idx + 1).unwrap());
            updates.push((id, self.build_filter_update(idx, sample_rate)));
        }

        updates
    }
}
