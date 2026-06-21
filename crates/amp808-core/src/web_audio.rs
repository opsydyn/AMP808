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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserMediaError {
    Aborted,
    Network,
    Decode,
    SourceNotSupported,
    Unknown,
}

impl BrowserMediaError {
    pub fn from_code(code: u16) -> Self {
        match code {
            1 => Self::Aborted,
            2 => Self::Network,
            3 => Self::Decode,
            4 => Self::SourceNotSupported,
            _ => Self::Unknown,
        }
    }

    pub fn user_message(self, is_hosted_url: bool) -> &'static str {
        match (self, is_hosted_url) {
            (Self::Aborted, _) => "Browser audio loading was aborted.",
            (Self::Network, true) => {
                "Hosted audio network load failed. Check the URL and server availability."
            }
            (Self::Network, false) => "Browser could not read this local audio file.",
            (Self::Decode, _) => {
                "Browser could not decode this audio. Try a supported codec or container."
            }
            (Self::SourceNotSupported, true) => {
                "Hosted audio must be a browser-supported media file and allow CORS for AMP808 web playback."
            }
            (Self::SourceNotSupported, false) => {
                "This local audio codec or container is not supported by the browser."
            }
            (Self::Unknown, true) => HostedAudioIssue::CorsRequired.user_message(),
            (Self::Unknown, false) => "Browser could not load this audio file.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebExternalProvider {
    SoundCloud,
    YouTube,
    Bandcamp,
}

impl WebExternalProvider {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::SoundCloud => "SoundCloud",
            Self::YouTube => "YouTube",
            Self::Bandcamp => "Bandcamp",
        }
    }

    pub fn unsupported_static_web_message(self) -> &'static str {
        match self {
            Self::SoundCloud => {
                "SoundCloud page URLs need native AMP808 with yt-dlp; AMP808 Web needs a direct CORS-enabled audio URL."
            }
            Self::YouTube => {
                "YouTube page URLs need native AMP808 with yt-dlp; AMP808 Web needs a direct CORS-enabled audio URL."
            }
            Self::Bandcamp => {
                "Bandcamp page URLs need native AMP808 with yt-dlp; AMP808 Web needs a direct CORS-enabled audio URL."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAudioSourceKind {
    LocalFile,
    DirectMediaUrl,
    ProviderPage(WebExternalProvider),
    HostedUrl,
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

    pub fn kind(&self) -> WebAudioSourceKind {
        match self {
            Self::LocalFile { .. } => WebAudioSourceKind::LocalFile,
            Self::HostedUrl { url } => classify_hosted_audio_url(url),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::LocalFile { name } => name,
            Self::HostedUrl { url } => url,
        }
    }
}

pub fn classify_hosted_audio_url(url: &str) -> WebAudioSourceKind {
    let Some(host) = hosted_url_host(url) else {
        return WebAudioSourceKind::HostedUrl;
    };

    if is_soundcloud_host(&host) {
        return WebAudioSourceKind::ProviderPage(WebExternalProvider::SoundCloud);
    }
    if is_youtube_host(&host) {
        return WebAudioSourceKind::ProviderPage(WebExternalProvider::YouTube);
    }
    if is_bandcamp_host(&host) {
        return WebAudioSourceKind::ProviderPage(WebExternalProvider::Bandcamp);
    }
    if hosted_url_has_direct_media_extension(url) {
        return WebAudioSourceKind::DirectMediaUrl;
    }

    WebAudioSourceKind::HostedUrl
}

fn hosted_url_host(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("//"))
        .unwrap_or(url);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .trim();
    if authority.is_empty() {
        return None;
    }

    Some(
        authority
            .trim_matches(['[', ']'])
            .split(':')
            .next()
            .unwrap_or(authority)
            .trim_end_matches('.')
            .to_ascii_lowercase(),
    )
}

fn is_soundcloud_host(host: &str) -> bool {
    host == "soundcloud.com" || host.ends_with(".soundcloud.com")
}

fn is_youtube_host(host: &str) -> bool {
    matches!(host, "youtu.be" | "youtube.com" | "youtube-nocookie.com")
        || host.ends_with(".youtube.com")
        || host.ends_with(".youtube-nocookie.com")
}

fn is_bandcamp_host(host: &str) -> bool {
    host == "bandcamp.com" || host.ends_with(".bandcamp.com")
}

fn hosted_url_has_direct_media_extension(url: &str) -> bool {
    let path = url
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        ".aac", ".flac", ".m4a", ".m4b", ".mp3", ".oga", ".ogg", ".opus", ".wav", ".webm",
    ]
    .iter()
    .any(|extension| path.ends_with(extension))
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
    smoothed_candidate: Option<f64>,
    stable_candidate_count: u8,
}

impl WebBpmState {
    pub fn unavailable() -> Self {
        Self {
            display: WebBpmDisplayState::Unavailable,
            envelope: VecDeque::with_capacity(WEB_BPM_MAX_ENVELOPE_POINTS),
            previous_energy: None,
            last_candidate: None,
            smoothed_candidate: None,
            stable_candidate_count: 0,
        }
    }

