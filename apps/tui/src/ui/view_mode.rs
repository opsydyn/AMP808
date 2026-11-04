/// Top-level layout mode for the TUI.
///
/// Replaces the old `mode_808: bool` flag so that exactly one layout is
/// active at a time, eliminating the impossible `808 + Expanded` state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Winamp-style compact single-pane layout (original default).
    #[default]
    Compact,
    /// Multi-pane expanded layout: browser | playlist | art | now-playing + spectrogram.
    Expanded,
    /// Roland TR-808 recreation with animated chrome.
    Drum808,
    /// TR-808 aesthetic applied to the full multi-pane expanded layout.
    Drum808Expanded,
}

impl ViewMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewMode::Compact => "compact",
            ViewMode::Expanded => "expanded",
            ViewMode::Drum808 => "808",
            ViewMode::Drum808Expanded => "808_expanded",
        }
    }
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
