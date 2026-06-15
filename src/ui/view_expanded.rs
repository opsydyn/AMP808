use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, FrameExt as _, Paragraph};

use super::App;
use super::bpm::BpmDisplayState;
use super::keys::Focus;
use super::view_mode::ViewMode;

const MIN_WIDTH: u16 = 120;
const MIN_HEIGHT: u16 = 36;

impl App {
    pub fn render_expanded(&mut self, frame: &mut Frame) {
        let area = frame.area();

        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            self.render_expanded_size_guard(frame, area);
            return;
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),     // top panel row
                Constraint::Length(12), // spectrogram (10 rows + 2 border rows)
                Constraint::Length(1),  // help bar
            ])
            .split(area);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50), // now playing (art + metadata)
                Constraint::Percentage(20), // browser
                Constraint::Percentage(30), // playlist
            ])
            .split(rows[0]);

        self.render_expanded_now_playing(frame, cols[0]);
        self.render_expanded_browser(frame, cols[1]);
        self.render_expanded_playlist(frame, cols[2]);
        self.render_expanded_spectrogram(frame, rows[1]);
        self.render_expanded_help(frame, rows[2]);
    }

    // ── Browser pane ─────────────────────────────────────────────────────────

    fn render_expanded_browser(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Browser;
        let block = pane_block(
            " File Browser ",
            focused,
            self.palette.title_style(),
            self.palette.accent,
            self.palette.dim,
        );

        if let Some(ref explorer) = self.explorer {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            if inner.height == 0 {
                return;
            }
            let cwd = format!(" {}", explorer.cwd().display());
            let header_area = Rect::new(inner.x, inner.y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    truncate_str(&cwd, inner.width as usize),
                    self.palette.dim_style(),
                ))),
                header_area,
            );
            if inner.height > 1 {
                let list_area = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
                frame.render_widget_ref(explorer.widget(), list_area);
            }
        } else {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No browser open",
                    self.palette.dim_style(),
                ))),
                inner,
            );
        }
    }

    // ── Playlist pane ────────────────────────────────────────────────────────

    fn render_expanded_playlist(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Playlist;
        let title = format!(" ♪ Playlist ({}) ", self.playlist.len());
        let block = pane_block(
            &title,
            focused,
            self.palette.title_style(),
            self.palette.accent,
            self.palette.dim,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let tracks = self.playlist.tracks();
        if tracks.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No tracks loaded",
                    self.palette.dim_style(),
                ))),
                inner,
            );
            return;
        }

        if self.searching {
            self.render_expanded_search_results(frame, inner);
            return;
        }

        let current_idx = self.playlist.index().unwrap_or(usize::MAX);
        let visible = inner.height as usize;
        let scroll = self.pl_scroll.min(tracks.len().saturating_sub(1));

        // Width of the largest track number + dot + space e.g. "99. " = 4, "999. " = 5
        let num_w = format!("{}", tracks.len()).len() + 2; // digits + ". "

        let mut lines: Vec<Line> = Vec::with_capacity(visible);
        for (i, track) in tracks.iter().enumerate().skip(scroll).take(visible) {
            let is_playing = i == current_idx && self.player.is_playing();
            let is_cursor = self.focus == Focus::Playlist && i == self.pl_cursor;

            let item_style = if is_playing {
                self.palette.playlist_active_style()
            } else if is_cursor {
                self.palette.playlist_selected_style()
            } else {
                self.palette.playlist_item_style()
            };

            let play_icon = if is_playing { "▶" } else { " " };
            // track number right-aligned within num_w, e.g. " 1. " or "10. "
            let num_str = format!("{:>width$}. ", i + 1, width = num_w - 2);
            let reserved = 2 + num_str.len(); // play_icon + space + num
            let max_w = inner.width.saturating_sub(reserved as u16) as usize;
            let display = truncate_str(&track.display_name(), max_w);

            let num_style = if is_playing {
                self.palette.playlist_active_style()
            } else {
                self.palette.dim_style()
            };

            let mut spans = vec![
                Span::styled(format!("{play_icon} "), item_style),
                Span::styled(num_str, num_style),
                Span::styled(display, item_style),
            ];
            let qp = self.playlist.queue_position(i);
            if qp > 0 {
                spans.push(Span::styled(
                    format!(" [Q{qp}]"),
                    self.palette.active_toggle_style(),
                ));
            }
            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_expanded_search_results(&self, frame: &mut Frame, area: Rect) {
        if self.search_results.is_empty() {
            let text = if self.search_query.is_empty() {
                "  Type to search…"
            } else {
                "  No matches"
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(text, self.palette.dim_style()))),
                area,
            );
            return;
        }

        let tracks = self.playlist.tracks();
        let current_idx = self.playlist.index().unwrap_or(usize::MAX);
        let visible = area.height as usize;
        let scroll = if self.search_cursor >= visible {
            self.search_cursor - visible + 1
        } else {
            0
        };

        let mut lines = Vec::with_capacity(visible);
        for j in scroll..(scroll + visible).min(self.search_results.len()) {
            let i = self.search_results[j];
            let mut prefix = "  ";
            let mut style = self.palette.playlist_item_style();
            if i == current_idx && self.player.is_playing() {
                prefix = "▶ ";
                style = self.palette.playlist_active_style();
            }
            if j == self.search_cursor {
                style = self.palette.playlist_selected_style();
            }
            let name = tracks[i].display_name();
            let max_w = area.width.saturating_sub(4) as usize;
            let display = truncate_str(&name, max_w);
            lines.push(Line::from(Span::styled(
                format!("{prefix}{display}"),
                style,
            )));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    // ── Now Playing pane (art + metadata) ────────────────────────────────────

    fn render_expanded_now_playing(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::EQ;
        let block = pane_block(
            " Now Playing ",
            focused,
            self.palette.title_style(),
            self.palette.accent,
            self.palette.dim,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 {
            return;
        }

        // Art on the left when available; metadata fills the right (or all if no art).
        let proto = self
            .cover_art_proto_expanded
            .as_ref()
            .or(self.cover_art_proto.as_ref());
        let (art_col, meta_col) = if let Some(proto) = proto {
            if inner.width > 24 {
                // Art column = square sized to the inner height, capped at ~half the width
                let art_h = inner.height;
                let art_w = art_h.min(inner.width / 2).min(proto.area().width.max(1));
                let gap = 1u16;
                let meta_w = inner.width.saturating_sub(art_w + gap);
                let art_rect = Rect::new(inner.x, inner.y, art_w, art_h);
                let meta_rect = Rect::new(inner.x + art_w + gap, inner.y, meta_w, inner.height);
                (Some((art_rect, proto)), meta_rect)
            } else {
                (None, inner)
            }
        } else {
            (None, inner)
        };

        if let Some((art_rect, proto)) = art_col {
            let slot = self.cover_art_area_for_slot(art_rect, proto.area());
            self.render_cover_art_proto(frame, slot, proto);
            if self.view_mode == ViewMode::Drum808Expanded
                && let Some(deck_area) = deck_area_below_art_808(art_rect, slot)
            {
                self.render_808_art_deck(frame, deck_area);
            }
        }

        self.render_expanded_metadata(frame, meta_col);
    }

    fn render_808_art_deck(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" 808 Deck ", self.palette.title_style()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.palette.accent));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width < 16 || inner.height < 8 {
            return;
        }

        let pad_h = if inner.height >= 14 { 4 } else { 2 };
        let top_h = inner.height.saturating_sub(pad_h + 2).clamp(5, 12);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(top_h),
                Constraint::Length(pad_h),
                Constraint::Min(0),
            ])
            .split(inner);

        let dial_w = (inner.width.saturating_mul(3) / 5).clamp(12, 22);
        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(dial_w), Constraint::Min(8)])
            .split(rows[0]);

        let (bpm_norm, bpm_label) = match self.bpm.display {
            BpmDisplayState::Locked(bpm) => {
                let norm = ((bpm as f64 - 60.0) / 140.0).clamp(0.0, 1.0);
                (norm, bpm.to_string())
            }
            _ => {
                let (pos, dur) = self.track_position();
                let norm = if dur > 0 {
                    pos as f64 / dur as f64
                } else {
                    (self.title_off % 16) as f64 / 15.0
                }
                .clamp(0.0, 1.0);
                (norm, "--".to_string())
            }
        };

        super::view_808::render_tempo_dial(
            frame,
            top_cols[0],
            bpm_norm,
            bpm_label,
            self.colors_808(),
        );
        self.render_808_deck_switches(frame, top_cols[1]);
        self.render_808_deck_pad_bank(frame, rows[1]);
        if rows[2].height > 0 {
            frame.render_widget(
                Paragraph::new(self.deck_step_strip_line()).alignment(Alignment::Center),
                rows[2],
            );
        }
    }

    fn render_808_deck_switches(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let repeat = self.playlist.repeat().to_string();
        let scale = if self.player.mono() { "MONO" } else { "STEREO" };
        let shuffled = if self.playlist.shuffled() {
            "ON"
        } else {
            "OFF"
        };
        let queue = self.playlist.queue_len();
        let lines = vec![
            Line::from(Span::styled("PATTERN", self.palette.label_style())),
            Line::from(Span::styled(
                truncate_str(&repeat.to_ascii_uppercase(), area.width as usize),
                self.palette.track_style().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled("SCALE", self.palette.label_style())),
            Line::from(Span::styled(scale, self.palette.time_style())),
            Line::from(Span::styled("SHUF", self.palette.label_style())),
            Line::from(Span::styled(
                shuffled,
                if self.playlist.shuffled() {
                    self.palette.active_toggle_style()
                } else {
                    self.palette.dim_style()
                },
            )),
            Line::from(Span::styled(format!("Q {queue}"), self.palette.dim_style())),
        ];
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_808_deck_pad_bank(&self, frame: &mut Frame, area: Rect) {
        let pads = super::view_808::performance_pads_808(
            self.player.supports_seek(),
            self.player.supports_local_playlist(),
            self.playlist.shuffled(),
            self.playlist.repeat(),
            self.player.is_playing() && !self.player.is_paused(),
        );
        super::view_808::render_pad_bank_808(frame, area, &pads, self.colors_808());
    }

    fn deck_step_strip_line(&self) -> Line<'static> {
        let (pos, dur) = self.track_position();
        let active = if dur > 0 {
            ((pos as f64 / dur as f64).clamp(0.0, 1.0) * 15.0).round() as usize
        } else {
            self.title_off % 16
        };

        let mut spans = Vec::with_capacity(32);
        for idx in 0..16 {
            let fill = match idx {
                0..=3 => self.palette.error,
                4..=7 => self.palette.playing,
                8..=11 => self.palette.seek_bar,
                _ => self.palette.accent,
            };
            let style = if idx == active {
                Style::default()
                    .fg(Color::Black)
                    .bg(fill)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(fill)
            };
            spans.push(Span::styled(if idx == active { "■" } else { "●" }, style));
            if idx < 15 {
                spans.push(Span::raw(" "));
            }
        }
        Line::from(spans)
    }

    fn render_expanded_metadata(&self, frame: &mut Frame, area: Rect) {
        let (track_title, track_artist) = if let Some((track, _)) = self.playlist.current() {
            (track.title.clone(), track.artist.clone())
        } else if !self.stream_title.is_empty() {
            (self.stream_title.clone(), String::new())
        } else {
            ("No track loaded".to_string(), String::new())
        };

        let w = area.width as usize;
        let val_w = w.saturating_sub(7);

        let mut lines: Vec<Line> = Vec::new();

        // Title — bold + accent colour for visual weight
        lines.push(Line::from(vec![
            Span::styled("TITLE  ", self.palette.label_style()),
            Span::styled(
                truncate_str(&track_title, val_w),
                self.palette.track_style().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("ARTIST ", self.palette.label_style()),
            Span::styled(
                if track_artist.is_empty() {
                    "—".to_string()
                } else {
                    truncate_str(&track_artist, val_w)
                },
                self.palette.time_style(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("ALBUM  ", self.palette.label_style()),
            Span::styled("—", self.palette.dim_style()),
        ]));
        let sr_khz = self.player.sample_rate() / 1000;
        lines.push(Line::from(vec![
            Span::styled("INFO   ", self.palette.label_style()),
            Span::styled(format!("{sr_khz} kHz"), self.palette.dim_style()),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─".repeat(w),
            self.palette.dim_style(),
        )));

        lines.push(self.expanded_seek_bar_line(area.width));

        // Time row: elapsed / total  -remaining (right: status)
        let (status_text, status_style) = self.playback_status();
        let time = self.time_string();
        let (pos_secs, dur_secs) = self.track_position();
        let rem_str = if dur_secs > 0 && !self.player.is_streaming() {
            let rem = dur_secs.saturating_sub(pos_secs);
            format!("  -{:02}:{:02}", rem / 60, rem % 60)
        } else {
            String::new()
        };
        let time_with_rem = format!("{time}{rem_str}");
        let gap = w.saturating_sub(time_with_rem.len() + status_text.len() + 1);
        lines.push(Line::from(vec![
            Span::styled(time_with_rem, self.palette.time_style()),
            Span::raw(" ".repeat(gap)),
            Span::styled(status_text, status_style),
        ]));

        lines.push(Line::from(""));
        lines.push(self.expanded_badge_line());
        lines.push(self.expanded_volume_line(area.width));

        if self.player.supports_eq() {
            lines.push(Line::from(""));
            lines.push(self.expanded_eq_line());
        }

        // Next track preview
        if let Some(next) = self.playlist.peek_next() {
            let next_name = next.display_name();
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("NEXT   ", self.palette.label_style()),
                Span::styled(truncate_str(&next_name, val_w), self.palette.dim_style()),
            ]));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn expanded_seek_bar_line(&self, width: u16) -> Line<'static> {
        let w = width as usize;
        if self.player.is_streaming() {
            return Line::from(Span::styled("━".repeat(w), self.palette.seek_dim_style()));
        }
        let (pos_secs, dur_secs) = self.track_position();
        let progress = if dur_secs > 0 {
            (pos_secs as f64 / dur_secs as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let filled = (progress * w.saturating_sub(1) as f64) as usize;
        let remaining = w.saturating_sub(filled + 1);

        let mut spans: Vec<Span<'static>> = vec![
            Span::styled("━".repeat(filled), self.palette.seek_fill_style()),
            Span::styled("●".to_string(), self.palette.seek_fill_style()),
        ];
        if remaining > 0 {
            spans.push(Span::styled(
                "━".repeat(remaining),
                self.palette.seek_dim_style(),
            ));
        }
        Line::from(spans)
    }

    fn expanded_badge_line(&self) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();

        let repeat_label = format!("[{}]", self.playlist.repeat());
        let rpt_style = if self.playlist.repeat() != crate::playlist::RepeatMode::Off {
            self.palette.active_toggle_style()
        } else {
            self.palette.dim_style()
        };
        spans.push(Span::styled(repeat_label, rpt_style));
        spans.push(Span::raw(" "));

        let shuf_style = if self.playlist.shuffled() {
            self.palette.active_toggle_style()
        } else {
            self.palette.dim_style()
        };
        spans.push(Span::styled("[SHUF]".to_string(), shuf_style));

        if self.player.mono() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "[MONO]".to_string(),
                self.palette.active_toggle_style(),
            ));
        }

        let bpm_text = self.bpm.standard_text();
        if !bpm_text.is_empty() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(bpm_text, self.palette.dim_style()));
        }

        let q_len = self.playlist.queue_len();
        if q_len > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("[Q:{q_len}]"),
                self.palette.active_toggle_style(),
            ));
        }

        Line::from(spans)
    }

    fn expanded_volume_line(&self, width: u16) -> Line<'static> {
        let vol = self.player.volume();
        let frac = ((vol + 30.0) / 36.0).clamp(0.0, 1.0);
        let bar_w = (width as usize).saturating_sub(16).clamp(10, 30);
        let filled = (frac * bar_w as f64) as usize;

        Line::from(vec![
            Span::styled("VOL ".to_string(), self.palette.label_style()),
            Span::styled("█".repeat(filled), self.palette.vol_bar_style()),
            Span::styled("░".repeat(bar_w - filled), self.palette.dim_style()),
            Span::styled(format!(" {:+.1}dB", vol), self.palette.dim_style()),
        ])
    }

    fn expanded_eq_line(&self) -> Line<'static> {
        let bands = self.player.eq_bands();
        let labels = [
            "70", "180", "320", "600", "1k", "3k", "6k", "12k", "14k", "16k",
        ];

        let mut spans: Vec<Span<'static>> =
            vec![Span::styled("EQ  ".to_string(), self.palette.label_style())];

        for (i, label) in labels.iter().enumerate() {
            let display = if bands[i] != 0.0 {
                format!("{:+.0}", bands[i])
            } else {
                label.to_string()
            };
            // Cursor: accent highlight. Otherwise: warm→cool gradient across freq range.
            let style = if self.focus == Focus::EQ && i == self.eq_cursor {
                self.palette.eq_active_style()
            } else {
                self.palette.spectrum_style(i as f64 / 9.0)
            };
            spans.push(Span::styled(display, style));
            if i < 9 {
                spans.push(Span::raw(" "));
            }
        }
        Line::from(spans)
    }

    // ── Spectrogram ──────────────────────────────────────────────────────────

    fn render_expanded_spectrogram(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(
                " Visualizer — Spectrogram ",
                self.palette.title_style(),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.palette.dim));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Compute current bands and push to rolling history
        let bands = if self.player.is_music_app() {
            let (pos_secs, _) = self.track_position();
            self.vis.synthetic_bands(
                pos_secs as f64,
                self.player.is_playing(),
                self.player.is_paused(),
            )
        } else {
            let samples = self.player.samples();
            self.vis.analyze(&samples)
        };

        let cap = inner.width as usize;
        self.vis.spectrogram_history.push_back(bands);
        while self.vis.spectrogram_history.len() > cap {
            self.vis.spectrogram_history.pop_front();
        }

        // Render: rows = bands (high → low), cols = time (oldest left → newest right)
        let n_bands = 10usize;
        let n_rows = (inner.height as usize).min(n_bands);
        let hist_len = self.vis.spectrogram_history.len();
        let history: Vec<_> = self.vis.spectrogram_history.iter().collect();

        for row in 0..n_rows {
            let band_idx = n_bands - 1 - row; // row 0 = highest freq band
            let y = inner.y + row as u16;
            let line_area = Rect::new(inner.x, y, inner.width, 1);

            let mut spans: Vec<Span<'static>> = Vec::with_capacity(cap);
            for col in 0..cap {
                let amp = if col < cap.saturating_sub(hist_len) {
                    0.0
                } else {
                    let hist_idx = col - (cap - hist_len);
                    history[hist_idx][band_idx]
                };
                let color = spectrogram_color(amp);
                spans.push(Span::styled(" ".to_string(), Style::default().bg(color)));
            }

            frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    // ── Help bar ─────────────────────────────────────────────────────────────

    fn render_expanded_help(&self, frame: &mut Frame, area: Rect) {
        let text =
            " w compact  Tab focus  Space play  n/p next/prev  ←→ seek  +/- vol  8 808  q quit";
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, self.palette.help_style()))),
            area,
        );
    }

    // ── 808 Expanded entry point ──────────────────────────────────────────────

    // Header = 6 rows; knobs = 7; visualizer = 12; help = 1 → 26 fixed rows.
    const MIN_HEIGHT_808: u16 = 44;

    pub fn render_808_expanded(&mut self, frame: &mut Frame) {
        let area = frame.area();

        if area.width < MIN_WIDTH || area.height < Self::MIN_HEIGHT_808 {
            self.render_expanded_size_guard(frame, area);
            return;
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),  // TR-808 header (full width)
                Constraint::Min(0),     // top pane row
                Constraint::Length(7),  // 808 knob bar (5 inner + 2 border → 4 canvas rows)
                Constraint::Length(12), // 808 visualizer (10 inner + 2 border)
                Constraint::Length(1),  // help bar
            ])
            .split(area);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50), // now playing (art + metadata)
                Constraint::Percentage(20), // browser
                Constraint::Percentage(30), // playlist
            ])
            .split(rows[1]);

        self.render_808_header(frame, rows[0]);
        self.render_expanded_now_playing(frame, cols[0]);
        self.render_expanded_browser(frame, cols[1]);
        self.render_expanded_playlist(frame, cols[2]);
        self.render_808_expanded_knobs(frame, rows[2]);
        self.render_808_expanded_visualizer(frame, rows[3]);
        self.render_808_expanded_help(frame, rows[4]);
    }

    fn render_808_expanded_knobs(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(
                " 808 Controls — VOL · EQ ",
                self.palette.title_style(),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.palette.accent));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width < 44 || inner.height == 0 {
            return;
        }

        // 11 knobs across the full width: VOL + 10 EQ bands
        let knob_w = inner.width / 11;
        let constraints: Vec<Constraint> = (0..11).map(|_| Constraint::Length(knob_w)).collect();
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(inner);

        let colors = self.colors_808();
        let is_eq_focused = self.focus == super::keys::Focus::EQ;
        let vol = self.player.volume();
        let vol_norm = ((vol + 30.0) / 36.0).clamp(0.0, 1.0);
        super::view_808::render_knob(frame, cols[0], vol_norm, "VOL", false, colors);

        let bands = self.player.eq_bands();
        let eq_labels = super::view_808::EQ_LABELS;
        for i in 0..10 {
            let norm = ((bands[i] + 12.0) / 24.0).clamp(0.0, 1.0);
            let selected = is_eq_focused && self.eq_cursor == i;
            super::view_808::render_knob(frame, cols[i + 1], norm, eq_labels[i], selected, colors);
        }
    }

    fn render_808_expanded_visualizer(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" 808 Visualizer ", self.palette.title_style()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.palette.accent));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Split inner: TEMPO dial on the left, spectrum bars on the right.
        const DIAL_W: u16 = 24;
        const GAP: u16 = 1;
        if inner.width > DIAL_W + GAP + 10 {
            let dial_rect = Rect::new(inner.x, inner.y, DIAL_W, inner.height);
            let spec_rect = Rect::new(
                inner.x + DIAL_W + GAP,
                inner.y,
                inner.width.saturating_sub(DIAL_W + GAP),
                inner.height,
            );

            let (bpm_norm, bpm_label) = match self.bpm.display {
                BpmDisplayState::Locked(bpm) => {
                    let norm = ((bpm as f64 - 60.0) / 140.0).clamp(0.0, 1.0);
                    (norm, bpm.to_string())
                }
                _ => (0.0_f64, "--".to_string()),
            };

            super::view_808::render_tempo_dial(
                frame,
                dial_rect,
                bpm_norm,
                bpm_label,
                self.colors_808(),
            );
            self.render_808_spectrum(frame, spec_rect);
        } else {
            self.render_808_spectrum(frame, inner);
        }
    }

    fn render_808_expanded_help(&self, frame: &mut Frame, area: Rect) {
        let chips: &[(&str, &str)] = &[
            ("W", "compact"),
            ("8", "Waveform"),
            ("Tab", "focus"),
            ("Space", "play"),
            ("n/p", "next/prev"),
            ("←→", "seek"),
            ("+/-", "vol"),
            ("q", "quit"),
        ];

        let key_bg = self.colors_808().keycap();
        let text = self.palette.text;

        let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
        for (key, label) in chips {
            spans.push(Span::styled(
                format!(" {key} "),
                Style::default().fg(Color::Black).bg(key_bg),
            ));
            spans.push(Span::styled(
                format!(" {label}  "),
                Style::default().fg(text),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    // ── Size guard ───────────────────────────────────────────────────────────

    fn render_expanded_size_guard(&self, frame: &mut Frame, area: Rect) {
        let msg = format!(
            " Terminal too small for expanded mode ({}×{} — need {}×{}).  Press w to return to compact. ",
            area.width, area.height, MIN_WIDTH, MIN_HEIGHT,
        );
        let y = area.height / 2;
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, self.palette.error_style()))),
            Rect::new(area.x, area.y + y, area.width, 1),
        );
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn pane_block<'a>(
    title: &'a str,
    focused: bool,
    title_style: ratatui::style::Style,
    accent: Color,
    dim: Color,
) -> Block<'a> {
    let border_color = if focused { accent } else { dim };
    Block::default()
        .title(Span::styled(title, title_style))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
}