    pub fn estimating() -> Self {
        Self {
            display: WebBpmDisplayState::Estimating,
            envelope: VecDeque::with_capacity(WEB_BPM_MAX_ENVELOPE_POINTS),
            previous_energy: None,
            last_candidate: None,
            smoothed_candidate: None,
            stable_candidate_count: 0,
        }
    }

    pub fn display_state(&self) -> WebBpmDisplayState {
        self.display
    }

    pub fn provisional_bpm(&self) -> Option<u16> {
        match self.display {
            WebBpmDisplayState::Locked(bpm) => Some(bpm),
            WebBpmDisplayState::Estimating => self
                .smoothed_candidate
                .map(|candidate| candidate.round() as u16),
            WebBpmDisplayState::Unavailable => None,
        }
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
        self.smoothed_candidate = None;
        self.stable_candidate_count = 0;
    }

    fn accept_candidate(&mut self, candidate: u16) {
        self.smoothed_candidate = Some(
            self.smoothed_candidate
                .map(|previous| previous.mul_add(0.82, f64::from(candidate) * 0.18))
                .unwrap_or(f64::from(candidate)),
        );

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
    use super::{
        BrowserMediaError, HostedAudioIssue, WebAudioSource, WebBpmDisplayState, WebBpmState,
    };

    #[test]
    fn hosted_url_cors_error_names_amp808_web_playback() {
        let message = HostedAudioIssue::CorsRequired.user_message();

        assert_eq!(
            message,
            "This hosted audio URL must allow CORS for AMP808 web playback."
        );
    }

    #[test]
    fn browser_media_error_messages_distinguish_hosted_url_failures() {
        assert_eq!(
            BrowserMediaError::from_code(2).user_message(true),
            "Hosted audio network load failed. Check the URL and server availability."
        );
        assert_eq!(
            BrowserMediaError::from_code(3).user_message(true),
            "Browser could not decode this audio. Try a supported codec or container."
        );
        assert_eq!(
            BrowserMediaError::from_code(4).user_message(true),
            "Hosted audio must be a browser-supported media file and allow CORS for AMP808 web playback."
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
    fn web_audio_sources_classify_provider_page_urls() {
        let cases = [
            (
                "https://soundcloud.com/artist/track",
                super::WebExternalProvider::SoundCloud,
            ),
            (
                "https://on.soundcloud.com/share",
                super::WebExternalProvider::SoundCloud,
            ),
            (
                "https://www.youtube.com/watch?v=abc",
                super::WebExternalProvider::YouTube,
            ),
            ("https://youtu.be/abc", super::WebExternalProvider::YouTube),
            (
                "https://artist.bandcamp.com/track/song",
                super::WebExternalProvider::Bandcamp,
            ),
        ];

        for (url, provider) in cases {
            assert_eq!(
                WebAudioSource::hosted_url(url).kind(),
                super::WebAudioSourceKind::ProviderPage(provider),
                "{url} should be classified as a provider page"
            );
        }
    }

    #[test]
    fn web_audio_sources_classify_direct_media_urls_and_local_files() {
        assert_eq!(
            WebAudioSource::hosted_url("https://cdn.example.com/audio.MP3?token=123").kind(),
            super::WebAudioSourceKind::DirectMediaUrl
        );
        assert_eq!(
            WebAudioSource::hosted_url("https://example.com/listen/123").kind(),
            super::WebAudioSourceKind::HostedUrl
        );
        assert_eq!(
            WebAudioSource::local_file("private-break.wav").kind(),
            super::WebAudioSourceKind::LocalFile
        );
    }

    #[test]
    fn provider_page_messages_name_native_amp808_and_direct_cors_audio() {
        assert_eq!(
            super::WebExternalProvider::SoundCloud.unsupported_static_web_message(),
            "SoundCloud page URLs need native AMP808 with yt-dlp; AMP808 Web needs a direct CORS-enabled audio URL."
        );
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
    fn web_bpm_exposes_provisional_number_before_locking() {
        let mut bpm = WebBpmState::estimating();
        let hop_seconds = 0.05;

        for frame in 0..48 {
            let on_beat = frame % 10 == 0;
            let byte = if on_beat { 255 } else { 128 };
            let frame = vec![byte; 512];
            bpm.update_from_time_domain_bytes(&frame, hop_seconds, true);
        }

        assert_eq!(bpm.display_state(), WebBpmDisplayState::Estimating);
        assert_eq!(bpm.provisional_bpm(), Some(120));
    }

    #[test]
    fn web_bpm_smooths_provisional_candidate_jumps() {
        let mut bpm = WebBpmState::estimating();

        bpm.accept_candidate(120);
        bpm.accept_candidate(180);

        assert!(
            bpm.provisional_bpm()
                .is_some_and(|candidate| candidate < 140),
            "provisional display should not jump straight to a single noisy candidate"
        );
    }

    #[test]
    fn web_bpm_resets_to_estimating_when_playback_stops() {
        let mut bpm = WebBpmState::estimating();
        bpm.update_from_time_domain_bytes(&[255; 512], 0.05, true);
        bpm.update_from_time_domain_bytes(&[128; 512], 0.05, false);

        assert_eq!(bpm.display_state(), WebBpmDisplayState::Estimating);
    }
}
