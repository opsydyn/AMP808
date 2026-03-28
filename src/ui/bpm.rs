use std::collections::VecDeque;

const FRAME_SIZE: usize = 512;
const MAX_ENVELOPE_POINTS: usize = 1024;
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 190.0;
const MIN_ANALYSIS_POINTS: usize = 96;
const LOCK_TOLERANCE_BPM: i32 = 3;
const LOCK_STREAK_REQUIRED: u8 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BpmDisplayState {
    Estimating,
    Locked(u16),
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct BpmState {
    pub display: BpmDisplayState,
    envelope: VecDeque<f64>,
    prev_frame_energy: f64,
    last_candidate: Option<u16>,
    candidate_streak: u8,
}

impl BpmState {
    pub fn unavailable() -> Self {
        Self {
            display: BpmDisplayState::Unavailable,
            envelope: VecDeque::with_capacity(MAX_ENVELOPE_POINTS),
            prev_frame_energy: 0.0,
            last_candidate: None,
            candidate_streak: 0,
        }
    }

    pub fn estimating() -> Self {
        let mut state = Self::unavailable();
        state.display = BpmDisplayState::Estimating;
        state
    }

    #[allow(dead_code)]
    pub fn locked(bpm: u16) -> Self {
        let mut state = Self::estimating();
        state.display = BpmDisplayState::Locked(bpm);
        state
    }

    pub fn for_music_app(is_music_app: bool) -> Self {
        if is_music_app {
            Self::unavailable()
        } else {
            Self::estimating()
        }
    }

    pub fn reset_for_backend(&mut self, is_music_app: bool) {
        *self = Self::for_music_app(is_music_app);
    }

    pub fn update(&mut self, samples: &[f64], sample_rate: u32, is_playing: bool, is_paused: bool) {
        if matches!(self.display, BpmDisplayState::Unavailable) {
            return;
        }

        if !is_playing {
            self.clear_estimation();
            self.display = BpmDisplayState::Estimating;
            return;
        }

        if is_paused || sample_rate == 0 {
            return;
        }

        self.push_onset_frames(samples);

        if self.envelope.len() < MIN_ANALYSIS_POINTS {
            self.display = BpmDisplayState::Estimating;
            return;
        }

        let Some(candidate) = estimate_bpm_from_envelope(&self.envelope, sample_rate) else {
            self.display = BpmDisplayState::Estimating;
            return;
        };

        self.update_lock(candidate);
    }

    pub fn standard_text(&self) -> String {
        match self.display {
            BpmDisplayState::Estimating => "[BPM EST]".to_string(),
            BpmDisplayState::Locked(bpm) => format!("[{bpm} BPM]"),
            BpmDisplayState::Unavailable => "[BPM --]".to_string(),
        }
    }

    fn clear_estimation(&mut self) {
        self.envelope.clear();
        self.prev_frame_energy = 0.0;
        self.last_candidate = None;
        self.candidate_streak = 0;
    }

    fn push_onset_frames(&mut self, samples: &[f64]) {
        for frame in samples.chunks_exact(FRAME_SIZE) {
            let energy =
                frame.iter().map(|sample| sample * sample).sum::<f64>() / frame.len() as f64;
            let onset = (energy - self.prev_frame_energy).max(0.0);
            self.prev_frame_energy = energy;

            if self.envelope.len() == MAX_ENVELOPE_POINTS {
                self.envelope.pop_front();
            }
            self.envelope.push_back(onset);
        }
    }

    fn update_lock(&mut self, candidate: u16) {
        match self.last_candidate {
            Some(last) if (last as i32 - candidate as i32).abs() <= LOCK_TOLERANCE_BPM => {
                self.candidate_streak = self.candidate_streak.saturating_add(1);
            }
            _ => {
                self.last_candidate = Some(candidate);
                self.candidate_streak = 1;
            }
        }

        if self.candidate_streak >= LOCK_STREAK_REQUIRED {
            self.display = BpmDisplayState::Locked(candidate);
            self.last_candidate = Some(candidate);
        } else {
            self.display = BpmDisplayState::Estimating;
        }
    }
}

fn estimate_bpm_from_envelope(envelope: &VecDeque<f64>, sample_rate: u32) -> Option<u16> {
    if envelope.len() < MIN_ANALYSIS_POINTS || sample_rate == 0 {
        return None;
    }

    let hop_secs = FRAME_SIZE as f64 / sample_rate as f64;
    let min_lag = ((60.0 / MAX_BPM) / hop_secs).round().max(1.0) as usize;
    let max_lag = ((60.0 / MIN_BPM) / hop_secs).round().max(min_lag as f64) as usize;
    if envelope.len() <= max_lag {
        return None;
    }

    let mut centered: Vec<f64> = envelope.iter().copied().collect();
    let mean = centered.iter().sum::<f64>() / centered.len() as f64;
    for value in &mut centered {
        *value -= mean;
    }

    let mut best_lag = None;
    let mut best_score = 0.0;

    for lag in min_lag..=max_lag {
        let score = centered
            .iter()
            .zip(centered.iter().skip(lag))
            .map(|(a, b)| a * b)
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

    let bpm = (60.0 / (lag as f64 * hop_secs)).round();
    Some(bpm.clamp(MIN_BPM, MAX_BPM) as u16)
}

#[cfg(test)]
mod tests {
    use super::{BpmDisplayState, BpmState};
    use crate::player::decode::decode_file;

    fn read_all_samples(path: &str) -> Vec<f64> {
        let (mut source, _) = decode_file(path, 44_100).expect("decode local file");
        let mut frames = vec![[0.0f32; 2]; 2048];
        let mut samples = Vec::new();

        loop {
            let read = source.read(&mut frames);
            if read == 0 {
                break;
            }
            for frame in frames.iter().take(read) {
                samples.push(((frame[0] + frame[1]) / 2.0) as f64);
            }
        }

        samples
    }

    fn synthetic_click_track(bpm: f64, sample_rate: u32, seconds: f64) -> Vec<f64> {
        let total_samples = (sample_rate as f64 * seconds) as usize;
        let beat_interval = ((60.0 / bpm) * sample_rate as f64) as usize;
        let pulse_width = (sample_rate / 200).max(1) as usize;
        let mut samples = vec![0.0; total_samples];

        let mut pos = 0usize;
        while pos < total_samples {
            for sample in samples
                .iter_mut()
                .take((pos + pulse_width).min(total_samples))
                .skip(pos)
            {
                *sample = 1.0;
            }
            pos += beat_interval;
        }

        samples
    }

    fn feed_track(state: &mut BpmState, samples: &[f64], sample_rate: u32) {
        for chunk in samples.chunks(2048) {
            state.update(chunk, sample_rate, true, false);
        }
    }

    #[test]
    fn bpm_state_is_unavailable_for_music_app() {
        assert_eq!(
            BpmState::for_music_app(true).display,
            BpmDisplayState::Unavailable
        );
    }

    #[test]
    fn bpm_state_is_estimating_for_local_backend() {
        assert_eq!(
            BpmState::for_music_app(false).display,
            BpmDisplayState::Estimating
        );
    }

    #[test]
    fn bpm_label_matches_display_state() {
        assert_eq!(BpmState::estimating().standard_text(), "[BPM EST]");
        assert_eq!(BpmState::locked(128).standard_text(), "[128 BPM]");
        assert_eq!(BpmState::unavailable().standard_text(), "[BPM --]");
    }

    #[test]
    fn bpm_stays_unavailable_for_music_app_even_when_updated() {
        let mut state = BpmState::unavailable();
        let samples = synthetic_click_track(120.0, 44_100, 8.0);
        feed_track(&mut state, &samples, 44_100);
        assert_eq!(state.display, BpmDisplayState::Unavailable);
    }

    #[test]
    fn bpm_locks_on_stable_pulse_train() {
        let mut state = BpmState::estimating();
        let samples = synthetic_click_track(120.0, 44_100, 12.0);
        feed_track(&mut state, &samples, 44_100);

        match state.display {
            BpmDisplayState::Locked(bpm) => assert!((bpm as i32 - 120).abs() <= 2),
            other => panic!("expected locked bpm, got {other:?}"),
        }
    }

    #[test]
    fn bpm_locks_on_multiple_tempos() {
        for expected in [90.0, 128.0, 160.0] {
            let mut state = BpmState::estimating();
            let samples = synthetic_click_track(expected, 44_100, 12.0);
            feed_track(&mut state, &samples, 44_100);

            match state.display {
                BpmDisplayState::Locked(bpm) => {
                    assert!(
                        (bpm as i32 - expected.round() as i32).abs() <= 3,
                        "expected about {expected}, got {bpm}"
                    );
                }
                other => panic!("expected locked bpm for {expected}, got {other:?}"),
            }
        }
    }

    #[test]
    fn bpm_freezes_while_paused() {
        let mut state = BpmState::estimating();
        let samples = synthetic_click_track(120.0, 44_100, 12.0);
        feed_track(&mut state, &samples, 44_100);
        let locked = state.standard_text();

        let more_samples = synthetic_click_track(90.0, 44_100, 4.0);
        for chunk in more_samples.chunks(2048) {
            state.update(chunk, 44_100, true, true);
        }

        assert_eq!(state.standard_text(), locked);
    }

    #[test]
    fn bpm_resets_to_estimating_when_not_playing() {
        let mut state = BpmState::estimating();
        let samples = synthetic_click_track(120.0, 44_100, 12.0);
        feed_track(&mut state, &samples, 44_100);
        assert!(matches!(state.display, BpmDisplayState::Locked(_)));

        state.update(&[], 44_100, false, false);
        assert_eq!(state.display, BpmDisplayState::Estimating);
    }

    #[test]
    #[ignore = "manual tuning smoke test; set AMP808_BPM_SMOKE_FILES to comma-separated local audio paths"]
    fn local_media_smoke_prints_estimated_bpms() {
        let paths = std::env::var("AMP808_BPM_SMOKE_FILES")
            .expect("set AMP808_BPM_SMOKE_FILES to comma-separated local audio file paths");

        for path in paths
            .split(',')
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            let mut state = BpmState::estimating();
            let samples = read_all_samples(path);
            feed_track(&mut state, &samples, 44_100);
            eprintln!("{path}: {}", state.standard_text());
            assert!(matches!(
                state.display,
                BpmDisplayState::Locked(_) | BpmDisplayState::Estimating
            ));
        }
    }
}
