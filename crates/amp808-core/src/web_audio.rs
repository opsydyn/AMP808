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
}
