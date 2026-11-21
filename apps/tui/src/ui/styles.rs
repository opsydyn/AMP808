use ratatui::style::{Color, Modifier, Style};

use super::theme::Theme;

/// Panel width (usable inner width).
pub const PANEL_WIDTH: u16 = 74;

/// Derived color palette from the active theme.
/// Stored in App and used by all render functions.
#[derive(Debug, Clone)]
pub struct Palette {
    pub title: Color,
    pub text: Color,
    pub dim: Color,
    pub accent: Color,
    pub playing: Color,
    pub seek_bar: Color,
    pub volume: Color,
    pub error: Color,
    pub spectrum_low: Color,
    pub spectrum_mid: Color,
    pub spectrum_high: Color,
}

impl Palette {
    /// 808 color palette (Roland TR-808 inspired).
    pub fn tr808() -> Self {
        Self {
            title: Color::Rgb(0xF8, 0xA1, 0x25),         // canonical orange
            text: Color::Rgb(0xC9, 0xC9, 0xC9),          // panel grey
            dim: Color::Rgb(0x66, 0x66, 0x66),           // darker grey
            accent: Color::Rgb(0xF8, 0xA1, 0x25),        // canonical orange chrome
            playing: Color::Rgb(0xF8, 0xA1, 0x25),       // canonical orange
            seek_bar: Color::Rgb(0xF8, 0xA1, 0x25),      // canonical orange
            volume: Color::Rgb(0xF8, 0xA1, 0x25),        // canonical orange
            error: Color::Rgb(0xE7, 0x2E, 0x2E),         // canonical red
            spectrum_low: Color::Rgb(0xF1, 0xF8, 0x27),  // canonical yellow
            spectrum_mid: Color::Rgb(0xF8, 0xA1, 0x25),  // canonical orange
            spectrum_high: Color::Rgb(0xE7, 0x2E, 0x2E), // canonical red
        }
    }

    /// Create a palette from a theme.
    pub fn from_theme(theme: &Theme) -> Self {
        if theme.is_default() {
            Self::default()
        } else {
            Self {
                title: theme.accent,
                text: theme.bright_fg,
                dim: theme.fg,
                accent: theme.accent,
                playing: theme.green,
                seek_bar: theme.accent,
                volume: theme.green,
                error: theme.red,
                spectrum_low: theme.green,
                spectrum_mid: theme.yellow,
                spectrum_high: theme.red,
            }
        }
    }

    pub fn title_style(&self) -> Style {
        Style::default().fg(self.title).add_modifier(Modifier::BOLD)
    }

    pub fn track_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    pub fn time_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn status_style(&self) -> Style {
        Style::default()
            .fg(self.playing)
            .add_modifier(Modifier::BOLD)
    }

    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.dim)
    }

    pub fn label_style(&self) -> Style {
        Style::default().fg(self.text).add_modifier(Modifier::BOLD)
    }

    pub fn eq_active_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn eq_inactive_style(&self) -> Style {
        Style::default().fg(self.dim)
    }

    pub fn playlist_active_style(&self) -> Style {
        Style::default()
            .fg(self.playing)
            .add_modifier(Modifier::BOLD)
    }

    pub fn playlist_item_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn playlist_selected_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn help_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn active_toggle_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn seek_fill_style(&self) -> Style {
        Style::default().fg(self.seek_bar)
    }

    pub fn seek_dim_style(&self) -> Style {
        Style::default().fg(self.dim)
    }

    pub fn vol_bar_style(&self) -> Style {
        Style::default().fg(self.volume)
    }

    pub fn spectrum_style(&self, row_bottom: f64) -> Style {
        let color = if row_bottom >= 0.6 {
            self.spectrum_high
        } else if row_bottom >= 0.3 {
            self.spectrum_mid
        } else {
            self.spectrum_low
        };
        Style::default().fg(color)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            title: Color::LightGreen,
            text: Color::White,
            dim: Color::Gray,
            accent: Color::LightYellow,
            playing: Color::LightGreen,
            seek_bar: Color::LightYellow,
            volume: Color::Green,
            error: Color::LightRed,
            spectrum_low: Color::LightGreen,
            spectrum_mid: Color::LightYellow,
            spectrum_high: Color::LightRed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_808_palette_uses_orange_chrome_and_title() {
        let palette = Palette::tr808();

        assert_eq!(palette.title, Color::Rgb(0xF8, 0xA1, 0x25));
        assert_eq!(palette.accent, Color::Rgb(0xF8, 0xA1, 0x25));
        assert_eq!(palette.playing, Color::Rgb(0xF8, 0xA1, 0x25));
        assert_eq!(palette.seek_bar, Color::Rgb(0xF8, 0xA1, 0x25));
        assert_eq!(palette.error, Color::Rgb(0xE7, 0x2E, 0x2E));
        assert_eq!(palette.spectrum_low, Color::Rgb(0xF1, 0xF8, 0x27));
        assert_eq!(palette.spectrum_mid, Color::Rgb(0xF8, 0xA1, 0x25));
        assert_eq!(palette.spectrum_high, Color::Rgb(0xE7, 0x2E, 0x2E));
    }
}
