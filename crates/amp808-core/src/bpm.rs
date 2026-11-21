//! Incremental, dependency-free tempo estimation for sampled audio.

use std::collections::VecDeque;

const FRAME_SIZE: usize = 512;
const MAX_ENVELOPE_POINTS: usize = 1_024;
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 190.0;
const MIN_ANALYSIS_POINTS: usize = 96;
const LOCK_TOLERANCE_BPM: i32 = 3;
const LOCK_STREAK_REQUIRED: u8 = 3;

/// The stable tempo information currently available from an analyzer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BpmEstimate {
    /// No sampled signal is available for tempo analysis.
    Unavailable,
    /// More sampled audio is required before a stable tempo can be reported.
    #[default]
    Estimating,
    /// Repeated estimates agreed on this tempo in beats per minute.
    Locked(u16),
}

/// Incrementally estimates tempo from mono PCM windows.
#[derive(Clone, Debug)]
pub struct BpmAnalyzer {
    estimate: BpmEstimate,
    envelope: VecDeque<f64>,
    previous_frame_energy: f64,
    last_candidate: Option<u16>,
    candidate_streak: u8,
}

impl BpmAnalyzer {
    /// Creates an empty analyzer in the estimating state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            estimate: BpmEstimate::Estimating,
            envelope: VecDeque::with_capacity(MAX_ENVELOPE_POINTS),
            previous_frame_energy: 0.0,
            last_candidate: None,
            candidate_streak: 0,
        }
    }

    /// Returns the current stable tempo state.
    #[must_use]
    pub const fn estimate(&self) -> BpmEstimate {
        self.estimate
    }

    /// Discards accumulated samples and returns to the estimating state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Adds mono PCM samples captured at `sample_rate` hertz.
    pub fn update<Sample>(&mut self, samples: &[Sample], sample_rate: u32)
    where
        Sample: Copy + Into<f64>,
    {
        if sample_rate == 0 {
            return;
        }
        self.push_onset_frames(samples);
        if self.envelope.len() < MIN_ANALYSIS_POINTS {
            self.estimate = BpmEstimate::Estimating;
            return;
        }
        let Some(candidate) = estimate_bpm_from_envelope(&self.envelope, sample_rate) else {
            self.estimate = BpmEstimate::Estimating;
            return;
        };
        self.update_lock(candidate);
    }

    fn push_onset_frames<Sample>(&mut self, samples: &[Sample])
    where
        Sample: Copy + Into<f64>,
    {
        for frame in samples
            .chunks(FRAME_SIZE)
            .filter(|frame| frame.len() == FRAME_SIZE)
        {
            let energy = frame
                .iter()
                .map(|sample| {
                    let sample = (*sample).into();
                    sample * sample
                })
                .sum::<f64>()
                / frame.len() as f64;
            let onset = (energy - self.previous_frame_energy).max(0.0);
            self.previous_frame_energy = energy;
            if self.envelope.len() == MAX_ENVELOPE_POINTS {
                self.envelope.pop_front();
            }
            self.envelope.push_back(onset);
        }
    }

    fn update_lock(&mut self, candidate: u16) {
        match self.last_candidate {
            Some(last) if (i32::from(last) - i32::from(candidate)).abs() <= LOCK_TOLERANCE_BPM => {
                self.candidate_streak = self.candidate_streak.saturating_add(1);
            }
            _ => {
                self.last_candidate = Some(candidate);
                self.candidate_streak = 1;
            }
        }
        self.estimate = if self.candidate_streak >= LOCK_STREAK_REQUIRED {
            self.last_candidate = Some(candidate);
            BpmEstimate::Locked(candidate)
        } else {
            BpmEstimate::Estimating
        };
    }
}

