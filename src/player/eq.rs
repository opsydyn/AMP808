/// Center frequencies for the 10-band parametric equalizer.
pub const EQ_FREQS: [f64; 10] = [
    70.0, 180.0, 320.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];

/// Second-order IIR peaking EQ filter (Audio EQ Cookbook).
/// Reads gain from a shared reference, so EQ changes take effect immediately.
pub struct Biquad {
    freq: f64,
    q: f64,
    sr: f64,
    // Per-channel filter state
    x1: [f64; 2],
    x2: [f64; 2],
    y1: [f64; 2],
    y2: [f64; 2],
    // Cached coefficients
    last_gain: f64,
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    inited: bool,
}

impl Biquad {
    pub fn new(freq: f64, q: f64, sr: f64) -> Self {
        Self {
            freq,
            q,
            sr,
            x1: [0.0; 2],
            x2: [0.0; 2],
            y1: [0.0; 2],
            y2: [0.0; 2],
            last_gain: 0.0,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            inited: false,
        }
    }

    /// Recalculate coefficients if gain changed.
    fn calc_coeffs(&mut self, db: f64) {
        if self.inited && db == self.last_gain {
            return;
        }
        self.last_gain = db;
        self.inited = true;

        let a = 10.0_f64.powf(db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * self.freq / self.sr;
        let sin_w0 = w0.sin();
        let cos_w0 = w0.cos();
        let alpha = sin_w0 / (2.0 * self.q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Process stereo samples in-place with the given gain in dB.
    /// Skips processing when gain is near zero.
    pub fn process(&mut self, samples: &mut [[f32; 2]], gain_db: f64) {
        // Skip when gain is effectively zero
        if gain_db > -0.1 && gain_db < 0.1 {
            return;
        }

        self.calc_coeffs(gain_db);

        for sample in samples.iter_mut() {
            #[allow(clippy::needless_range_loop)]
            for ch in 0..2 {
                let x = sample[ch] as f64;
                let y = self.b0 * x + self.b1 * self.x1[ch] + self.b2 * self.x2[ch]
                    - self.a1 * self.y1[ch]
                    - self.a2 * self.y2[ch];
                self.x2[ch] = self.x1[ch];
                self.x1[ch] = x;
                self.y2[ch] = self.y1[ch];
                self.y1[ch] = y;
                sample[ch] = y as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biquad_zero_gain_passthrough() {
        let mut bq = Biquad::new(1000.0, 1.4, 44100.0);
        let mut samples = vec![[0.5f32, -0.5], [0.3, -0.3]];
        let original = samples.clone();
        bq.process(&mut samples, 0.0);
        // Zero gain should pass through unchanged
        assert_eq!(samples, original);
    }

    #[test]
    fn test_biquad_nonzero_gain_modifies() {
        let mut bq = Biquad::new(1000.0, 1.4, 44100.0);
        let mut samples = vec![[0.5f32, -0.5]; 100];
        let original = samples.clone();
        bq.process(&mut samples, 6.0);
        // With +6dB boost, samples should be modified
        assert_ne!(samples, original);
    }

    #[test]
    fn test_biquad_coefficient_caching() {
        let mut bq = Biquad::new(1000.0, 1.4, 44100.0);
        let mut samples = vec![[0.5f32, -0.5]; 10];
        bq.process(&mut samples, 3.0);
        let b0_first = bq.b0;
        bq.process(&mut samples, 3.0);
        // Same gain should reuse coefficients
        assert_eq!(bq.b0, b0_first);
    }

    #[test]
    fn test_eq_freqs_count() {
        assert_eq!(EQ_FREQS.len(), 10);
        assert_eq!(EQ_FREQS[0], 70.0);
        assert_eq!(EQ_FREQS[9], 16000.0);
    }
}
