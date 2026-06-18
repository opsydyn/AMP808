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

#[cfg(test)]
mod tests {
    use super::{HostedAudioIssue, WebAudioSource};

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
}
