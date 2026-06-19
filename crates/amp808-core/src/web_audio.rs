use std::collections::VecDeque;

pub const WEB_BPM_MIN: f64 = 70.0;
pub const WEB_BPM_MAX: f64 = 190.0;
const WEB_BPM_MIN_ANALYSIS_POINTS: usize = 48;
const WEB_BPM_MAX_ENVELOPE_POINTS: usize = 256;
const WEB_BPM_LOCK_TOLERANCE: i32 = 3;
const WEB_BPM_LOCK_STREAK_REQUIRED: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedAudioIssue {
    CorsRequired,
}

impl HostedAudioIssue {
    pub fn user_message(self) -> &'static str {
        match self {
            Self::CorsRequired => "This hosted audio URL must allow CORS for AMP808 web playback.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAudioSource {
    LocalFile { name: String },
    HostedUrl { url: String },
}

impl WebAudioSource {
    pub fn local_file(name: impl Into<String>) -> Self {
        Self::LocalFile { name: name.into() }
    }

    pub fn hosted_url(url: impl Into<String>) -> Self {
        Self::HostedUrl { url: url.into() }
    }

    pub fn is_hosted_url(&self) -> bool {
        matches!(self, Self::HostedUrl { .. })
    }

    pub fn label(&self) -> &str {
        match self {
            Self::LocalFile { name } => name,
            Self::HostedUrl { url } => url,
        }
    }
}

/// Converts analyser byte bins into normalized visual bands.
///
/// The output length always equals `band_count`. Empty input fills the requested
/// bands with zeroes. When there are fewer bins than bands, the nearest
/// available bins are reused to preserve the requested visual band count.
pub fn analyser_bins_to_bands(bins: &[u8], band_count: usize) -> Vec<f32> {
    if band_count == 0 {
        return Vec::new();
    }

    if bins.is_empty() {
        return vec![0.0; band_count];
    }

    let mut bands = Vec::with_capacity(band_count);
    for band in 0..band_count {
        let start = band * bins.len() / band_count;
        let end = ((band + 1) * bins.len() / band_count).max(start + 1);
        let end = end.min(bins.len());
        let slice = &bins[start..end];
        let sum: u32 = slice.iter().map(|value| u32::from(*value)).sum();
        let average = sum as f32 / slice.len() as f32;
        bands.push(average / 255.0);
    }
    bands
}

/// Scales normalized analyser bands into terminal row heights.
///
/// Non-finite values and values below zero render as silence. Values above one
/// are capped at the available visual height.
pub fn analyser_bands_to_heights(bands: &[f32], max_height: u16) -> Vec<u16> {
    bands
        .iter()
        .map(|band| {
            let normalized = if band.is_finite() {
                band.clamp(0.0, 1.0)
            } else {
                0.0
            };
            (normalized * f32::from(max_height)).round() as u16
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebBpmDisplayState {
    Estimating,
    Locked(u16),
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct WebBpmState {
    display: WebBpmDisplayState,
    envelope: VecDeque<f64>,
    previous_energy: Option<f64>,
    last_candidate: Option<u16>,
    stable_candidate_count: u8,
}

impl WebBpmState {
    pub fn unavailable() -> Self {
        Self {
            display: WebBpmDisplayState::Unavailable,
            envelope: VecDeque::with_capacity(WEB_BPM_MAX_ENVELOPE_POINTS),
            previous_energy: None,
            last_candidate: None,
            stable_candidate_count: 0,
        }
    }

    pub fn estimating() -> Self {
        Self {
            display: WebBpmDisplayState::Estimating,
            envelope: VecDeque::with_capacity(WEB_BPM_MAX_ENVELOPE_POINTS),
            previous_energy: None,
            last_candidate: None,
            stable_candidate_count: 0,
        }
    }

    pub fn display_state(&self) -> WebBpmDisplayState {
        self.display
    }

    pub fn update_from_time_domain_bytes(
        &mut self,
        bytes: &[u8],
        hop_seconds: f64,
        is_playing: bool,
    ) {
        if !is_playing {
            self.reset_estimate();
            return;
        }

        if bytes.is_empty() || !hop_seconds.is_finite() || hop_seconds <= 0.0 {
            self.display = WebBpmDisplayState::Estimating;
            return;
        }

        let energy = byte_frame_rms(bytes);
        let onset = self
            .previous_energy
            .map(|previous| (energy - previous).max(0.0))
            .unwrap_or(0.0);
        self.previous_energy = Some(energy);

        if self.envelope.len() == WEB_BPM_MAX_ENVELOPE_POINTS {
            self.envelope.pop_front();
        }
        self.envelope.push_back(onset);

        if let Some(candidate) = estimate_bpm_from_envelope(&self.envelope, hop_seconds) {
            self.accept_candidate(candidate);
        } else if !matches!(self.display, WebBpmDisplayState::Locked(_)) {
            self.display = WebBpmDisplayState::Estimating;
        }
    }

    fn reset_estimate(&mut self) {
        self.display = WebBpmDisplayState::Estimating;
        self.envelope.clear();
        self.previous_energy = None;
        self.last_candidate = None;
        self.stable_candidate_count = 0;
    }

    fn accept_candidate(&mut self, candidate: u16) {
        let stable = self.last_candidate.is_some_and(|last| {
            (i32::from(last) - i32::from(candidate)).abs() <= WEB_BPM_LOCK_TOLERANCE
        });

        self.stable_candidate_count = if stable {
            self.stable_candidate_count
                .saturating_add(1)
                .min(WEB_BPM_LOCK_STREAK_REQUIRED)
        } else {
            1
        };
        self.last_candidate = Some(candidate);

        if self.stable_candidate_count >= WEB_BPM_LOCK_STREAK_REQUIRED {
            self.display = WebBpmDisplayState::Locked(candidate);
        } else {
            self.display = WebBpmDisplayState::Estimating;
        }
    }
}

fn byte_frame_rms(bytes: &[u8]) -> f64 {
    let sum = bytes
        .iter()
        .map(|byte| {
            let sample = (f64::from(*byte) - 128.0) / 128.0;
            sample * sample
        })
        .sum::<f64>();
    (sum / bytes.len() as f64).sqrt()
}

fn estimate_bpm_from_envelope(envelope: &VecDeque<f64>, hop_seconds: f64) -> Option<u16> {
    if envelope.len() < WEB_BPM_MIN_ANALYSIS_POINTS {
        return None;
    }

    let min_lag = ((60.0 / WEB_BPM_MAX) / hop_seconds).ceil().max(1.0) as usize;
    let max_lag = ((60.0 / WEB_BPM_MIN) / hop_seconds)
        .floor()
        .max(min_lag as f64) as usize;
    let max_lag = max_lag.min(envelope.len().saturating_sub(1));
    if min_lag > max_lag {
        return None;
    }

    let values = envelope.iter().copied().collect::<Vec<_>>();
    let mut best_lag = None;
    let mut best_score = 0.0;

    for lag in min_lag..=max_lag {
        let mut score = 0.0;
        for index in lag..values.len() {
            score += values[index] * values[index - lag];
        }
        if score > best_score {
            best_score = score;
            best_lag = Some(lag);
        }
    }

    let lag = best_lag?;
    if best_score <= f64::EPSILON {
        return None;
    }

    let bpm = 60.0 / (lag as f64 * hop_seconds);
    (WEB_BPM_MIN..=WEB_BPM_MAX)
        .contains(&bpm)
        .then(|| bpm.round() as u16)
}

#[cfg(test)]
mod tests {
    use super::{HostedAudioIssue, WebAudioSource, WebBpmDisplayState, WebBpmState};

    #[test]
    fn hosted_url_cors_error_names_amp808_web_playback() {
        let message = HostedAudioIssue::CorsRequired.user_message();

        assert_eq!(
            message,
            "This hosted audio URL must allow CORS for AMP808 web playback."
        );
    }

    #[test]
    fn local_file_source_is_not_hosted() {
        let source = WebAudioSource::local_file("amen-break.wav");

        assert!(!source.is_hosted_url());
        assert_eq!(source.label(), "amen-break.wav");
    }

    #[test]
    fn hosted_url_source_is_hosted_and_uses_url_as_label() {
        let source = WebAudioSource::hosted_url("https://example.com/audio.mp3");

        assert!(source.is_hosted_url());
        assert_eq!(source.label(), "https://example.com/audio.mp3");
    }

    #[test]
    fn analyser_bins_are_averaged_into_normalized_bands() {
        let bins = [0, 64, 128, 255];

        let bands = super::analyser_bins_to_bands(&bins, 2);

        assert_eq!(bands.len(), 2);
        assert!((bands[0] - 0.1254902).abs() < 0.0001);
        assert!((bands[1] - 0.7509804).abs() < 0.0001);
    }

    #[test]
    fn analyser_bins_reuse_nearest_bins_when_more_bands_than_bins() {
        let bins = [0, 255];

        let bands = super::analyser_bins_to_bands(&bins, 4);

        assert_eq!(bands, vec![0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn analyser_bins_return_empty_when_no_bands_requested() {
        let bands = super::analyser_bins_to_bands(&[1, 2], 0);

        assert!(bands.is_empty());
    }

    #[test]
    fn analyser_bins_fill_zeroes_when_input_is_empty() {
        let bands = super::analyser_bins_to_bands(&[], 3);

        assert_eq!(bands, vec![0.0; 3]);
    }

    #[test]
    fn analyser_bands_scale_to_clamped_visual_heights() {
        let bands = [-0.5, 0.0, 0.49, 0.5, 1.0, 1.5, f32::NAN];

        let heights = super::analyser_bands_to_heights(&bands, 8);

        assert_eq!(heights, vec![0, 0, 4, 4, 8, 8, 0]);
    }

    #[test]
    fn web_bpm_locks_on_synthetic_time_domain_pulses() {
        let mut bpm = WebBpmState::estimating();
        let hop_seconds = 0.05;

        for frame in 0..180 {
            let on_beat = frame % 10 == 0;
            let byte = if on_beat { 255 } else { 128 };
            let frame = vec![byte; 512];
            bpm.update_from_time_domain_bytes(&frame, hop_seconds, true);
        }

        assert_eq!(bpm.display_state(), WebBpmDisplayState::Locked(120));
    }

    #[test]
    fn web_bpm_resets_to_estimating_when_playback_stops() {
        let mut bpm = WebBpmState::estimating();
        bpm.update_from_time_domain_bytes(&[255; 512], 0.05, true);
        bpm.update_from_time_domain_bytes(&[128; 512], 0.05, false);

        assert_eq!(bpm.display_state(), WebBpmDisplayState::Estimating);
    }
}