impl Default for BpmAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn estimate_bpm_from_envelope(envelope: &VecDeque<f64>, sample_rate: u32) -> Option<u16> {
    if envelope.len() < MIN_ANALYSIS_POINTS || sample_rate == 0 {
        return None;
    }
    let hop_seconds = FRAME_SIZE as f64 / f64::from(sample_rate);
    let minimum_lag = ((60.0 / MAX_BPM) / hop_seconds).round().max(1.0) as usize;
    let maximum_lag = ((60.0 / MIN_BPM) / hop_seconds)
        .round()
        .max(minimum_lag as f64) as usize;
    if envelope.len() <= maximum_lag {
        return None;
    }

    let mut centered: Vec<f64> = envelope.iter().copied().collect();
    let mean = centered.iter().sum::<f64>() / centered.len() as f64;
    for value in &mut centered {
        *value -= mean;
    }

    let mut best_lag = None;
    let mut best_score = 0.0;
    for lag in minimum_lag..=maximum_lag {
        let score = centered
            .iter()
            .zip(centered.iter().skip(lag))
            .map(|(left, right)| left * right)
            .sum::<f64>();
        if score > best_score {
            best_score = score;
            best_lag = Some(lag);
        }
    }
    let lag = best_lag?;
    if best_score <= 0.0 {
        return None;
    }
    let bpm = (60.0 / (lag as f64 * hop_seconds)).round();
    Some(bpm.clamp(MIN_BPM, MAX_BPM) as u16)
}

#[cfg(test)]
mod tests {
    use super::{BpmAnalyzer, BpmEstimate};

    fn synthetic_click_track(bpm: f32, sample_rate: u32, seconds: f32) -> Vec<f32> {
        let sample_count = (sample_rate as f32 * seconds) as usize;
        let beat_interval = ((60.0 / bpm) * sample_rate as f32) as usize;
        let pulse_width = (sample_rate / 200).max(1) as usize;
        let mut samples = vec![0.0; sample_count];

        for beat_start in (0..sample_count).step_by(beat_interval) {
            let beat_end = (beat_start + pulse_width).min(sample_count);
            samples[beat_start..beat_end].fill(1.0);
        }
        samples
    }

    fn feed(analyzer: &mut BpmAnalyzer, samples: &[f32], sample_rate: u32) {
        for chunk in samples.chunks(2_048) {
            analyzer.update(chunk, sample_rate);
        }
    }

    #[test]
    fn stable_pulse_train_locks_near_its_tempo() {
        let mut analyzer = BpmAnalyzer::new();
        let samples = synthetic_click_track(120.0, 44_100, 12.0);

        feed(&mut analyzer, &samples, 44_100);

        let BpmEstimate::Locked(bpm) = analyzer.estimate() else {
            panic!("expected a locked tempo, got {:?}", analyzer.estimate());
        };
        assert!((i32::from(bpm) - 120).abs() <= 2, "locked at {bpm} BPM");
    }

    #[test]
    fn estimator_avoids_octave_substitution_across_supported_tempos() {
        for expected in [90.0, 128.0, 160.0] {
            let mut analyzer = BpmAnalyzer::new();
            let samples = synthetic_click_track(expected, 44_100, 12.0);

            feed(&mut analyzer, &samples, 44_100);

            let BpmEstimate::Locked(actual) = analyzer.estimate() else {
                panic!(
                    "expected {expected} BPM to lock, got {:?}",
                    analyzer.estimate()
                );
            };
            assert!(
                (i32::from(actual) - expected.round() as i32).abs() <= 3,
                "expected about {expected} BPM, locked at {actual} BPM"
            );
        }
    }

    #[test]
    fn reset_discards_the_previous_track_tempo() {
        let mut analyzer = BpmAnalyzer::new();
        let samples = synthetic_click_track(128.0, 44_100, 12.0);
        feed(&mut analyzer, &samples, 44_100);
        assert!(matches!(analyzer.estimate(), BpmEstimate::Locked(_)));

        analyzer.reset();

        assert_eq!(analyzer.estimate(), BpmEstimate::Estimating);
    }
}
