use std::f32::consts::TAU;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, FrameExt as _, Paragraph,
    canvas::{Canvas, Line as CanvasLine},
};
use tachyonfx::{EffectRenderer, Interpolation, fx};
use tui_big_text::{BigText, PixelSize};

use super::App;
use super::keys::Focus;
use super::styles::Palette;
use super::visualizer::{SpectrumSegment, SpectrumSegmentKind, VisMode};

/// 808 color constants.
const C808_RED: Color = Color::Rgb(0xD7, 0x26, 0x2E);
const C808_ORANGE: Color = Color::Rgb(0xF0, 0x5A, 0x28);
const C808_AMBER: Color = Color::Rgb(0xF6, 0xA6, 0x23);
const C808_YELLOW: Color = Color::Rgb(0xFF, 0xD4, 0x00);
const C808_GREY: Color = Color::Rgb(0xC9, 0xC9, 0xC9);
const C808_DIM: Color = Color::Rgb(0x66, 0x66, 0x66);
const C808_DEEP_AMBER: Color = Color::Rgb(0xB8, 0x6A, 0x1F);
const C808_SUNSET_ORANGE: Color = Color::Rgb(0xFF, 0x7A, 0x45);
const C808_HOT_PINK: Color = Color::Rgb(0xFF, 0x4D, 0xB8);
const C808_IVORY: Color = Color::Rgb(0xEE, 0xEA, 0xD8);
const C808_BLACK: Color = Color::Rgb(0x12, 0x12, 0x12);

