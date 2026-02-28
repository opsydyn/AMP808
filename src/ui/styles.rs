use ratatui::style::{Color, Modifier, Style};

/// CLIAMP color palette using standard ANSI terminal colors.
pub const COLOR_TITLE: Color = Color::LightGreen;
pub const COLOR_TEXT: Color = Color::White;
pub const COLOR_DIM: Color = Color::Gray;
pub const COLOR_ACCENT: Color = Color::LightYellow;
pub const COLOR_PLAYING: Color = Color::LightGreen;
pub const COLOR_SEEK_BAR: Color = Color::LightYellow;
pub const COLOR_VOLUME: Color = Color::Green;
pub const COLOR_ERROR: Color = Color::LightRed;

/// Spectrum gradient colors.
pub const SPECTRUM_LOW: Color = Color::LightGreen;
pub const SPECTRUM_MID: Color = Color::LightYellow;
pub const SPECTRUM_HIGH: Color = Color::LightRed;

/// Panel width (usable inner width).
pub const PANEL_WIDTH: u16 = 74;

// Pre-built styles

pub fn title_style() -> Style {
    Style::default()
        .fg(COLOR_TITLE)
        .add_modifier(Modifier::BOLD)
}

pub fn track_style() -> Style {
    Style::default().fg(COLOR_ACCENT)
}

pub fn time_style() -> Style {
    Style::default().fg(COLOR_TEXT)
}

pub fn status_style() -> Style {
    Style::default()
        .fg(COLOR_PLAYING)
        .add_modifier(Modifier::BOLD)
}

pub fn dim_style() -> Style {
    Style::default().fg(COLOR_DIM)
}

pub fn label_style() -> Style {
    Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD)
}

pub fn eq_active_style() -> Style {
    Style::default()
        .fg(COLOR_ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn eq_inactive_style() -> Style {
    Style::default().fg(COLOR_DIM)
}

pub fn playlist_active_style() -> Style {
    Style::default()
        .fg(COLOR_PLAYING)
        .add_modifier(Modifier::BOLD)
}

pub fn playlist_item_style() -> Style {
    Style::default().fg(COLOR_TEXT)
}

pub fn playlist_selected_style() -> Style {
    Style::default()
        .fg(COLOR_ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn help_style() -> Style {
    Style::default().fg(COLOR_DIM)
}

pub fn error_style() -> Style {
    Style::default().fg(COLOR_ERROR)
}

pub fn active_toggle_style() -> Style {
    Style::default()
        .fg(COLOR_ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn seek_fill_style() -> Style {
    Style::default().fg(COLOR_SEEK_BAR)
}

pub fn seek_dim_style() -> Style {
    Style::default().fg(COLOR_DIM)
}

pub fn vol_bar_style() -> Style {
    Style::default().fg(COLOR_VOLUME)
}

/// Spectrum color for a given row height (0.0 to 1.0).
pub fn spectrum_style(row_bottom: f64) -> Style {
    let color = if row_bottom >= 0.6 {
        SPECTRUM_HIGH
    } else if row_bottom >= 0.3 {
        SPECTRUM_MID
    } else {
        SPECTRUM_LOW
    };
    Style::default().fg(color)
}