fn spectrogram_color(amp: f64) -> Color {
    let a = amp.clamp(0.0, 1.0);
    if a < 0.10 {
        lerp_rgb((8, 0, 20), (30, 0, 50), a / 0.10)
    } else if a < 0.25 {
        lerp_rgb((30, 0, 50), (60, 0, 80), (a - 0.10) / 0.15)
    } else if a < 0.45 {
        lerp_rgb((60, 0, 80), (140, 0, 0), (a - 0.25) / 0.20)
    } else if a < 0.65 {
        lerp_rgb((140, 0, 0), (200, 60, 0), (a - 0.45) / 0.20)
    } else if a < 0.80 {
        lerp_rgb((200, 60, 0), (230, 180, 0), (a - 0.65) / 0.15)
    } else if a < 0.95 {
        lerp_rgb((230, 180, 0), (255, 240, 60), (a - 0.80) / 0.15)
    } else {
        lerp_rgb((255, 240, 60), (255, 255, 255), (a - 0.95) / 0.05)
    }
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::Rgb(
        (a.0 as f64 + (b.0 as f64 - a.0 as f64) * t) as u8,
        (a.1 as f64 + (b.1 as f64 - a.1 as f64) * t) as u8,
        (a.2 as f64 + (b.2 as f64 - a.2 as f64) * t) as u8,
    )
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let mut r: String = chars[..max_chars - 1].iter().collect();
        r.push('…');
        r
    } else {
        chars[..max_chars].iter().collect()
    }
}