/// EQ frequency labels matching the 10-band EQ.
const EQ_LABELS: [&str; 10] = [
    "70", "180", "320", "600", "1k", "3k", "6k", "12k", "14k", "16k",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderMode808 {
    HorizontalBars,
    LedColumns,
    Retro,
    Oscilloscope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChromeTransportState {
    Stopped,
    Paused,
    Playing,
    Buffering,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChromeFxSignature {
    transport: ChromeTransportState,
    focus: Focus,
    mode: VisMode,
}

#[derive(Clone, Copy, Debug)]
struct PanelTraceConfig {
    cycle_ms: u32,
    tail_ratio: f32,
    head_mix: f32,
    tail_mix: f32,
}

#[derive(Clone, Copy, Debug)]
struct HeaderAccentConfig {
    cycle_ms: u32,
    amplitude: f32,
}

#[derive(Clone, Copy, Debug)]
struct PanelTraceState {
    base: Color,
    head: Color,
    tail: Color,
    config: PanelTraceConfig,
}

#[derive(Clone, Copy, Debug)]
struct HeaderAccentState {
    dim: Color,
    grey: Color,
    accent: Color,
    warm: Color,
    config: HeaderAccentConfig,
}

impl ChromeFxSignature {
    fn panel_trace_config(self) -> Option<PanelTraceConfig> {
        let transport = match self.transport {
            ChromeTransportState::Playing => 1.0_f32,
            ChromeTransportState::Buffering => 0.78_f32,
            ChromeTransportState::Paused => 0.42_f32,
            ChromeTransportState::Stopped => 0.18_f32,
            ChromeTransportState::Error => 0.0_f32,
        };
        let mode = match self.mode {
            VisMode::Scope => 1.0_f32,
            VisMode::Retro => 0.52_f32,
            VisMode::Bars | VisMode::BarsGap | VisMode::VBars | VisMode::Bricks => 0.74_f32,
        };
        let focus = match self.focus {
            Focus::EQ => 0.82_f32,
            Focus::Playlist => 0.68_f32,
            Focus::Provider => 0.38_f32,
            Focus::Browser => 0.22_f32,
        };
        let intensity = (transport * mode * focus).clamp(0.0_f32, 1.0_f32);
        if intensity < 0.12 {
            return None;
        }

        Some(PanelTraceConfig {
            cycle_ms: (9800.0_f32 - intensity * 3600.0_f32).round() as u32,
            tail_ratio: 0.12_f32 + intensity * 0.08_f32,
            head_mix: 0.36_f32 + intensity * 0.38_f32,
            tail_mix: 0.14_f32 + intensity * 0.16_f32,
        })
    }

    fn header_accent_config(self) -> Option<HeaderAccentConfig> {
        let transport = match self.transport {
            ChromeTransportState::Playing => 0.34_f32,
            ChromeTransportState::Buffering => 0.28_f32,
            ChromeTransportState::Paused => 0.18_f32,
            ChromeTransportState::Stopped => 0.08_f32,
            ChromeTransportState::Error => 0.0_f32,
        };
        let mode = match self.mode {
            VisMode::Scope => 1.0_f32,
            VisMode::Retro => 0.58_f32,
            VisMode::Bars | VisMode::BarsGap | VisMode::VBars | VisMode::Bricks => 0.78_f32,
        };
        let amplitude = (transport * mode).clamp(0.0_f32, 0.34_f32);
        if amplitude < 0.07 {
            return None;
        }

        Some(HeaderAccentConfig {
            cycle_ms: (7600.0_f32 - amplitude * 1800.0_f32).round() as u32,
            amplitude,
        })
    }

    fn seek_pulse_strength(self) -> f32 {
        let transport = match self.transport {
            ChromeTransportState::Playing => 0.24_f32,
            ChromeTransportState::Buffering => 0.18_f32,
            ChromeTransportState::Paused => 0.08_f32,
            ChromeTransportState::Stopped | ChromeTransportState::Error => 0.0_f32,
        };
        let mode = match self.mode {
            VisMode::Scope => 1.0_f32,
            VisMode::Retro => 0.55_f32,
            VisMode::Bars | VisMode::BarsGap | VisMode::VBars | VisMode::Bricks => 0.8_f32,
        };
        (transport * mode).clamp(0.0_f32, 0.26_f32)
    }

    fn row_lift_strength(self) -> f32 {
        let base = match self.focus {
            Focus::Playlist => 0.16_f32,
            Focus::Provider => 0.14_f32,
            Focus::Browser | Focus::EQ => 0.0_f32,
        };
        let transport = match self.transport {
            ChromeTransportState::Playing | ChromeTransportState::Buffering => 1.0_f32,
            ChromeTransportState::Paused => 0.82_f32,
            ChromeTransportState::Stopped => 0.72_f32,
            ChromeTransportState::Error => 0.5_f32,
        };
        (base * transport).clamp(0.0_f32, 0.18_f32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Classic808Colors {
    themed: bool,
    red: Color,
    orange: Color,
    amber: Color,
    yellow: Color,
    grey: Color,
    dim: Color,
    deep_amber: Color,
    sunset_orange: Color,
    hot_pink: Color,
    ivory: Color,
    black: Color,
}

impl Classic808Colors {
    fn classic() -> Self {
        Self {
            themed: false,
            red: C808_RED,
            orange: C808_ORANGE,
            amber: C808_AMBER,
            yellow: C808_YELLOW,
            grey: C808_GREY,
            dim: C808_DIM,
            deep_amber: C808_DEEP_AMBER,
            sunset_orange: C808_SUNSET_ORANGE,
            hot_pink: C808_HOT_PINK,
            ivory: C808_IVORY,
            black: C808_BLACK,
        }
    }

    fn themed(palette: &Palette) -> Self {
        Self {
            themed: true,
            red: palette.error,
            orange: palette.playing,
            amber: palette.seek_bar,
            yellow: palette.accent,
            grey: palette.text,
            dim: palette.dim,
            deep_amber: mix_rgb(palette.seek_bar, palette.dim, 0.45),
            sunset_orange: mix_rgb(palette.spectrum_mid, palette.accent, 0.35),
            hot_pink: mix_rgb(palette.spectrum_high, palette.accent, 0.45),
            ivory: mix_rgb(palette.text, palette.accent, 0.15),
            black: C808_BLACK,
        }
    }

    fn header_accent(self) -> Color {
        if self.themed {
            mix_rgb(self.grey, self.yellow, 0.42)
        } else {
            mix_rgb(self.amber, self.yellow, 0.32)
        }
    }

    fn header_warm(self) -> Color {
        if self.themed {
            mix_rgb(self.dim, self.yellow, 0.24)
        } else {
            mix_rgb(self.deep_amber, self.amber, 0.58)
        }
    }

    fn panel_trace_head(self) -> Color {
        if self.themed {
            mix_rgb(self.grey, self.yellow, 0.58)
        } else {
            mix_rgb(self.amber, self.yellow, 0.28)
        }
    }

    fn panel_trace_tail(self) -> Color {
        if self.themed {
            mix_rgb(self.dim, self.yellow, 0.32)
        } else {
            mix_rgb(self.deep_amber, self.amber, 0.46)
        }
    }

    fn seek_tail_color(self, pulse: f32) -> Color {
        if self.themed {
            mix_rgb(self.dim, self.yellow, 0.18 + pulse * 0.18)
        } else {
            mix_rgb(self.amber, self.orange, 0.24 + pulse * 0.32)
        }
    }

    fn seek_dot_color(self, pulse: f32) -> Color {
        if self.themed {
            mix_rgb(self.grey, self.yellow, 0.48 + pulse * 0.24)
        } else {
            mix_rgb(self.amber, self.yellow, 0.36 + pulse * 0.46)
        }
    }

    fn row_lift_target(self) -> Color {
        if self.themed {
            mix_rgb(self.grey, self.yellow, 0.24)
        } else {
            self.ivory
        }
    }
}

fn mix_rgb(a: Color, b: Color, ratio_to_b: f32) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let t = ratio_to_b.clamp(0.0, 1.0);
            let lerp = |lhs: u8, rhs: u8| -> u8 {
                ((lhs as f32 * (1.0 - t)) + (rhs as f32 * t)).round() as u8
            };
            Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
        }
        _ => a,
    }
}

fn content_height_808(browser_focus: bool, retro_mode: bool, terminal_height: u16) -> u16 {
    let target = if retro_mode {
        if browser_focus { 34 } else { 31 }
    } else if browser_focus {
        31
    } else {
        28
    };
    target.min(terminal_height)
}

fn browser_panel_min_height_808(browser_focus: bool) -> u16 {
    if browser_focus { 6 } else { 3 }
}

impl App {
    fn colors_808(&self) -> Classic808Colors {
        if self.theme_idx.is_some() {
            Classic808Colors::themed(&self.palette)
        } else {
            Classic808Colors::classic()
        }
    }

    fn chrome_signature_808(&self) -> ChromeFxSignature {
        ChromeFxSignature {
            transport: chrome_transport_state(
                self.buffering,
                self.err.is_some(),
                self.player.is_playing(),
                self.player.is_paused(),
            ),
            focus: self.focus,
            mode: self.vis.mode,
        }
    }

    /// Render the 808 layout (called when mode_808 == true).
    pub fn render_808(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let browser_focus = self.focus == Focus::Browser;
        let retro_mode = self.vis.mode == VisMode::Retro;

        // Content sizing — centre both horizontally and vertically
        let content_width = 80u16.min(area.width);
        let content_height = content_height_808(browser_focus, retro_mode, area.height);
        let spectrum_height = if retro_mode { 8u16 } else { 5u16 };
        let x = area.width.saturating_sub(content_width) / 2;
        let y = area.height.saturating_sub(content_height) / 2;
        let inner = Rect::new(x, y, content_width, content_height);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),               // header (large-style title)
                Constraint::Length(3),               // knob row 1 (vol + 5 EQ)
                Constraint::Length(3),               // knob row 2 (5 EQ)
                Constraint::Length(1),               // spacer
                Constraint::Length(2),               // now playing + seek
                Constraint::Length(1),               // status line (repeat/shuffle/mono)
                Constraint::Length(1),               // spacer
                Constraint::Length(spectrum_height), // spectrum LED
                Constraint::Length(0),               // spacer
                Constraint::Min(browser_panel_min_height_808(browser_focus)), // playlist/browser
                Constraint::Length(2),               // help controls
                Constraint::Length(1),               // error/save
            ])
            .split(inner);

        self.render_808_header(frame, chunks[0]);
        self.render_808_knob_row1(frame, chunks[1]);
        self.render_808_knob_row2(frame, chunks[2]);
        self.render_808_now_playing(frame, chunks[4]);
        self.render_808_status(frame, chunks[5]);

        // Split spectrum area for album art when available
        if self.show_cover_art && self.cover_art_proto.is_some() {
            let spec_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40), Constraint::Length(24)])
                .split(chunks[7]);
            self.render_808_spectrum(frame, spec_chunks[0]);
            self.render_cover_art(frame, spec_chunks[1]);
        } else {
            self.render_808_spectrum(frame, chunks[7]);
        }
        if self.focus == Focus::Provider {
            self.render_808_provider(frame, chunks[9]);
        } else if self.focus == Focus::Browser {
            self.render_808_browser(frame, chunks[9]);
        } else {
            self.render_808_playlist(frame, chunks[9]);
        }
        self.render_808_help(frame, chunks[10]);

        if chunks.len() > 11 {
            self.render_808_status_line(frame, chunks[11]);
        }

        let header_area = Rect::new(
            inner.x,
            chunks[0].y,
            inner.width,
            chunks[2].bottom().saturating_sub(chunks[0].y),
        );
        let focus_area = if self.focus == Focus::EQ {
            Rect::new(
                chunks[1].x,
                chunks[1].y,
                chunks[1].width,
                chunks[2].bottom().saturating_sub(chunks[1].y),
            )
        } else {
            chunks[9]
        };
        self.render_808_chrome(frame, inner, header_area, focus_area);
    }

    fn render_808_chrome(
        &mut self,
        frame: &mut Frame,
        outer_area: Rect,
        header_area: Rect,
        focus_area: Rect,
    ) {
        let colors = self.colors_808();
        let chrome = self.chrome_signature_808();
        self.ensure_808_effects(chrome);

        let base = Style::default().fg(colors.dim);
        if outer_area.width >= 2 && outer_area.height >= 2 {
            frame.render_widget(
                Block::default().borders(Borders::ALL).border_style(base),
                outer_area,
            );
        }
        if header_area.width >= 2 && header_area.height >= 2 {
            frame.render_widget(
                Block::default().borders(Borders::ALL).border_style(base),
                header_area,
            );
        }
        if focus_area.width >= 2 && focus_area.height >= 2 {
            frame.render_widget(
                Block::default().borders(Borders::ALL).border_style(base),
                focus_area,
            );
        }

        let now = std::time::Instant::now();
        let elapsed = now.saturating_duration_since(self.fx_last_frame);
        self.fx_last_frame = now;
        let tick_ms = elapsed.as_millis().clamp(16, 120) as u32;
        let tick = tachyonfx::Duration::from_millis(tick_ms);

        if let Some(effect) = self.fx_808_header.as_mut() {
            frame.render_effect(effect, header_area, tick);
        }
        if let Some(effect) = self.fx_808_panel.as_mut() {
            frame.render_effect(effect, focus_area, tick);
        }
    }

    fn ensure_808_effects(&mut self, chrome: ChromeFxSignature) {
        let colors = self.colors_808();
        if self.fx_808_signature != Some(chrome) {
            self.fx_808_header = make_808_header_effect(colors, chrome);
            self.fx_808_panel = make_808_panel_effect(colors, chrome);
            self.fx_808_signature = Some(chrome);
        }
    }

    fn render_808_header(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // top divider
                Constraint::Length(3), // big text
                Constraint::Length(1), // subtitle
                Constraint::Length(1), // bottom divider
            ])
            .split(area);

        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
                Style::default().fg(colors.dim),
            ))),
            chunks[0],
        );

        let logo = BigText::builder()
            .pixel_size(PixelSize::ThirdHeight)
            .style(
                Style::default()
                    .fg(colors.yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .centered()
            .lines(vec![Line::from("TR-808")])
            .build();
        frame.render_widget(logo, chunks[1]);

        let subtitle = Line::from(vec![
            Span::styled("  ▬▬▬  ", Style::default().fg(colors.grey)),
            Span::styled("SOFTWARE RHYTHM COMPOSER", Style::default().fg(colors.grey)),
            Span::styled("  ▬▬▬   ", Style::default().fg(colors.grey)),
            Span::styled("Computer Controlled", Style::default().fg(colors.grey)),
        ]);
        frame.render_widget(
            Paragraph::new(subtitle).alignment(Alignment::Center),
            chunks[2],
        );

        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
                Style::default().fg(colors.dim),
            ))),
            chunks[3],
        );
    }

    fn render_808_knob_row1(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        // VOL knob + first 5 EQ bands
        let knob_w = area.width / 6;
        let constraints: Vec<Constraint> = (0..6).map(|_| Constraint::Length(knob_w)).collect();

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        // Volume knob
        let vol = self.player.volume();
        let vol_norm = ((vol + 30.0) / 36.0).clamp(0.0, 1.0);
        let is_focused = self.focus == Focus::EQ; // vol doesn't have its own focus
        render_knob(frame, cols[0], vol_norm, "VOL", false, colors);

        // EQ bands 0-4
        let bands = self.player.eq_bands();
        for i in 0..5 {
            let norm = ((bands[i] + 12.0) / 24.0).clamp(0.0, 1.0);
            let selected = is_focused && self.eq_cursor == i;
            render_knob(frame, cols[i + 1], norm, EQ_LABELS[i], selected, colors);
        }
    }

    fn render_808_knob_row2(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        // EQ bands 5-9 + empty slot
        let knob_w = area.width / 6;
        let constraints: Vec<Constraint> = (0..6).map(|_| Constraint::Length(knob_w)).collect();

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        let bands = self.player.eq_bands();
        let is_focused = self.focus == Focus::EQ;

        for i in 0..5 {
            let band_idx = i + 5;
            let norm = ((bands[band_idx] + 12.0) / 24.0).clamp(0.0, 1.0);
            let selected = is_focused && self.eq_cursor == band_idx;
            render_knob(frame, cols[i], norm, EQ_LABELS[band_idx], selected, colors);
        }

        // Preset indicator in the last slot
        let preset_name = self.eq_preset_name();
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("[{preset_name}]"),
                Style::default().fg(colors.dim),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), cols[5]);
    }

    fn render_808_now_playing(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        let chrome = self.chrome_signature_808();
        let (pos_secs, dur_secs) = self.track_position();
        let pos_min = pos_secs / 60;
        let pos_sec = pos_secs % 60;

        let is_stream = self
            .playlist
            .current()
            .map(|(t, _)| t.stream)
            .unwrap_or(false);

        // Track name
        let name = if !self.stream_title.is_empty() {
            self.stream_title.clone()
        } else if let Some((track, _)) = self.playlist.current() {
            track.display_name()
        } else {
            "No track loaded".to_string()
        };

        let status_label = playback_state_label(
            self.buffering,
            self.err.is_some(),
            self.player.is_playing(),
            self.player.is_paused(),
        );

        let status_prefix = match status_label {
            "BUFFERING" => "◌ BUFFERING",
            "ERROR" => "! ERROR",
            "PAUSED" => "⏸ PAUSED",
            "PLAYING" if self.player.is_streaming() => "● PLAYING",
            "PLAYING" => "▶ PLAYING",
            _ => "■ STOPPED",
        };

        let time_str = if is_stream && dur_secs == 0 {
            format!("{pos_min:02}:{pos_sec:02}/--:--")
        } else {
            let dur_min = dur_secs / 60;
            let dur_sec = dur_secs % 60;
            format!("{pos_min:02}:{pos_sec:02}/{dur_min:02}:{dur_sec:02}")
        };
        let max_name = (area.width as usize).saturating_sub(time_str.len() + 5);
        let display_name: String = if name.chars().count() > max_name {
            let mut s: String = name.chars().take(max_name.saturating_sub(1)).collect();
            s.push('…');
            s
        } else {
            name
        };

        let transport_color = transport_accent_color_808(colors, chrome.transport);
        let mut transport_style = Style::default().fg(transport_color);
        if matches!(
            chrome.transport,
            ChromeTransportState::Playing | ChromeTransportState::Buffering
        ) {
            transport_style = transport_style.add_modifier(Modifier::BOLD);
        }
        let name_span = Span::styled(format!(" {status_prefix} {display_name}"), transport_style);
        let gap = area
            .width
            .saturating_sub(name_span.width() as u16 + time_str.len() as u16 + 1);
        let spaces = Span::raw(" ".repeat(gap as usize));
        let time_span = Span::styled(format!("{time_str} "), Style::default().fg(colors.grey));

        let track_line = Line::from(vec![name_span, spaces, time_span]);

        // Seek bar
        let w = area.width as usize;
        let seek_line = if self.player.is_streaming() {
            let label = "━━━ STREAMING ━━━";
            let pad = w.saturating_sub(label.len() + 1) / 2;
            let bar = format!(
                " {}{}{}",
                "━".repeat(pad),
                label,
                "━".repeat(w.saturating_sub(pad + label.len() + 1))
            );
            Line::from(Span::styled(bar, Style::default().fg(colors.dim)))
        } else {
            let progress = if dur_secs > 0 {
                (pos_secs as f64 / dur_secs as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let filled = (progress * (w.saturating_sub(2)) as f64) as usize;
            let pulse = seek_dot_pulse_808(self.title_off, chrome);
            let warm_tail = filled.min(4);
            let base_fill = filled.saturating_sub(warm_tail);
            let warm_color = colors.seek_tail_color(pulse);
            let dot_color = colors.seek_dot_color(pulse);
            let mut dot_style = Style::default().fg(dot_color);
            if pulse > 0.12 {
                dot_style = dot_style.add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(" ", Style::default()),
                Span::styled("━".repeat(base_fill), Style::default().fg(colors.amber)),
                Span::styled("━".repeat(warm_tail), Style::default().fg(warm_color)),
                Span::styled("●", dot_style),
                Span::styled(
                    "━".repeat(w.saturating_sub(filled + 2)),
                    Style::default().fg(colors.dim),
                ),
            ])
        };

        frame.render_widget(Paragraph::new(vec![track_line, seek_line]), area);
    }

    fn render_808_status(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        let mut spans = vec![Span::styled(" ", Style::default())];

        // Repeat
        let repeat_str = format!("RPT:{}", self.playlist.repeat());
        if self.playlist.repeat() != crate::playlist::RepeatMode::Off {
            spans.push(Span::styled(
                repeat_str,
                Style::default()
                    .fg(colors.yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(repeat_str, Style::default().fg(colors.dim)));
        }

        spans.push(Span::raw("  "));

        // Shuffle
        if self.playlist.shuffled() {
            spans.push(Span::styled(
                "SHF",
                Style::default()
                    .fg(colors.yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled("SHF", Style::default().fg(colors.dim)));
        }

        spans.push(Span::raw("  "));

        // Mono
        if self.player.mono() {
            spans.push(Span::styled(
                "MONO",
                Style::default()
                    .fg(colors.orange)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled("MONO", Style::default().fg(colors.dim)));
        }

        spans.push(Span::raw("  "));

        // Volume dB readout
        spans.push(Span::styled(
            format!("{:+.1}dB", self.player.volume()),
            Style::default().fg(colors.grey),
        ));

        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            self.bpm.machine_text(),
            Style::default().fg(colors.grey),
        ));

        // Queue count
        let q_len = self.playlist.queue_len();
        if q_len > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("Q:{q_len}"),
                Style::default()
                    .fg(colors.amber)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("VIS:{}", vis_mode_label(self.vis.mode)),
            Style::default().fg(colors.dim),
        ));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_808_spectrum(&mut self, frame: &mut Frame, area: Rect) {
        if self.player.is_music_app() {
            let (pos_secs, _) = self.track_position();
            let animate = self.player.is_playing() && !self.player.is_paused();
            let bands = self.vis.synthetic_bands(
                pos_secs as f64,
                self.player.is_playing(),
                self.player.is_paused(),
            );

            match render_mode_808(self.vis.mode) {
                RenderMode808::HorizontalBars => {
                    let solid = matches!(self.vis.mode, VisMode::BarsGap);
                    let lines = self.vis.render_808_horizontal(&bands, solid);
                    self.render_808_visual_lines(frame, area, lines);
                }
                RenderMode808::LedColumns => {
                    self.render_808_led_columns(frame, area, &bands);
                }
                RenderMode808::Retro => {
                    let lines = self.vis.render_retro(
                        &bands,
                        area.width as usize,
                        area.height as usize,
                        animate,
                    );
                    self.render_808_visual_lines(frame, area, lines);
                }
                RenderMode808::Oscilloscope => {
                    self.render_808_led_columns(frame, area, &bands);
                }
            }
            return;
        }

        let samples = self.player.samples();
        match render_mode_808(self.vis.mode) {
            RenderMode808::HorizontalBars => {
                let bands = self.vis.analyze(&samples);
                let solid = matches!(self.vis.mode, VisMode::BarsGap);
                let lines = self.vis.render_808_horizontal(&bands, solid);
                self.render_808_visual_lines(frame, area, lines);
            }
            RenderMode808::LedColumns => {
                let bands = self.vis.analyze(&samples);
                self.render_808_led_columns(frame, area, &bands);
            }
            RenderMode808::Retro => {
                let bands = self.vis.analyze(&samples);
                let animate = self.player.is_playing() && !self.player.is_paused();
                let lines = self.vis.render_retro(
                    &bands,
                    area.width as usize,
                    area.height as usize,
                    animate,
                );
                self.render_808_visual_lines(frame, area, lines);
            }
            RenderMode808::Oscilloscope => {
                let lines =
                    self.vis
                        .render_scope(&samples, area.width as usize, area.height as usize);
                self.render_808_visual_lines(frame, area, lines);
            }
        }
    }

    fn render_808_visual_lines(
        &self,
        frame: &mut Frame,
        area: Rect,
        lines: Vec<super::visualizer::SpectrumLine>,
    ) {
        let colors = self.colors_808();
        for (row, spec_line) in lines.iter().enumerate() {
            if row >= area.height as usize {
                break;
            }
            let spans: Vec<Span> = spec_line
                .segments
                .iter()
                .map(|seg| Span::styled(&seg.text, spectrum_style_808(seg, colors)))
                .collect();

            let y = area.y + row as u16;
            let line_area = Rect::new(area.x, y, area.width, 1);
            frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    fn render_808_led_columns(&self, frame: &mut Frame, area: Rect, bands: &[f64; 10]) {
        let colors = self.colors_808();
        const HEIGHT: usize = 5;

        for row in 0..HEIGHT {
            if row >= area.height as usize {
                break;
            }
            let row_bottom = (HEIGHT - 1 - row) as f64 / HEIGHT as f64;
            let row_top = (HEIGHT - row) as f64 / HEIGHT as f64;

            let band_w = (area.width as usize) / 10;
            let mut spans = Vec::with_capacity(10);

            for &level in bands.iter() {
                let color = if row_bottom >= 0.8 {
                    colors.red
                } else if row_bottom >= 0.6 {
                    colors.orange
                } else if row_bottom >= 0.3 {
                    colors.amber
                } else {
                    colors.yellow
                };

                let block = if level >= row_top {
                    "█".repeat(band_w.saturating_sub(1))
                } else if level > row_bottom {
                    let frac = (level - row_bottom) / (row_top - row_bottom);
                    let blocks = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
                    let idx = (frac * (blocks.len() - 1) as f64) as usize;
                    blocks[idx.min(blocks.len() - 1)].repeat(band_w.saturating_sub(1))
                } else {
                    " ".repeat(band_w.saturating_sub(1))
                };

                spans.push(Span::styled(block, Style::default().fg(color)));
                spans.push(Span::raw(" "));
            }

            let y = area.y + row as u16;
            let line_area = Rect::new(area.x, y, area.width, 1);
            frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    fn render_808_playlist(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        let chrome = self.chrome_signature_808();
        let tracks = self.playlist.tracks();
        if tracks.is_empty() {
            let text = if self.player.is_music_app() {
                "  Music.app backend active"
            } else {
                "  No tracks loaded"
            };
            let line = Line::from(Span::styled(text, Style::default().fg(colors.dim)));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        if self.searching {
            self.render_808_search(frame, area);
            return;
        }

        let current_idx = self.playlist.index().unwrap_or(usize::MAX);
        let visible = (area.height as usize).min(tracks.len());
        // Update pl_visible for scroll calculations
        let pl_visible = visible;

        let mut scroll = self.pl_scroll;
        if scroll + pl_visible > tracks.len() {
            scroll = tracks.len().saturating_sub(pl_visible);
        }

        let mut lines = Vec::with_capacity(visible);
        for i in scroll..scroll + pl_visible {
            if i >= tracks.len() {
                break;
            }

            let mut prefix = "   ";
            let mut style = Style::default().fg(colors.grey);

            if i == current_idx && self.player.is_playing() {
                prefix = " ► ";
                style = Style::default()
                    .fg(colors.orange)
                    .add_modifier(Modifier::BOLD);
            }

            if self.focus == Focus::Playlist && i == self.pl_cursor {
                style = lift_style_808(style, colors, chrome.row_lift_strength());
            }

            let name = tracks[i].display_name();
            let max_w = area.width as usize - 8;
            let display_name: String = if name.chars().count() > max_w {
                let mut s: String = name.chars().take(max_w - 1).collect();
                s.push('…');
                s
            } else {
                name
            };

            let num = format!("{:02}", i + 1);
            let mut spans = vec![Span::styled(
                format!("{prefix}{num}. {display_name}"),
                style,
            )];

            let qp = self.playlist.queue_position(i);
            if qp > 0 {
                spans.push(Span::styled(
                    format!(" [Q{qp}]"),
                    Style::default()
                        .fg(colors.amber)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_provider(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        let chrome = self.chrome_signature_808();
        if self.prov_loading {
            let name = self.provider_name();
            let target = if self.apple_music_showing_tracks() {
                "tracks"
            } else {
                "playlists"
            };
            let line = Line::from(Span::styled(
                format!("  Loading {name} {target}..."),
                Style::default().fg(colors.dim),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        if self.apple_music_showing_tracks() {
            if self.apple_music_tracks.is_empty() {
                let line = Line::from(Span::styled(
                    "  No tracks found",
                    Style::default().fg(colors.dim),
                ));
                frame.render_widget(Paragraph::new(line), area);
                return;
            }

            let visible = (area.height as usize).min(self.apple_music_tracks.len());
            let scroll = if self.prov_cursor >= visible {
                self.prov_cursor - visible + 1
            } else {
                0
            };

            let mut lines = Vec::with_capacity(visible);
            for i in scroll..self.apple_music_tracks.len().min(scroll + visible) {
                let track = &self.apple_music_tracks[i];
                let style = if i == self.prov_cursor {
                    lift_style_808(
                        Style::default().fg(colors.grey),
                        colors,
                        chrome.row_lift_strength(),
                    )
                } else {
                    Style::default().fg(colors.grey)
                };
                let prefix = if i == self.prov_cursor {
                    " ► "
                } else {
                    "   "
                };
                let name = if track.artist.is_empty() {
                    track.title.clone()
                } else {
                    format!("{} - {}", track.artist, track.title)
                };
                lines.push(Line::from(Span::styled(format!("{prefix}{name}"), style)));
            }

            frame.render_widget(Paragraph::new(lines), area);
            return;
        }

        if self.provider_lists.is_empty() {
            let line = Line::from(Span::styled(
                "  No playlists found",
                Style::default().fg(colors.dim),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        let visible = (area.height as usize).min(self.provider_lists.len());
        let scroll = if self.prov_cursor >= visible {
            self.prov_cursor - visible + 1
        } else {
            0
        };

        let mut lines = Vec::with_capacity(visible);
        for i in scroll..self.provider_lists.len().min(scroll + visible) {
            let pl = &self.provider_lists[i];
            let style = if i == self.prov_cursor {
                lift_style_808(
                    Style::default().fg(colors.grey),
                    colors,
                    chrome.row_lift_strength(),
                )
            } else {
                Style::default().fg(colors.grey)
            };
            let prefix = if i == self.prov_cursor {
                " ► "
            } else {
                "   "
            };
            lines.push(Line::from(Span::styled(
                format!("{prefix}{} ({} tracks)", pl.name, pl.track_count),
                style,
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_browser(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        let Some(explorer) = self.explorer.as_ref() else {
            let line = Line::from(Span::styled(
                "  Press [L] to browse local playlists",
                Style::default().fg(colors.dim),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        };

        if area.height < 2 {
            let line = Line::from(Span::styled(
                explorer.header_text(),
                Style::default().fg(colors.grey),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);
        let header = Line::from(Span::styled(
            explorer.header_text(),
            Style::default().fg(colors.grey),
        ));
        frame.render_widget(Paragraph::new(header), chunks[0]);
        frame.render_widget_ref(explorer.widget(), chunks[1]);
    }

    fn render_808_search(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        let mut lines = vec![Line::from(vec![
            Span::styled(" / ", Style::default().fg(colors.yellow)),
            Span::styled(&self.search_query, Style::default().fg(colors.grey)),
            Span::styled(
                format!("  ({} found)", self.search_results.len()),
                Style::default().fg(colors.dim),
            ),
        ])];

        let tracks = self.playlist.tracks();
        let visible = (area.height as usize).saturating_sub(1);
        let scroll = if self.search_cursor >= visible {
            self.search_cursor - visible + 1
        } else {
            0
        };

        for j in scroll..self.search_results.len().min(scroll + visible) {
            let i = self.search_results[j];
            let name = tracks[i].display_name();
            let max_w = area.width as usize - 8;
            let display_name: String = if name.chars().count() > max_w {
                let mut s: String = name.chars().take(max_w - 1).collect();
                s.push('…');
                s
            } else {
                name
            };

            let style = if j == self.search_cursor {
                Style::default()
                    .fg(colors.yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.grey)
            };

            lines.push(Line::from(Span::styled(
                format!("   {:02}. {display_name}", i + 1),
                style,
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_help(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        if self.command_mode {
            let line = Line::from(Span::styled(
                format!(" : {}  [Enter]RUN [Esc]CANCEL", self.command_input),
                Style::default().fg(colors.grey),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        let controls = controls_808(
            self.focus,
            self.player.supports_seek(),
            self.player.supports_eq(),
            self.player.supports_visualizer(),
            self.player.supports_cover_art(),
            self.player.supports_local_playlist(),
            self.apple_music_showing_tracks(),
        );
        if controls.is_empty() {
            return;
        }

        if area.height < 2 {
            // Fallback for very small terminals.
            let text = controls
                .iter()
                .map(|(key, action)| format!("[{key}]{action}"))
                .collect::<Vec<_>>()
                .join(" ");
            let line = Line::from(Span::styled(text, Style::default().fg(colors.dim)));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        const CELL_W: usize = 6;
        let mut key_spans = Vec::new();
        let mut action_spans = Vec::new();
        let mut used = 0u16;

        for (i, (key, action)) in controls.iter().enumerate() {
            let extra_gap = if i == 0 { 0 } else { 1 };
            let need = CELL_W as u16 + extra_gap;
            if used + need > area.width {
                break;
            }

            if i > 0 {
                key_spans.push(Span::raw(" "));
                action_spans.push(Span::raw(" "));
                used += 1;
            }

            let pad_color = step_pad_color(i, colors);
            key_spans.push(Span::styled(
                format!("{:^CELL_W$}", key),
                Style::default()
                    .fg(colors.black)
                    .bg(pad_color)
                    .add_modifier(Modifier::BOLD),
            ));
            action_spans.push(Span::styled(
                format!("{:^CELL_W$}", action),
                Style::default().fg(colors.dim),
            ));
            used += CELL_W as u16;
        }

        let lines = vec![Line::from(key_spans), Line::from(action_spans)];
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_status_line(&self, frame: &mut Frame, area: Rect) {
        let colors = self.colors_808();
        if self.command_mode {
            let line = Line::from(Span::styled(
                format!(" :{}_", self.command_input),
                Style::default().fg(colors.yellow),
            ));
            frame.render_widget(Paragraph::new(line), area);
        } else if let Some(ref err) = self.err {
            let line = Line::from(Span::styled(
                format!(" ERR: {err}"),
                Style::default().fg(colors.red),
            ));
            frame.render_widget(Paragraph::new(line), area);
        } else if !self.save_msg.is_empty() {
            let line = Line::from(Span::styled(
                format!(" {}", self.save_msg),
                Style::default().fg(colors.orange),
            ));
            frame.render_widget(Paragraph::new(line), area);
        }
    }
}

/// Render a single rotary knob using Canvas with Braille markers.
///
/// `value` is normalized 0.0-1.0.
/// The knob arc spans from 210deg (min) to -30deg (max), clockwise.
fn render_knob(
    frame: &mut Frame,
    area: Rect,
    value: f64,
    label: &str,
    selected: bool,
    colors: Classic808Colors,
) {
    if area.width < 4 || area.height < 3 {
        return;
    }

    // Split: top part for knob canvas, bottom line for label
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),    // canvas
            Constraint::Length(1), // label
        ])
        .split(area);

    let canvas_area = chunks[0];
    let label_area = chunks[1];

    // Knob parameters
    let cx = 0.0;
    let cy = 0.0;
    let radius = 3.0;
    let start_angle: f64 = 210.0_f64.to_radians(); // bottom-left (min)
    let end_angle: f64 = -30.0_f64.to_radians(); // bottom-right (max)

    // Current value angle (sweep clockwise from start to end)
    let sweep = start_angle - end_angle; // positive, ~240 degrees
    let val_angle = start_angle - value * sweep;

    let accent = if selected { colors.yellow } else { colors.grey };
    let fill_color = if selected {
        colors.orange
    } else {
        colors.amber
    };

    let canvas = Canvas::default()
        .x_bounds([-5.0, 5.0])
        .y_bounds([-4.0, 4.0])
        .marker(ratatui::symbols::Marker::Braille)
        .paint(move |ctx| {
            // Background arc — draw dots along the full arc
            let steps = 20;
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let angle = start_angle - t * sweep;
                let px = cx + radius * angle.cos();
                let py = cy + radius * angle.sin();
                ctx.print(px, py, Span::styled("·", Style::default().fg(colors.dim)));
            }

            // Active arc — draw from min to current value
            let active_steps = (value * 20.0) as usize;
            for i in 0..=active_steps {
                let t = i as f64 / 20.0;
                let angle = start_angle - t * sweep;
                let px = cx + radius * angle.cos();
                let py = cy + radius * angle.sin();
                ctx.print(px, py, Span::styled("●", Style::default().fg(fill_color)));
            }

            // Pointer line from center to current position
            let ptr_len = radius * 0.7;
            let px = cx + ptr_len * val_angle.cos();
            let py = cy + ptr_len * val_angle.sin();
            ctx.draw(&CanvasLine {
                x1: cx,
                y1: cy,
                x2: px,
                y2: py,
                color: accent,
            });
        });

    frame.render_widget(canvas, canvas_area);

    // Label below knob
    let label_style = if selected {
        Style::default()
            .fg(colors.yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.grey)
    };

    let label_line = Line::from(Span::styled(label, label_style));
    frame.render_widget(
        Paragraph::new(label_line).alignment(Alignment::Center),
        label_area,
    );
}

fn step_pad_color(idx: usize, colors: Classic808Colors) -> Color {
    match idx {
        0 | 1 => colors.red,
        2 | 3 => colors.orange,
        4 | 5 => colors.amber,
        6 | 7 => colors.yellow,
        _ => colors.ivory,
    }
}

fn render_mode_808(mode: VisMode) -> RenderMode808 {
    match mode {
        VisMode::BarsGap => RenderMode808::HorizontalBars,
        VisMode::Bars => RenderMode808::HorizontalBars,
        VisMode::VBars => RenderMode808::LedColumns,
        VisMode::Bricks => RenderMode808::LedColumns,
        VisMode::Retro => RenderMode808::Retro,
        VisMode::Scope => RenderMode808::Oscilloscope,
    }
}

fn vis_mode_label(mode: VisMode) -> &'static str {
    match mode {
        VisMode::Bars => "BARS",
        VisMode::BarsGap => "SOLID",
        VisMode::VBars => "VBAR",
        VisMode::Bricks => "LEDS",
        VisMode::Retro => "RETRO",
        VisMode::Scope => "SCOPE",
    }
}

fn retro_grid_color_808(row_bottom: f64, colors: Classic808Colors) -> Color {
    if row_bottom >= 0.72 {
        colors.red
    } else if row_bottom >= 0.45 {
        colors.orange
    } else if row_bottom >= 0.18 {
        colors.amber
    } else {
        colors.deep_amber
    }
}

fn retro_wave_color_808(row_bottom: f64, colors: Classic808Colors) -> Color {
    if row_bottom >= 0.72 {
        colors.hot_pink
    } else if row_bottom >= 0.4 {
        colors.sunset_orange
    } else {
        colors.orange
    }
}

fn spectrum_style_808(seg: &SpectrumSegment, colors: Classic808Colors) -> Style {
    match seg.kind {
        SpectrumSegmentKind::Gradient => {
            let color = if seg.row_bottom >= 0.6 {
                colors.red
            } else if seg.row_bottom >= 0.3 {
                colors.amber
            } else {
                colors.yellow
            };
            Style::default().fg(color)
        }
        SpectrumSegmentKind::RetroGrid => {
            Style::default().fg(retro_grid_color_808(seg.row_bottom, colors))
        }
        SpectrumSegmentKind::RetroSun => Style::default().fg(colors.yellow),
        SpectrumSegmentKind::RetroWave => Style::default()
            .fg(retro_wave_color_808(seg.row_bottom, colors))
            .add_modifier(Modifier::BOLD),
    }
}

fn make_808_header_effect(
    colors: Classic808Colors,
    chrome: ChromeFxSignature,
) -> Option<tachyonfx::Effect> {
    let config = chrome.header_accent_config()?;
    Some(fx::repeating(fx::effect_fn(
        HeaderAccentState {
            dim: colors.dim,
            grey: colors.grey,
            accent: colors.header_accent(),
            warm: colors.header_warm(),
            config,
        },
        (config.cycle_ms, Interpolation::SineInOut),
        |state, ctx, cells| {
            let width = ctx.area.width.max(1) as f32;
            cells.for_each_cell(|pos, cell| {
                if is_block_border_symbol_808(cell.symbol()) || cell.symbol().trim().is_empty() {
                    return;
                }

                let base = match cell.fg {
                    color if color == state.dim => state.dim,
                    color if color == state.grey => state.grey,
                    _ => return,
                };

                let offset = pos.x.saturating_sub(ctx.area.x) as f32 / width;
                let wave = (ctx.alpha() * TAU + offset * 1.35).sin() * 0.5 + 0.5;
                let lift = state.config.amplitude * (0.35 + 0.65 * wave);
                let target = if base == state.dim {
                    mix_rgb(base, state.warm, lift)
                } else {
                    mix_rgb(base, state.accent, lift * 0.82)
                };
                cell.set_fg(target);
            });
        },
    )))
}

fn make_808_panel_effect(
    colors: Classic808Colors,
    chrome: ChromeFxSignature,
) -> Option<tachyonfx::Effect> {
    let config = chrome.panel_trace_config()?;
    Some(fx::repeating(fx::effect_fn(
        PanelTraceState {
            base: colors.dim,
            head: colors.panel_trace_head(),
            tail: colors.panel_trace_tail(),
            config,
        },
        (config.cycle_ms, Interpolation::Linear),
        |state, ctx, cells| {
            if ctx.area.width < 2 || ctx.area.height < 2 {
                return;
            }

            let perimeter = perimeter_len_808(ctx.area);
            if perimeter == 0 {
                return;
            }

            let head = ctx.alpha() * perimeter as f32;
            let tail_len = (perimeter as f32 * state.config.tail_ratio).max(4.0);

            cells.for_each_cell(|pos, cell| {
                let Some(index) = perimeter_index_808(ctx.area, pos) else {
                    return;
                };

                let distance = (head - index as f32).rem_euclid(perimeter as f32);
                if distance <= tail_len {
                    let falloff = 1.0 - distance / tail_len;
                    let hotspot = falloff.powf(1.7);
                    let accent = mix_rgb(state.tail, state.head, hotspot);
                    let mix = state.config.tail_mix
                        + (state.config.head_mix - state.config.tail_mix) * hotspot;
                    cell.set_fg(mix_rgb(state.base, accent, mix));
                } else {
                    cell.set_fg(state.base);
                }
            });
        },
    )))
}

fn transport_accent_color_808(colors: Classic808Colors, transport: ChromeTransportState) -> Color {
    match transport {
        ChromeTransportState::Playing => colors.orange,
        ChromeTransportState::Buffering => mix_rgb(colors.amber, colors.orange, 0.35),
        ChromeTransportState::Paused => mix_rgb(colors.orange, colors.grey, 0.42),
        ChromeTransportState::Stopped => mix_rgb(colors.grey, colors.dim, 0.28),
        ChromeTransportState::Error => colors.red,
    }
}

fn lift_style_808(style: Style, colors: Classic808Colors, strength: f32) -> Style {
    let lifted = mix_rgb(
        style.fg.unwrap_or(colors.grey),
        colors.row_lift_target(),
        strength,
    );
    style.fg(lifted).add_modifier(Modifier::BOLD)
}

fn seek_dot_pulse_808(frame: usize, chrome: ChromeFxSignature) -> f32 {
    let strength = chrome.seek_pulse_strength();
    if strength <= 0.0 {
        return 0.0;
    }

    let period = match chrome.mode {
        VisMode::Scope => 18.0,
        VisMode::Retro => 28.0,
        VisMode::Bars | VisMode::BarsGap | VisMode::VBars | VisMode::Bricks => 22.0,
    };
    let wave = ((frame as f32 / period) * TAU).sin() * 0.5 + 0.5;
    strength * wave
}

fn chrome_transport_state(
    buffering: bool,
    has_error: bool,
    is_playing: bool,
    is_paused: bool,
) -> ChromeTransportState {
    if buffering {
        ChromeTransportState::Buffering
    } else if has_error {
        ChromeTransportState::Error
    } else if is_playing && is_paused {
        ChromeTransportState::Paused
    } else if is_playing {
        ChromeTransportState::Playing
    } else {
        ChromeTransportState::Stopped
    }
}

fn perimeter_len_808(area: Rect) -> u16 {
    area.width
        .saturating_mul(2)
        .saturating_add(area.height.saturating_mul(2))
        .saturating_sub(4)
}

fn perimeter_index_808(area: Rect, pos: Position) -> Option<u16> {
    if area.width < 2 || area.height < 2 {
        return None;
    }

    let left = area.x;
    let right = area.right().saturating_sub(1);
    let top = area.y;
    let bottom = area.bottom().saturating_sub(1);

    if pos.y == top && pos.x >= left && pos.x <= right {
        Some(pos.x - left)
    } else if pos.x == right && pos.y > top && pos.y <= bottom {
        Some(area.width + (pos.y - top - 1))
    } else if pos.y == bottom && pos.x >= left && pos.x < right {
        Some(area.width + (area.height - 1) + (right - pos.x - 1))
    } else if pos.x == left && pos.y > top && pos.y < bottom {
        Some(area.width + (area.height - 1) + (area.width - 1) + (bottom - pos.y - 1))
    } else {
        None
    }
}

fn is_block_border_symbol_808(symbol: &str) -> bool {
    matches!(
        symbol,
        "─" | "│" | "┌" | "┐" | "└" | "┘" | "├" | "┤" | "┬" | "┴" | "┼"
    )
}

fn playback_state_label(
    buffering: bool,
    has_error: bool,
    is_playing: bool,
    is_paused: bool,
) -> &'static str {
    if buffering {
        "BUFFERING"
    } else if has_error {
        "ERROR"
    } else if is_playing && is_paused {
        "PAUSED"
    } else if is_playing {
        "PLAYING"
    } else {
        "STOPPED"
    }
}

fn controls_808(
    focus: Focus,
    supports_seek: bool,
    supports_eq: bool,
    supports_visualizer: bool,
    supports_cover_art: bool,
    supports_local_playlist: bool,
    apple_music_tracks: bool,
) -> Vec<(&'static str, &'static str)> {
    if focus == Focus::Provider {
        if apple_music_tracks {
            return vec![
                ("↑↓", "NAV"),
                ("Enter", "INFO"),
                ("Esc", "BACK"),
                ("Tab", "FOCS"),
                ("Q", "QUIT"),
            ];
        }

        return vec![
            ("↑↓", "NAV"),
            ("Enter", "OPEN"),
            ("Tab", "FOCS"),
            ("Q", "QUIT"),
        ];
    }

    if focus == Focus::Browser {
        return vec![
            ("↑↓", "NAV"),
            ("←", "UP"),
            ("→", "OPEN"),
            ("Esc", "PLAY"),
            ("L", "CLOSE"),
            ("Tab", "FOCS"),
        ];
    }

    let mut controls = vec![("Spc", "PLAY"), ("<>", "TRK"), ("+-", "VOL")];

    if supports_seek {
        controls.insert(2, ("←→", "SEEK"));
    }

    if supports_eq {
        controls.push(("e", "EQ"));
    }
    if supports_visualizer {
        controls.push(("v", "VIS"));
    }
    if supports_cover_art {
        controls.push(("c", "ART"));
    }
    if supports_local_playlist {
        controls.extend([
            ("S", "SAVE"),
            ("L", "LOAD"),
            (":", "CMD"),
            ("/", "FIND"),
            ("Tab", "FOCS"),
        ]);
    } else {
        controls.push(("s", "STOP"));
        controls.push((":", "CMD"));
    }

    controls.push(("8", "808"));
    controls.push(("Q", "QUIT"));

    controls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::visualizer::VisMode;

    #[test]
    fn controls_playlist_seekable_has_seek() {
        let controls = controls_808(Focus::Playlist, true, true, true, true, true, false);
        assert!(
            controls
                .iter()
                .any(|(key, action)| *key == "←→" && *action == "SEEK")
        );
    }

    #[test]
    fn controls_playlist_stream_hides_seek() {
        let controls = controls_808(Focus::Playlist, false, true, true, true, true, false);
        assert!(!controls.iter().any(|(key, _)| *key == "←→"));
    }

    #[test]
    fn controls_playlist_has_visualizer_toggle() {
        let controls = controls_808(Focus::Playlist, true, true, true, true, true, false);
        assert!(
            controls
                .iter()
                .any(|(key, action)| *key == "v" && *action == "VIS")
        );
    }

    #[test]
    fn playback_state_label_prioritizes_buffering_and_error() {
        assert_eq!(playback_state_label(true, false, true, false), "BUFFERING");
        assert_eq!(playback_state_label(false, true, false, false), "ERROR");
        assert_eq!(playback_state_label(false, false, true, true), "PAUSED");
        assert_eq!(playback_state_label(false, false, true, false), "PLAYING");
        assert_eq!(playback_state_label(false, false, false, false), "STOPPED");
    }

    #[test]
    fn controls_playlist_has_load_browser() {
        let controls = controls_808(Focus::Playlist, true, true, true, true, true, false);
        assert!(
            controls
                .iter()
                .any(|(key, action)| *key == "L" && *action == "LOAD")
        );
    }

    #[test]
    fn controls_browser_mode_exposes_navigation() {
        let controls = controls_808(Focus::Browser, true, true, true, true, true, false);
        assert!(
            controls
                .iter()
                .any(|(key, action)| *key == "←" && *action == "UP")
        );
        assert!(
            controls
                .iter()
                .any(|(key, action)| *key == "Esc" && *action == "PLAY")
        );
    }

    #[test]
    fn controls_provider_mode_is_compact() {
        let controls = controls_808(Focus::Provider, true, true, true, true, true, false);
        assert_eq!(
            controls,
            vec![
                ("↑↓", "NAV"),
                ("Enter", "OPEN"),
                ("Tab", "FOCS"),
                ("Q", "QUIT")
            ]
        );
    }

    #[test]
    fn controls_provider_track_view_includes_back_action() {
        let controls = controls_808(Focus::Provider, true, true, true, true, true, true);
        assert_eq!(
            controls,
            vec![
                ("↑↓", "NAV"),
                ("Enter", "INFO"),
                ("Esc", "BACK"),
                ("Tab", "FOCS"),
                ("Q", "QUIT")
            ]
        );
    }

    #[test]
    fn music_app_controls_hide_local_only_actions() {
        let controls = controls_808(Focus::Playlist, false, false, false, false, false, false);
        assert!(!controls.iter().any(|(key, _)| *key == "L"));
        assert!(!controls.iter().any(|(key, _)| *key == "v"));
        assert!(!controls.iter().any(|(key, _)| *key == "c"));
        assert!(
            controls
                .iter()
                .any(|(key, action)| *key == "s" && *action == "STOP")
        );
    }

    #[test]
    fn browser_focus_reserves_base_view_browser_height() {
        assert_eq!(content_height_808(true, false, 40), 31);
        assert_eq!(browser_panel_min_height_808(true), 6);
    }

    #[test]
    fn non_browser_focus_keeps_standard_808_height() {
        assert_eq!(content_height_808(false, false, 40), 28);
        assert_eq!(browser_panel_min_height_808(false), 3);
    }

    #[test]
    fn retro_mode_reserves_more_vertical_space() {
        assert_eq!(content_height_808(false, true, 40), 31);
        assert_eq!(content_height_808(true, true, 40), 34);
    }

    #[test]
    fn render_mode_mapping_matches_808_design() {
        assert_eq!(
            render_mode_808(VisMode::Bars),
            RenderMode808::HorizontalBars
        );
        assert_eq!(
            render_mode_808(VisMode::BarsGap),
            RenderMode808::HorizontalBars
        );
        assert_eq!(render_mode_808(VisMode::VBars), RenderMode808::LedColumns);
        assert_eq!(render_mode_808(VisMode::Bricks), RenderMode808::LedColumns);
        assert_eq!(render_mode_808(VisMode::Retro), RenderMode808::Retro);
        assert_eq!(render_mode_808(VisMode::Scope), RenderMode808::Oscilloscope);
    }

    #[test]
    fn vis_mode_labels_are_short_and_clear() {
        assert_eq!(vis_mode_label(VisMode::Bars), "BARS");
        assert_eq!(vis_mode_label(VisMode::BarsGap), "SOLID");
        assert_eq!(vis_mode_label(VisMode::VBars), "VBAR");
        assert_eq!(vis_mode_label(VisMode::Bricks), "LEDS");
        assert_eq!(vis_mode_label(VisMode::Retro), "RETRO");
        assert_eq!(vis_mode_label(VisMode::Scope), "SCOPE");
    }

    #[test]
    fn retro_grid_uses_warm_gradient_instead_of_flat_grey() {
        let colors = Classic808Colors::classic();
        let hot_horizon = SpectrumSegment {
            text: "⣿".to_string(),
            row_bottom: 0.8,
            kind: SpectrumSegmentKind::RetroGrid,
        };
        let deep_floor = SpectrumSegment {
            text: "⣿".to_string(),
            row_bottom: 0.1,
            kind: SpectrumSegmentKind::RetroGrid,
        };

        assert_eq!(spectrum_style_808(&hot_horizon, colors).fg, Some(C808_RED));
        assert_eq!(
            spectrum_style_808(&deep_floor, colors).fg,
            Some(C808_DEEP_AMBER)
        );
    }

    #[test]
    fn retro_wave_uses_synthwave_hot_pink_to_orange_ramp() {
        let colors = Classic808Colors::classic();
        let crest = SpectrumSegment {
            text: "⣿".to_string(),
            row_bottom: 0.85,
            kind: SpectrumSegmentKind::RetroWave,
        };
        let mid_wave = SpectrumSegment {
            text: "⣿".to_string(),
            row_bottom: 0.55,
            kind: SpectrumSegmentKind::RetroWave,
        };
        let lower_wave = SpectrumSegment {
            text: "⣿".to_string(),
            row_bottom: 0.25,
            kind: SpectrumSegmentKind::RetroWave,
        };

        assert_eq!(spectrum_style_808(&crest, colors).fg, Some(C808_HOT_PINK));
        assert_eq!(
            spectrum_style_808(&mid_wave, colors).fg,
            Some(C808_SUNSET_ORANGE)
        );
        assert_eq!(
            spectrum_style_808(&lower_wave, colors).fg,
            Some(C808_ORANGE)
        );
        assert!(
            spectrum_style_808(&crest, colors)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn themed_808_colors_follow_palette() {
        let palette = Palette {
            title: Color::Rgb(0x11, 0x22, 0x33),
            text: Color::Rgb(0xE0, 0xE1, 0xE2),
            dim: Color::Rgb(0x44, 0x55, 0x66),
            accent: Color::Rgb(0xAA, 0xBB, 0xCC),
            playing: Color::Rgb(0x10, 0x20, 0x30),
            seek_bar: Color::Rgb(0x40, 0x50, 0x60),
            volume: Color::Rgb(0x70, 0x80, 0x90),
            error: Color::Rgb(0x91, 0x92, 0x93),
            spectrum_low: Color::Rgb(0x12, 0x13, 0x14),
            spectrum_mid: Color::Rgb(0x21, 0x22, 0x23),
            spectrum_high: Color::Rgb(0x31, 0x32, 0x33),
        };

        let themed = Classic808Colors::themed(&palette);
        assert!(themed.themed);
        assert_eq!(themed.red, palette.error);
        assert_eq!(themed.orange, palette.playing);
        assert_eq!(themed.amber, palette.seek_bar);
        assert_eq!(themed.yellow, palette.accent);
        assert_eq!(themed.grey, palette.text);
        assert_eq!(themed.dim, palette.dim);
    }

    #[test]
    fn themed_motion_colors_stay_palette_relative() {
        let palette = Palette {
            title: Color::Rgb(0x11, 0x22, 0x33),
            text: Color::Rgb(0xE0, 0xE1, 0xE2),
            dim: Color::Rgb(0x44, 0x55, 0x66),
            accent: Color::Rgb(0xAA, 0xBB, 0xCC),
            playing: Color::Rgb(0x10, 0x20, 0x30),
            seek_bar: Color::Rgb(0x40, 0x50, 0x60),
            volume: Color::Rgb(0x70, 0x80, 0x90),
            error: Color::Rgb(0x91, 0x92, 0x93),
            spectrum_low: Color::Rgb(0x12, 0x13, 0x14),
            spectrum_mid: Color::Rgb(0x21, 0x22, 0x23),
            spectrum_high: Color::Rgb(0x31, 0x32, 0x33),
        };

        let themed = Classic808Colors::themed(&palette);
        assert_eq!(
            themed.header_accent(),
            mix_rgb(themed.grey, themed.yellow, 0.42)
        );
        assert_eq!(
            themed.header_warm(),
            mix_rgb(themed.dim, themed.yellow, 0.24)
        );
        assert_eq!(
            themed.panel_trace_head(),
            mix_rgb(themed.grey, themed.yellow, 0.58)
        );
        assert_eq!(
            themed.panel_trace_tail(),
            mix_rgb(themed.dim, themed.yellow, 0.32)
        );
        assert_eq!(
            themed.seek_tail_color(0.5),
            mix_rgb(themed.dim, themed.yellow, 0.27)
        );
        assert_eq!(
            themed.seek_dot_color(0.5),
            mix_rgb(themed.grey, themed.yellow, 0.60)
        );
    }

    #[test]
    fn classic_motion_colors_keep_warm_808_bias() {
        let classic = Classic808Colors::classic();

        assert!(!classic.themed);
        assert_eq!(
            classic.panel_trace_head(),
            mix_rgb(classic.amber, classic.yellow, 0.28)
        );
        assert_eq!(
            classic.panel_trace_tail(),
            mix_rgb(classic.deep_amber, classic.amber, 0.46)
        );
        assert_eq!(
            classic.seek_tail_color(0.5),
            mix_rgb(classic.amber, classic.orange, 0.40)
        );
    }

    #[test]
    fn chrome_transport_state_follows_play_pause_state() {
        assert_eq!(
            chrome_transport_state(true, false, true, false),
            ChromeTransportState::Buffering
        );
        assert_eq!(
            chrome_transport_state(false, true, true, false),
            ChromeTransportState::Error
        );
        assert_eq!(
            chrome_transport_state(false, false, true, true),
            ChromeTransportState::Paused
        );
        assert_eq!(
            chrome_transport_state(false, false, true, false),
            ChromeTransportState::Playing
        );
        assert_eq!(
            chrome_transport_state(false, false, false, false),
            ChromeTransportState::Stopped
        );
    }

    #[test]
    fn chrome_signature_softens_retro_and_browser_motion() {
        let scope = ChromeFxSignature {
            transport: ChromeTransportState::Playing,
            focus: Focus::Playlist,
            mode: VisMode::Scope,
        }
        .panel_trace_config()
        .unwrap();
        let retro = ChromeFxSignature {
            transport: ChromeTransportState::Playing,
            focus: Focus::Playlist,
            mode: VisMode::Retro,
        }
        .panel_trace_config()
        .unwrap();
        let browsing = ChromeFxSignature {
            transport: ChromeTransportState::Stopped,
            focus: Focus::Browser,
            mode: VisMode::Bars,
        }
        .panel_trace_config();

        assert!(scope.head_mix > retro.head_mix);
        assert!(scope.cycle_ms < retro.cycle_ms);
        assert!(browsing.is_none());
    }

    #[test]
    fn seek_pulse_only_moves_when_transport_has_motion() {
        let playing = ChromeFxSignature {
            transport: ChromeTransportState::Playing,
            focus: Focus::Playlist,
            mode: VisMode::Bars,
        }
        .seek_pulse_strength();
        let paused = ChromeFxSignature {
            transport: ChromeTransportState::Paused,
            focus: Focus::Playlist,
            mode: VisMode::Bars,
        }
        .seek_pulse_strength();
        let stopped = ChromeFxSignature {
            transport: ChromeTransportState::Stopped,
            focus: Focus::Playlist,
            mode: VisMode::Bars,
        }
        .seek_pulse_strength();

        assert!(playing > paused);
        assert_eq!(stopped, 0.0);
    }

    #[test]
    fn perimeter_index_walks_clockwise_once() {
        let area = Rect::new(10, 5, 4, 3);
        assert_eq!(perimeter_index_808(area, Position::new(10, 5)), Some(0));
        assert_eq!(perimeter_index_808(area, Position::new(13, 5)), Some(3));
        assert_eq!(perimeter_index_808(area, Position::new(13, 6)), Some(4));
        assert_eq!(perimeter_index_808(area, Position::new(13, 7)), Some(5));
        assert_eq!(perimeter_index_808(area, Position::new(12, 7)), Some(6));
        assert_eq!(perimeter_index_808(area, Position::new(11, 7)), Some(7));
        assert_eq!(perimeter_index_808(area, Position::new(10, 7)), Some(8));
        assert_eq!(perimeter_index_808(area, Position::new(10, 6)), Some(9));
        assert_eq!(perimeter_index_808(area, Position::new(11, 6)), None);
    }
}
