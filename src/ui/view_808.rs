use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Paragraph,
    canvas::{Canvas, Line as CanvasLine},
};

use super::App;
use super::keys::Focus;

/// 808 color constants.
const C808_RED: Color = Color::Rgb(0xD7, 0x26, 0x2E);
const C808_ORANGE: Color = Color::Rgb(0xF0, 0x5A, 0x28);
const C808_AMBER: Color = Color::Rgb(0xF6, 0xA6, 0x23);
const C808_YELLOW: Color = Color::Rgb(0xFF, 0xD4, 0x00);
const C808_GREY: Color = Color::Rgb(0xC9, 0xC9, 0xC9);
const C808_DIM: Color = Color::Rgb(0x66, 0x66, 0x66);

/// EQ frequency labels matching the 10-band EQ.
const EQ_LABELS: [&str; 10] = [
    "70", "180", "320", "600", "1k", "3k", "6k", "12k", "14k", "16k",
];

impl App {
    /// Render the 808 layout (called when mode_808 == true).
    pub fn render_808(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Content sizing — use most of the terminal width
        let content_width = 80u16.min(area.width);
        let content_height = area.height;
        let x = area.width.saturating_sub(content_width) / 2;
        let inner = Rect::new(x, 0, content_width, content_height);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // header
                Constraint::Length(4), // knob row 1 (vol + 5 EQ)
                Constraint::Length(4), // knob row 2 (5 EQ)
                Constraint::Length(1), // spacer
                Constraint::Length(2), // now playing + seek
                Constraint::Length(1), // status line (repeat/shuffle/mono)
                Constraint::Length(1), // spacer
                Constraint::Length(5), // spectrum LED
                Constraint::Length(1), // spacer
                Constraint::Min(3),    // playlist
                Constraint::Length(1), // help
                Constraint::Length(1), // error/save
            ])
            .split(inner);

        self.render_808_header(frame, chunks[0]);
        self.render_808_knob_row1(frame, chunks[1]);
        self.render_808_knob_row2(frame, chunks[2]);
        self.render_808_now_playing(frame, chunks[4]);
        self.render_808_status(frame, chunks[5]);
        self.render_808_spectrum(frame, chunks[7]);
        if self.focus == Focus::Provider {
            self.render_808_provider(frame, chunks[9]);
        } else {
            self.render_808_playlist(frame, chunks[9]);
        }
        self.render_808_help(frame, chunks[10]);

        if chunks.len() > 11 {
            self.render_808_status_line(frame, chunks[11]);
        }
    }

    fn render_808_header(&self, frame: &mut Frame, area: Rect) {
        let lines = vec![
            Line::from(Span::styled(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
                Style::default().fg(C808_DIM),
            )),
            Line::from(vec![
                Span::styled("  ▬▬▬  RHYTHM COMPOSER  ", Style::default().fg(C808_GREY)),
                Span::styled(
                    "TR-808",
                    Style::default()
                        .fg(C808_YELLOW)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  ▬▬▬   Computer Controlled",
                    Style::default().fg(C808_GREY),
                ),
            ]),
            Line::from(Span::styled(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
                Style::default().fg(C808_DIM),
            )),
        ];

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_knob_row1(&self, frame: &mut Frame, area: Rect) {
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
        render_knob(frame, cols[0], vol_norm, "VOL", false);

        // EQ bands 0-4
        let bands = self.player.eq_bands();
        for i in 0..5 {
            let norm = ((bands[i] + 12.0) / 24.0).clamp(0.0, 1.0);
            let selected = is_focused && self.eq_cursor == i;
            render_knob(frame, cols[i + 1], norm, EQ_LABELS[i], selected);
        }
    }

    fn render_808_knob_row2(&self, frame: &mut Frame, area: Rect) {
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
            render_knob(frame, cols[i], norm, EQ_LABELS[band_idx], selected);
        }

        // Preset indicator in the last slot
        let preset_name = self.eq_preset_name();
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("[{preset_name}]"),
                Style::default().fg(C808_DIM),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), cols[5]);
    }

    fn render_808_now_playing(&self, frame: &mut Frame, area: Rect) {
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

        let status_icon = if self.buffering {
            "◌"
        } else if self.player.is_playing() && self.player.is_paused() {
            "⏸"
        } else if self.player.is_playing() && !self.player.seekable() {
            "●"
        } else if self.player.is_playing() {
            "▶"
        } else {
            "■"
        };

        let time_str = if is_stream && dur_secs == 0 {
            format!("{pos_min:02}:{pos_sec:02}/--:--")
        } else {
            let dur_min = dur_secs / 60;
            let dur_sec = dur_secs % 60;
            format!("{pos_min:02}:{pos_sec:02}/{dur_min:02}:{dur_sec:02}")
        };
        let max_name = area.width as usize - time_str.len() - 5;
        let display_name: String = if name.chars().count() > max_name {
            let mut s: String = name.chars().take(max_name - 1).collect();
            s.push('…');
            s
        } else {
            name
        };

        let name_span = Span::styled(
            format!(" {status_icon} {display_name}"),
            Style::default()
                .fg(C808_ORANGE)
                .add_modifier(Modifier::BOLD),
        );
        let gap = area
            .width
            .saturating_sub(name_span.width() as u16 + time_str.len() as u16 + 1);
        let spaces = Span::raw(" ".repeat(gap as usize));
        let time_span = Span::styled(format!("{time_str} "), Style::default().fg(C808_GREY));

        let track_line = Line::from(vec![name_span, spaces, time_span]);

        // Seek bar
        let w = area.width as usize;
        let seek_line = if !self.player.seekable() && self.player.is_playing() {
            let label = "━━━ STREAMING ━━━";
            let pad = w.saturating_sub(label.len() + 1) / 2;
            let bar = format!(
                " {}{}{}",
                "━".repeat(pad),
                label,
                "━".repeat(w.saturating_sub(pad + label.len() + 1))
            );
            Line::from(Span::styled(bar, Style::default().fg(C808_DIM)))
        } else {
            let progress = if dur_secs > 0 {
                (pos_secs as f64 / dur_secs as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let filled = (progress * (w.saturating_sub(2)) as f64) as usize;
            Line::from(vec![
                Span::styled(" ", Style::default()),
                Span::styled("━".repeat(filled), Style::default().fg(C808_AMBER)),
                Span::styled("●", Style::default().fg(C808_YELLOW)),
                Span::styled(
                    "━".repeat(w.saturating_sub(filled + 2)),
                    Style::default().fg(C808_DIM),
                ),
            ])
        };

        frame.render_widget(Paragraph::new(vec![track_line, seek_line]), area);
    }

    fn render_808_status(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled(" ", Style::default())];

        // Repeat
        let repeat_str = format!("RPT:{}", self.playlist.repeat());
        if self.playlist.repeat() != crate::playlist::RepeatMode::Off {
            spans.push(Span::styled(
                repeat_str,
                Style::default()
                    .fg(C808_YELLOW)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(repeat_str, Style::default().fg(C808_DIM)));
        }

        spans.push(Span::raw("  "));

        // Shuffle
        if self.playlist.shuffled() {
            spans.push(Span::styled(
                "SHF",
                Style::default()
                    .fg(C808_YELLOW)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled("SHF", Style::default().fg(C808_DIM)));
        }

        spans.push(Span::raw("  "));

        // Mono
        if self.player.mono() {
            spans.push(Span::styled(
                "MONO",
                Style::default()
                    .fg(C808_ORANGE)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled("MONO", Style::default().fg(C808_DIM)));
        }

        spans.push(Span::raw("  "));

        // Volume dB readout
        spans.push(Span::styled(
            format!("{:+.1}dB", self.player.volume()),
            Style::default().fg(C808_GREY),
        ));

        // Queue count
        let q_len = self.playlist.queue_len();
        if q_len > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("Q:{q_len}"),
                Style::default().fg(C808_AMBER).add_modifier(Modifier::BOLD),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_808_spectrum(&mut self, frame: &mut Frame, area: Rect) {
        let samples = self.player.samples();
        let bands = self.vis.analyze(&samples);

        // LED-style columns using block elements with 808 color gradient
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
                // Color gradient: yellow at bottom, amber mid, orange high, red peak
                let color = if row_bottom >= 0.8 {
                    C808_RED
                } else if row_bottom >= 0.6 {
                    C808_ORANGE
                } else if row_bottom >= 0.3 {
                    C808_AMBER
                } else {
                    C808_YELLOW
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
                spans.push(Span::raw(" ")); // gap between bands
            }

            let y = area.y + row as u16;
            let line_area = Rect::new(area.x, y, area.width, 1);
            frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    fn render_808_playlist(&self, frame: &mut Frame, area: Rect) {
        let tracks = self.playlist.tracks();
        if tracks.is_empty() {
            let line = Line::from(Span::styled(
                "  No tracks loaded",
                Style::default().fg(C808_DIM),
            ));
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
            let mut style = Style::default().fg(C808_GREY);

            if i == current_idx && self.player.is_playing() {
                prefix = " ► ";
                style = Style::default()
                    .fg(C808_ORANGE)
                    .add_modifier(Modifier::BOLD);
            }

            if self.focus == Focus::Playlist && i == self.pl_cursor {
                style = Style::default()
                    .fg(C808_YELLOW)
                    .add_modifier(Modifier::BOLD);
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
                    Style::default().fg(C808_AMBER).add_modifier(Modifier::BOLD),
                ));
            }

            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_provider(&self, frame: &mut Frame, area: Rect) {
        if self.prov_loading {
            let name = self.provider_name();
            let line = Line::from(Span::styled(
                format!("  Loading {name}..."),
                Style::default().fg(C808_DIM),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        if self.provider_lists.is_empty() {
            let line = Line::from(Span::styled(
                "  No playlists found",
                Style::default().fg(C808_DIM),
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
                Style::default()
                    .fg(C808_YELLOW)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C808_GREY)
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

    fn render_808_search(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![Line::from(vec![
            Span::styled(" / ", Style::default().fg(C808_YELLOW)),
            Span::styled(&self.search_query, Style::default().fg(C808_GREY)),
            Span::styled(
                format!("  ({} found)", self.search_results.len()),
                Style::default().fg(C808_DIM),
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
                    .fg(C808_YELLOW)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C808_GREY)
            };

            lines.push(Line::from(Span::styled(
                format!("   {:02}. {display_name}", i + 1),
                style,
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_help(&self, frame: &mut Frame, area: Rect) {
        let text = if self.focus == Focus::Provider {
            "[↑↓]Navigate [Enter]Load Playlist [Tab]Focus [Q]Quit"
        } else if self.player.seekable() {
            "[Spc]⏯ [<>]Trk [←→]Seek [S]Save [+-]Vol [e]EQ [8]808 [/]Search [Tab]Focus [Q]Quit"
        } else {
            "[Spc]⏯ [<>]Trk [S]Save [+-]Vol [e]EQ [8]808 [/]Search [Tab]Focus [Q]Quit"
        };
        let line = Line::from(Span::styled(text, Style::default().fg(C808_DIM)));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_808_status_line(&self, frame: &mut Frame, area: Rect) {
        if let Some(ref err) = self.err {
            let line = Line::from(Span::styled(
                format!(" ERR: {err}"),
                Style::default().fg(C808_RED),
            ));
            frame.render_widget(Paragraph::new(line), area);
        } else if !self.save_msg.is_empty() {
            let line = Line::from(Span::styled(
                format!(" {}", self.save_msg),
                Style::default().fg(C808_ORANGE),
            ));
            frame.render_widget(Paragraph::new(line), area);
        }
    }
}

/// Render a single rotary knob using Canvas with Braille markers.
///
/// `value` is normalized 0.0-1.0.
/// The knob arc spans from 210deg (min) to -30deg (max), clockwise.
fn render_knob(frame: &mut Frame, area: Rect, value: f64, label: &str, selected: bool) {
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

    let accent = if selected { C808_YELLOW } else { C808_GREY };
    let fill_color = if selected { C808_ORANGE } else { C808_AMBER };

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
                ctx.print(px, py, Span::styled("·", Style::default().fg(C808_DIM)));
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
            .fg(C808_YELLOW)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(C808_GREY)
    };

    let label_line = Line::from(Span::styled(label, label_style));
    frame.render_widget(
        Paragraph::new(label_line).alignment(Alignment::Center),
        label_area,
    );
}
