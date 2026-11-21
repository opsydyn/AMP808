use amp808_core::bpm::{BpmAnalyzer, BpmEstimate};

pub type BpmDisplayState = BpmEstimate;

#[derive(Clone, Debug)]
pub struct BpmState {
    pub display: BpmDisplayState,
    analyzer: Option<BpmAnalyzer>,
}

impl BpmState {
    pub fn unavailable() -> Self {
        Self {
            display: BpmDisplayState::Unavailable,
            analyzer: None,
        }
    }

    pub fn estimating() -> Self {
        Self {
            display: BpmDisplayState::Estimating,
            analyzer: Some(BpmAnalyzer::new()),
        }
    }

    #[cfg(test)]
    pub fn locked(bpm: u16) -> Self {
        Self {
            display: BpmDisplayState::Locked(bpm),
            analyzer: Some(BpmAnalyzer::new()),
        }
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
        let Some(analyzer) = self.analyzer.as_mut() else {
            return;
        };
        if !is_playing {
            analyzer.reset();
            self.display = BpmDisplayState::Estimating;
            return;
        }
        if is_paused || sample_rate == 0 {
            return;
        }

        analyzer.update(samples, sample_rate);
        self.display = analyzer.estimate();
    }

    pub fn standard_text(&self) -> String {
        if self.analyzer.is_none() {
            return "[BPM --]".to_owned();
        }
        match self.display {
            BpmDisplayState::Unavailable => "[BPM --]".to_owned(),
            BpmDisplayState::Estimating => "[BPM EST]".to_owned(),
            BpmDisplayState::Locked(bpm) => format!("[{bpm} BPM]"),
        }
    }

    #[cfg(test)]
    const fn is_unavailable(&self) -> bool {
        self.analyzer.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::{BpmDisplayState, BpmState};

    fn synthetic_click_track(bpm: f64, sample_rate: u32, seconds: f64) -> Vec<f64> {
        let total_samples = (sample_rate as f64 * seconds) as usize;
        let beat_interval = ((60.0 / bpm) * sample_rate as f64) as usize;
        let pulse_width = (sample_rate / 200).max(1) as usize;
        let mut samples = vec![0.0; total_samples];
        for beat_start in (0..total_samples).step_by(beat_interval) {
            let beat_end = (beat_start + pulse_width).min(total_samples);
            samples[beat_start..beat_end].fill(1.0);
        }
        samples
    }

    fn feed_track(state: &mut BpmState, samples: &[f64], sample_rate: u32) {
        for chunk in samples.chunks(2_048) {
            state.update(chunk, sample_rate, true, false);
        }
    }

    #[test]
    fn bpm_state_is_unavailable_for_music_app() {
        let state = BpmState::for_music_app(true);

        assert!(state.is_unavailable());
        assert_eq!(state.standard_text(), "[BPM --]");
    }

    #[test]
    fn bpm_state_is_estimating_for_local_backend() {
        let state = BpmState::for_music_app(false);

        assert!(!state.is_unavailable());
        assert_eq!(state.display, BpmDisplayState::Estimating);
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

        assert!(state.is_unavailable());
    }

    #[test]
    fn bpm_locks_on_stable_pulse_train() {
        let mut state = BpmState::estimating();
        let samples = synthetic_click_track(120.0, 44_100, 12.0);

        feed_track(&mut state, &samples, 44_100);

        let BpmDisplayState::Locked(bpm) = state.display else {
            panic!("expected locked BPM, got {:?}", state.display);
        };
        assert!((i32::from(bpm) - 120).abs() <= 2, "locked at {bpm} BPM");
    }

    #[test]
    fn bpm_freezes_while_paused() {
        let mut state = BpmState::estimating();
        let samples = synthetic_click_track(120.0, 44_100, 12.0);
        feed_track(&mut state, &samples, 44_100);
        let locked = state.standard_text();

        let more_samples = synthetic_click_track(90.0, 44_100, 4.0);
        for chunk in more_samples.chunks(2_048) {
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
}
