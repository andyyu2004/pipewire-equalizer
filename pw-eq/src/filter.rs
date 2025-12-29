use pw_util::config::{BiquadCoefficients, FilterType};

// EQ Band state
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filter {
    pub frequency: f64,
    pub gain: f64,
    pub q: f64,
    pub filter_type: FilterType,
    pub muted: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            frequency: 1000.0,
            gain: 0.0,
            q: 1.0,
            filter_type: FilterType::Peaking,
            muted: false,
        }
    }
}

impl Filter {
    /// Calculate biquad coefficients based on filter type
    /// Returns normalized (b0, b1, b2, a0, a1, a2) where a0 = 1.0
    /// If muted, calculates with 0 gain (bypass)
    pub fn biquad_coeffs(&self, sample_rate: f64) -> BiquadCoefficients {
        use std::f64::consts::PI;

        let w0 = 2.0 * PI * self.frequency / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * self.q);

        // When muted, use 0 gain (no effect)
        let gain = if self.muted { 0.0 } else { self.gain };
        let a = 10_f64.powf(gain / 40.0); // dB to amplitude

        // These are not identical to pipewire's implementation, but the results are very close.
        // Can copy their implementation directly if exact match is needed.
        // pipewire/spa/plugins/audioconvert/biquad.c
        let (b0, b1, b2, a0, a1, a2) = match self.filter_type {
            FilterType::Peaking => {
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha / a;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::LowShelf => {
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::HighShelf => {
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
                (b0, b1, b2, a0, a1, a2)
            }
        };

        // Normalize by dividing all coefficients by a0
        BiquadCoefficients {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Calculate magnitude response in dB at a given frequency
    pub fn magnitude_db_at(&self, freq: f64, sample_rate: f64) -> f64 {
        // When muted, filter has no effect (0 dB)
        if self.muted {
            return 0.0;
        }

        use std::f64::consts::PI;

        let BiquadCoefficients { b0, b1, b2, a1, a2 } = self.biquad_coeffs(sample_rate);
        let w = 2.0 * PI * freq / sample_rate;

        // Numerator (zeros)
        let re_num = b0 + b1 * w.cos() + b2 * (2.0 * w).cos();
        let im_num = b1 * w.sin() + b2 * (2.0 * w).sin();

        // Denominator (poles)
        let re_den = 1.0 + a1 * w.cos() + a2 * (2.0 * w).cos();
        let im_den = a1 * w.sin() + a2 * (2.0 * w).sin();

        let mag_num = (re_num * re_num + im_num * im_num).sqrt();
        let mag_den = (re_den * re_den + im_den * im_den).sqrt();

        20.0 * (mag_num / mag_den).log10()
    }
}