fn deck_area_below_art_808(art_col: Rect, art_slot: Rect) -> Option<Rect> {
    const GAP: u16 = 1;
    const MIN_DECK_SIZE: u16 = 14;

    let deck_y = art_slot.bottom().saturating_add(GAP);
    let remaining_h = art_col.bottom().saturating_sub(deck_y);
    let deck_size = art_slot.width.min(remaining_h);
    if deck_size < MIN_DECK_SIZE {
        return None;
    }

    Some(Rect::new(
        art_slot.x + art_slot.width.saturating_sub(deck_size) / 2,
        deck_y,
        deck_size,
        deck_size,
    ))
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use tokio::sync::mpsc;

    use crate::external::music_app::{MusicAppPlayerState, MusicAppSnapshot};
    use crate::playback_backend::PlaybackBackend;
    use crate::playlist::Playlist;
    use crate::resolve::ytdl::YtdlTempTracker;
    use crate::ui::App;
    use crate::ui::view_mode::ViewMode;

    fn build_test_app(view_mode: ViewMode) -> App {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            PlaybackBackend::music_app_for_test(MusicAppSnapshot {
                state: MusicAppPlayerState::Playing,
                volume: 50,
                title: "Numbers".into(),
                artist: "Kraftwerk".into(),
                album: "Computer World".into(),
                position_secs: 12.0,
                duration_secs: 300.0,
            }),
            Playlist::new(),
            YtdlTempTracker::new(),
            tx,
            None,
        );
        app.view_mode = view_mode;
        app.refresh_palette();
        app
    }

    fn rendered_line(app: &mut App, width: u16, height: u16, y: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");
        terminal
            .draw(|frame| app.render(frame))
            .expect("render should succeed");

        let buffer = terminal.backend().buffer();
        (0..width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    }

    #[test]
    fn expanded_layout_orders_now_playing_browser_playlist_from_left_to_right() {
        let mut app = build_test_app(ViewMode::Expanded);

        let title_row = rendered_line(&mut app, 120, 44, 0);

        let now_playing = title_row
            .find("Now Playing")
            .expect("now playing pane title");
        let browser = title_row.find("File Browser").expect("browser pane title");
        let playlist = title_row.find("Playlist").expect("playlist pane title");
        assert!(now_playing < browser);
        assert!(browser < playlist);
    }

    #[test]
    fn expanded_808_layout_orders_now_playing_browser_playlist_from_left_to_right() {
        let mut app = build_test_app(ViewMode::Drum808Expanded);

        let title_row = rendered_line(&mut app, 120, 44, 6);

        let now_playing = title_row
            .find("Now Playing")
            .expect("now playing pane title");
        let browser = title_row.find("File Browser").expect("browser pane title");
        let playlist = title_row.find("Playlist").expect("playlist pane title");
        assert!(now_playing < browser);
        assert!(browser < playlist);
    }

    #[test]
    fn expanded_808_deck_uses_square_space_below_album_art() {
        let art_col = ratatui::layout::Rect::new(4, 10, 32, 64);
        let art_slot = ratatui::layout::Rect::new(4, 10, 32, 24);

        assert_eq!(
            super::deck_area_below_art_808(art_col, art_slot),
            Some(ratatui::layout::Rect::new(4, 35, 32, 32))
        );
    }

    #[test]
    fn expanded_808_deck_hides_when_space_below_art_is_too_short() {
        let art_col = ratatui::layout::Rect::new(4, 10, 32, 30);
        let art_slot = ratatui::layout::Rect::new(4, 10, 32, 24);

        assert_eq!(super::deck_area_below_art_808(art_col, art_slot), None);
    }

    #[test]
    fn expanded_808_deck_renders_keyboard_performance_pads() {
        let app = build_test_app(ViewMode::Drum808Expanded);
        let backend = TestBackend::new(34, 34);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");

        terminal
            .draw(|frame| app.render_808_art_deck(frame, frame.area()))
            .expect("render should succeed");

        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("PLAY"));
        assert!(rendered.contains("LOAD"));
        assert!(rendered.contains("SAVE"));
    }

    #[test]
    fn expanded_808_help_buttons_use_ivory_keycaps() {
        let mut app = build_test_app(ViewMode::Drum808Expanded);
        let backend = TestBackend::new(120, 44);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");

        terminal
            .draw(|frame| app.render(frame))
            .expect("render should succeed");

        let buffer = terminal.backend().buffer();
        let ivory = ratatui::style::Color::Rgb(0xEE, 0xEA, 0xD8);
        let saw_ivory_key = buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "W" && cell.bg == ivory);

        assert!(saw_ivory_key);
    }
}
