use std::fmt;
use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisAvailability {
    Available,
    Idle,
    Paused,
    Stale,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalyzerConfig {
    fft_size: usize,
    waveform_points: usize,
    spectrum_bands: usize,
}

impl AnalyzerConfig {
    pub const fn fft_size(self) -> usize {
        self.fft_size
    }
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            fft_size: 1_024,
            waveform_points: 128,
            spectrum_bands: 24,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisSnapshot {
    sequence: u64,
    availability: AnalysisAvailability,
    waveform: Box<[f32]>,
    spectrum: Box<[f32]>,
    peak: f32,
    rms: f32,
}

impl AnalysisSnapshot {
    pub fn empty(
        availability: AnalysisAvailability,
        sequence: u64,
        config: AnalyzerConfig,
    ) -> Self {
        Self {
            sequence,
            availability,
            waveform: vec![0.0; config.waveform_points].into_boxed_slice(),
            spectrum: vec![0.0; config.spectrum_bands].into_boxed_slice(),
            peak: 0.0,
            rms: 0.0,
        }
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn availability(&self) -> AnalysisAvailability {
        self.availability
    }

    pub fn waveform(&self) -> &[f32] {
        &self.waveform
    }

    pub fn spectrum(&self) -> &[f32] {
        &self.spectrum
    }

    pub const fn peak(&self) -> f32 {
        self.peak
    }

    pub const fn rms(&self) -> f32 {
        self.rms
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum AnalysisError {
    InvalidConfig,
    InsufficientSamples { required: usize, actual: usize },
    Fft(String),
}

impl AnalysisError {
    pub const fn required_samples(&self) -> usize {
        match self {
            Self::InsufficientSamples { required, .. } => *required,
            Self::InvalidConfig | Self::Fft(_) => 0,
        }
    }

    pub const fn actual_samples(&self) -> usize {
        match self {
            Self::InsufficientSamples { actual, .. } => *actual,
            Self::InvalidConfig | Self::Fft(_) => 0,
        }
    }
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => formatter.write_str("invalid analyzer configuration"),
            Self::InsufficientSamples { required, actual } => write!(
                formatter,
                "analysis requires {required} samples but received {actual}"
            ),
            Self::Fft(message) => write!(formatter, "FFT processing failed: {message}"),
        }
    }
}

impl std::error::Error for AnalysisError {}

pub struct Analyzer {
    config: AnalyzerConfig,
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl Analyzer {
    pub fn new(config: AnalyzerConfig) -> Result<Self, AnalysisError> {
        if config.fft_size < 2
            || !config.fft_size.is_power_of_two()
            || config.waveform_points == 0
            || config.waveform_points > config.fft_size
            || config.spectrum_bands == 0
        {
            return Err(AnalysisError::InvalidConfig);
        }

        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(config.fft_size);
        let input = fft.make_input_vec();
        let output = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();

        Ok(Self {
            config,
            fft,
            input,
            output,
            scratch,
        })
    }

    pub fn analyze(
        &mut self,
        samples: &[f32],
        sequence: u64,
    ) -> Result<AnalysisSnapshot, AnalysisError> {
        if samples.len() < self.config.fft_size {
            return Err(AnalysisError::InsufficientSamples {
                required: self.config.fft_size,
                actual: samples.len(),
            });
        }

        let window = &samples[samples.len() - self.config.fft_size..];
        let mut peak = 0.0_f32;
        let mut sum_squares = 0.0_f32;
        for sample in window {
            let sample = sample.clamp(-1.0, 1.0);
            peak = peak.max(sample.abs());
            sum_squares += sample * sample;
        }
        let rms = (sum_squares / window.len() as f32).sqrt();

        let waveform = self.waveform(window);
        self.prepare_fft_input(window);
        self.fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .map_err(|error| AnalysisError::Fft(error.to_string()))?;
        let spectrum = self.spectrum();

        Ok(AnalysisSnapshot {
            sequence,
            availability: AnalysisAvailability::Available,
            waveform: waveform.into_boxed_slice(),
            spectrum: spectrum.into_boxed_slice(),
            peak,
            rms,
        })
    }

    fn waveform(&self, samples: &[f32]) -> Vec<f32> {
        let mut waveform = Vec::with_capacity(self.config.waveform_points);
        for point in 0..self.config.waveform_points {
            let start = point * samples.len() / self.config.waveform_points;
            let end = (point + 1) * samples.len() / self.config.waveform_points;
            let value = samples[start..end]
                .iter()
                .map(|sample| sample.clamp(-1.0, 1.0))
                .max_by(|left, right| left.abs().total_cmp(&right.abs()))
                .unwrap_or(0.0);
            waveform.push(value);
        }
        waveform
    }

    fn prepare_fft_input(&mut self, samples: &[f32]) {
        let denominator = (samples.len() - 1) as f32;
        for (index, (destination, sample)) in self.input.iter_mut().zip(samples).enumerate() {
            let hann = 0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / denominator).cos();
            *destination = sample.clamp(-1.0, 1.0) * hann;
        }
    }

    fn spectrum(&self) -> Vec<f32> {
        let max_bin = self.output.len() - 1;
        let mut bands = Vec::with_capacity(self.config.spectrum_bands);

        for band in 0..self.config.spectrum_bands {
            let start = log_bin_boundary(band, self.config.spectrum_bands, max_bin);
            let end = log_bin_boundary(band + 1, self.config.spectrum_bands, max_bin)
                .max(start + 1)
                .min(self.output.len());
            let magnitude = self.output[start..end]
                .iter()
                .map(|value| value.norm())
                .fold(0.0_f32, f32::max);
            bands.push(magnitude);
        }

        let strongest = bands.iter().copied().fold(0.0_f32, f32::max);
        if strongest > 0.0 {
            for band in &mut bands {
                *band = (*band / strongest).clamp(0.0, 1.0);
            }
        }
        bands
    }
}

fn log_bin_boundary(band: usize, band_count: usize, max_bin: usize) -> usize {
    if band == 0 {
        return 1;
    }
    let exponent = band as f32 / band_count as f32;
    (max_bin as f32)
        .powf(exponent)
        .round()
        .clamp(1.0, max_bin as f32) as usize
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::{AnalysisAvailability, AnalysisSnapshot, Analyzer, AnalyzerConfig};

    #[test]
    fn non_signal_states_publish_explicit_empty_snapshots() {
        for availability in [
            AnalysisAvailability::Idle,
            AnalysisAvailability::Paused,
            AnalysisAvailability::Stale,
            AnalysisAvailability::Unavailable,
        ] {
            let snapshot = AnalysisSnapshot::empty(availability, 11, AnalyzerConfig::default());

            assert_eq!(snapshot.availability(), availability);
            assert_eq!(snapshot.sequence(), 11);
            assert!(snapshot.waveform().iter().all(|sample| *sample == 0.0));
            assert!(snapshot.spectrum().iter().all(|sample| *sample == 0.0));
        }
    }

    #[test]
    fn silence_produces_zero_levels_and_no_fabricated_visual_motion() {
        let mut analyzer = Analyzer::new(AnalyzerConfig::default()).unwrap();

        let snapshot = analyzer.analyze(&vec![0.0; 1_024], 7).unwrap();

        assert_eq!(snapshot.sequence(), 7);
        assert_eq!(snapshot.availability(), AnalysisAvailability::Available);
        assert_eq!(snapshot.peak(), 0.0);
        assert_eq!(snapshot.rms(), 0.0);
        assert!(snapshot.waveform().iter().all(|sample| *sample == 0.0));
        assert!(
            snapshot
                .spectrum()
                .iter()
                .all(|magnitude| *magnitude == 0.0)
        );
    }

    #[test]
    fn waveform_and_levels_are_bounded_for_hot_pcm() {
        let mut analyzer = Analyzer::new(AnalyzerConfig::default()).unwrap();
        let samples: Vec<f32> = (0..1_024)
            .map(|index| if index % 2 == 0 { 1.4 } else { -1.4 })
            .collect();

        let snapshot = analyzer.analyze(&samples, 1).unwrap();

        assert_eq!(snapshot.waveform().len(), 128);
        assert_eq!(snapshot.spectrum().len(), 24);
        assert!(
            snapshot
                .waveform()
                .iter()
                .all(|sample| (-1.0..=1.0).contains(sample))
        );
        assert!((snapshot.peak() - 1.0).abs() < f32::EPSILON);
        assert!((snapshot.rms() - 1.0).abs() < 0.000_1);
    }

    #[test]
    fn a_sine_wave_places_energy_in_a_bounded_spectrum_band() {
        let config = AnalyzerConfig::default();
        let mut analyzer = Analyzer::new(config).unwrap();
        let target_bin = 8.0;
        let samples: Vec<f32> = (0..config.fft_size())
            .map(|index| (TAU * target_bin * index as f32 / config.fft_size() as f32).sin())
            .collect();

        let snapshot = analyzer.analyze(&samples, 2).unwrap();
        let strongest = snapshot
            .spectrum()
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| index)
            .unwrap();

        assert!(
            (5..=9).contains(&strongest),
            "strongest band was {strongest}"
        );
        assert!(snapshot.spectrum()[strongest] > 0.5);
        assert!(
            snapshot
                .spectrum()
                .iter()
                .all(|value| (0.0..=1.0).contains(value))
        );
    }

    #[test]
    fn analysis_rejects_an_incomplete_fft_window() {
        let mut analyzer = Analyzer::new(AnalyzerConfig::default()).unwrap();

        let error = analyzer.analyze(&[0.0; 256], 0).unwrap_err();

        assert_eq!(error.required_samples(), 1_024);
        assert_eq!(error.actual_samples(), 256);
    }
}
