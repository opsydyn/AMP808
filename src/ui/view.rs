use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::App;
use super::keys::Focus;
use super::styles::*;

impl App {
    /// Render the full TUI frame.
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        if self.show_keymap {
            self.render_keymap(frame, area);
            return;
        }

        // Center content in terminal
        let content_width = PANEL_WIDTH + 6; // panel + padding
        let content_height = 28u16; // approximate
        let x = area.width.saturating_sub(content_width) / 2;
        let y = area.height.saturating_sub(content_height) / 2;
        let inner = Rect::new(
            x,
            y,
            content_width.min(area.width),
            content_height.min(area.height),
        );

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // track info
                Constraint::Length(1), // time + status
                Constraint::Length(1), // spacer
                Constraint::Length(5), // spectrum
                Constraint::Length(1), // seek bar
                Constraint::Length(1), // spacer
                Constraint::Length(1), // volume
                Constraint::Length(1), // EQ
                Constraint::Length(1), // spacer
                Constraint::Length(1), // playlist header
                Constraint::Length(5), // playlist
                Constraint::Length(1), // spacer
                Constraint::Length(1), // help
                Constraint::Min(0),    // error/status + remainder
            ])
            .split(inner);

        self.render_title(frame, chunks[0]);
        self.render_track_info(frame, chunks[1]);
        self.render_time_status(frame, chunks[2]);
        self.render_spectrum(frame, chunks[4]);
        self.render_seek_bar(frame, chunks[5]);
        self.render_volume(frame, chunks[7]);
        self.render_eq(frame, chunks[8]);
        self.render_playlist_header(frame, chunks[10]);
        self.render_playlist(frame, chunks[11]);
        self.render_help(frame, chunks[13]);

        // Error / save message at the bottom
        if chunks.len() > 14 {
            self.render_status_line(frame, chunks[14]);
        }
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let line = Line::from(Span::styled("C L I A M P", title_style()));
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Left), area);
    }

    fn render_track_info(&self, frame: &mut Frame, area: Rect) {
        let name = if let Some((track, _)) = self.playlist.current() {
            track.display_name()
        } else {
            "No track loaded".to_string()
        };

        let max_w = PANEL_WIDTH as usize - 4;
        let display = if name.chars().count() <= max_w {
            format!("♫ {name}")
        } else {
            // Cyclic scrolling for long titles
            let runes: Vec<char> = name.chars().collect();
            let sep: Vec<char> = "   ♫   ".chars().collect();
            let mut padded = runes;
            padded.extend_from_slice(&sep);
            let total = padded.len();
            let off = self.title_off % total;

            let display: String = (0..max_w).map(|i| padded[(off + i) % total]).collect();
            format!("♫ {display}")
        };

        let line = Line::from(Span::styled(display, track_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_time_status(&self, frame: &mut Frame, area: Rect) {
        let (pos_secs, dur_secs) = self.track_position();
        let pos_min = pos_secs / 60;
        let pos_sec = pos_secs % 60;
        let dur_min = dur_secs / 60;
        let dur_sec = dur_secs % 60;

        let time_str = format!("{pos_min:02}:{pos_sec:02} / {dur_min:02}:{dur_sec:02}");

        let status = if self.buffering {
            Span::styled("◌ Buffering...", status_style())
        } else if self.player.is_playing() && self.player.is_paused() {
            Span::styled("⏸ Paused", status_style())
        } else if self.player.is_playing() {
            Span::styled("▶ Playing", status_style())
        } else {
            Span::styled("■ Stopped", dim_style())
        };

        let time_span = Span::styled(time_str, time_style());
        let gap = area
            .width
            .saturating_sub(time_span.width() as u16 + status.width() as u16);
        let spaces = Span::raw(" ".repeat(gap as usize));

        let line = Line::from(vec![time_span, spaces, status]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_spectrum(&mut self, frame: &mut Frame, area: Rect) {
        let samples = self.player.samples();
        let bands = self.vis.analyze(&samples);
        let spec_lines = self.vis.render(&bands);

        for (row, spec_line) in spec_lines.iter().enumerate() {
            if row >= area.height as usize {
                break;
            }
            let spans: Vec<Span> = spec_line
                .segments
                .iter()
                .flat_map(|seg| vec![Span::styled(&seg.text, spectrum_style(seg.row_bottom))])
                .collect();

            let y = area.y + row as u16;
            let line_area = Rect::new(area.x, y, area.width, 1);
            frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    fn render_seek_bar(&self, frame: &mut Frame, area: Rect) {
        let (pos_secs, dur_secs) = self.track_position();
        let progress = if dur_secs > 0 {
            (pos_secs as f64 / dur_secs as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let w = area.width as usize;
        let filled = (progress * (w.saturating_sub(1)) as f64) as usize;

        let mut spans = vec![
            Span::styled("━".repeat(filled), seek_fill_style()),
            Span::styled("●", seek_fill_style()),
        ];
        let remaining = w.saturating_sub(filled + 1);
        if remaining > 0 {
            spans.push(Span::styled("━".repeat(remaining), seek_dim_style()));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_volume(&self, frame: &mut Frame, area: Rect) {
        let vol = self.player.volume();
        let frac = ((vol + 30.0) / 36.0).clamp(0.0, 1.0);

        let bar_w = 30;
        let filled = (frac * bar_w as f64) as usize;

        let mut spans = vec![
            Span::styled("VOL ", label_style()),
            Span::styled("█".repeat(filled), vol_bar_style()),
            Span::styled("░".repeat(bar_w - filled), dim_style()),
            Span::styled(format!(" {:+.1}dB", vol), dim_style()),
        ];

        if self.player.mono() {
            spans.push(Span::styled(" [Mono]", active_toggle_style()));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_eq(&self, frame: &mut Frame, area: Rect) {
        let bands = self.player.eq_bands();
        let labels = [
            "70", "180", "320", "600", "1k", "3k", "6k", "12k", "14k", "16k",
        ];

        let mut spans = vec![Span::styled("EQ  ", label_style())];

        for (i, label) in labels.iter().enumerate() {
            let display = if bands[i] != 0.0 {
                format!("{:+.0}", bands[i])
            } else {
                label.to_string()
            };

            let style = if self.focus == Focus::EQ && i == self.eq_cursor {
                eq_active_style()
            } else {
                eq_inactive_style()
            };

            spans.push(Span::styled(display, style));
            if i < 9 {
                spans.push(Span::raw(" "));
            }
        }

        // Preset name
        let preset_name = self.eq_preset_name();
        spans.push(Span::styled(format!(" [{preset_name}]"), dim_style()));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_playlist_header(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled("── Playlist ── ", dim_style())];

        if self.playlist.shuffled() {
            spans.push(Span::styled("[Shuffle]", active_toggle_style()));
        } else {
            spans.push(Span::styled("[Shuffle]", dim_style()));
        }

        spans.push(Span::raw(" "));

        let repeat_str = format!("[Repeat: {}]", self.playlist.repeat());
        if self.playlist.repeat() != crate::playlist::RepeatMode::Off {
            spans.push(Span::styled(repeat_str, active_toggle_style()));
        } else {
            spans.push(Span::styled(repeat_str, dim_style()));
        }

        let q_len = self.playlist.queue_len();
        if q_len > 0 {
            spans.push(Span::styled(
                format!(" [Queue: {q_len}]"),
                active_toggle_style(),
            ));
        }

        spans.push(Span::styled(" ──", dim_style()));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_playlist(&self, frame: &mut Frame, area: Rect) {
        let tracks = self.playlist.tracks();
        if tracks.is_empty() {
            let line = Line::from(Span::styled("  No tracks loaded", dim_style()));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        if self.searching {
            self.render_search_results(frame, area);
            return;
        }

        let current_idx = self.playlist.index().unwrap_or(usize::MAX);
        let visible = (area.height as usize).min(tracks.len());

        let mut scroll = self.pl_scroll;
        if scroll + visible > tracks.len() {
            scroll = tracks.len().saturating_sub(visible);
        }

        let mut lines = Vec::with_capacity(visible);
        for i in scroll..scroll + visible {
            if i >= tracks.len() {
                break;
            }

            let mut prefix = "  ";
            let mut style = playlist_item_style();

            if i == current_idx && self.player.is_playing() {
                prefix = "▶ ";
                style = playlist_active_style();
            }

            if self.focus == Focus::Playlist && i == self.pl_cursor {
                style = playlist_selected_style();
            }

            let name = tracks[i].display_name();
            let max_w = PANEL_WIDTH as usize - 6;
            let display_name: String = if name.chars().count() > max_w {
                let mut s: String = name.chars().take(max_w - 1).collect();
                s.push('…');
                s
            } else {
                name
            };

            let mut spans = vec![Span::styled(
                format!("{prefix}{}. {display_name}", i + 1),
                style,
            )];

            let qp = self.playlist.queue_position(i);
            if qp > 0 {
                spans.push(Span::styled(format!(" [Q{qp}]"), active_toggle_style()));
            }

            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_search_results(&self, frame: &mut Frame, area: Rect) {
        if self.search_results.is_empty() {
            let text = if self.search_query.is_empty() {
                "  Type to search…"
            } else {
                "  No matches"
            };
            let line = Line::from(Span::styled(text, dim_style()));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        let tracks = self.playlist.tracks();
        let current_idx = self.playlist.index().unwrap_or(usize::MAX);
        let visible = (area.height as usize).min(self.search_results.len());

        let scroll = if self.search_cursor >= visible {
            self.search_cursor - visible + 1
        } else {
            0
        };

        let mut lines = Vec::with_capacity(visible);
        for j in scroll..scroll + visible {
            if j >= self.search_results.len() {
                break;
            }
            let i = self.search_results[j];

            let mut prefix = "  ";
            let mut style = playlist_item_style();

            if i == current_idx && self.player.is_playing() {
                prefix = "▶ ";
                style = playlist_active_style();
            }

            if j == self.search_cursor {
                style = playlist_selected_style();
            }

            let name = tracks[i].display_name();
            let max_w = PANEL_WIDTH as usize - 6;
            let display_name: String = if name.chars().count() > max_w {
                let mut s: String = name.chars().take(max_w - 1).collect();
                s.push('…');
                s
            } else {
                name
            };

            lines.push(Line::from(Span::styled(
                format!("{prefix}{}. {display_name}", i + 1),
                style,
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let text = if self.searching {
            let count = self.search_results.len();
            format!(
                "/ {}  ({count} found)  [↑↓]Navigate [Enter]Play [Esc]Cancel",
                self.search_query
            )
        } else {
            "[Spc]⏯  [<>]Trk [←→]Seek [+-]Vol [m]Mono [e]EQ [v]Vis [a]Queue [/]Search [Tab]Focus [Q]Quit".to_string()
        };

        let line = Line::from(Span::styled(text, help_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_status_line(&self, frame: &mut Frame, area: Rect) {
        if let Some(ref err) = self.err {
            let line = Line::from(Span::styled(format!("ERR: {err}"), error_style()));
            frame.render_widget(Paragraph::new(line), area);
        } else if !self.save_msg.is_empty() {
            let line = Line::from(Span::styled(&self.save_msg, status_style()));
            frame.render_widget(Paragraph::new(line), area);
        }
    }

    fn render_keymap(&self, frame: &mut Frame, area: Rect) {
        let keys = [
            ("Space", "Play / Pause"),
            ("s", "Stop"),
            ("> .", "Next track"),
            ("< ,", "Previous track"),
            ("← →", "Seek ±5s"),
            ("+ -", "Volume up/down"),
            ("m", "Toggle mono"),
            ("e", "Cycle EQ preset"),
            ("v", "Cycle visualizer"),
            ("↑ ↓", "Playlist scroll / EQ adjust"),
            ("h l", "EQ cursor left/right"),
            ("Enter", "Play selected track"),
            ("a", "Toggle queue (play next)"),
            ("S", "Save track to ~/Music"),
            ("r", "Cycle repeat"),
            ("z", "Toggle shuffle"),
            ("/", "Search playlist"),
            ("Tab", "Toggle focus"),
            ("Ctrl+K", "This keymap"),
            ("q", "Quit"),
        ];

        let mut lines = vec![
            Line::from(Span::styled("K E Y M A P", title_style())),
            Line::from(""),
        ];

        for (key, action) in &keys {
            lines.push(Line::from(Span::styled(
                format!("  {key:<10} {action}"),
                dim_style(),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press any key to close",
            help_style(),
        )));

        let block = Block::default().borders(Borders::NONE);
        let paragraph = Paragraph::new(lines).block(block);

        // Center
        let w = 50u16.min(area.width);
        let h = 24u16.min(area.height);
        let x = area.width.saturating_sub(w) / 2;
        let y = area.height.saturating_sub(h) / 2;
        let inner = Rect::new(x, y, w, h);

        frame.render_widget(paragraph, inner);
    }
}
