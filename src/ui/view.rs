use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, FrameExt as _, Paragraph};

use super::App;
use super::keys::Focus;
use super::styles::PANEL_WIDTH;
use super::theme;
use super::visualizer::{SpectrumSegment, SpectrumSegmentKind, VisMode};

impl App {
    /// Render the full TUI frame.
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let tall_visual_mode = matches!(self.vis.mode, VisMode::Retro | VisMode::Logo);

        if self.show_keymap {
            self.render_keymap(frame, area);
            return;
        }

        if self.show_themes {
            self.render_theme_picker(frame, area);
            return;
        }

        if self.mode_808 {
            self.render_808(frame);
            return;
        }

        // Center content in terminal
        let content_width = PANEL_WIDTH + 6; // panel + padding
        let spectrum_height = if tall_visual_mode { 8u16 } else { 5u16 };
        let content_height = if tall_visual_mode { 31u16 } else { 28u16 };
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
                Constraint::Length(1),               // title
                Constraint::Length(1),               // track info
                Constraint::Length(1),               // time + status
                Constraint::Length(1),               // spacer
                Constraint::Length(spectrum_height), // spectrum
                Constraint::Length(1),               // seek bar
                Constraint::Length(1),               // spacer
                Constraint::Length(1),               // volume
                Constraint::Length(1),               // EQ
                Constraint::Length(1),               // spacer
                Constraint::Length(1),               // playlist header
                Constraint::Length(5),               // playlist
                Constraint::Length(1),               // spacer
                Constraint::Length(1),               // help
                Constraint::Min(0),                  // error/status + remainder
            ])
            .split(inner);

        self.render_title(frame, chunks[0]);
        self.render_track_info(frame, chunks[1]);
        let show_art = self.show_cover_art && self.cover_art_proto.is_some();
        if show_art {
            let time_status_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40), Constraint::Length(24)])
                .split(chunks[2]);
            self.render_time_only(frame, time_status_chunks[0]);
            self.render_status_only(frame, time_status_chunks[1]);
        } else {
            self.render_time_status(frame, chunks[2]);
        }

        // Split spectrum area for album art when available
        if show_art {
            let spec_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40), Constraint::Length(24)])
                .split(chunks[4]);
            self.render_spectrum(frame, spec_chunks[0]);
            // Anchor art under the play status row while keeping it in the spectrum zone.
            // ratatui-image Fit mode may encode a narrower protocol area (e.g. square art in a
            // wide slot), so center that effective area in the right column.
            let art_slot = Rect::new(
                spec_chunks[1].x,
                chunks[3].y,
                spec_chunks[1].width,
                spec_chunks[1].height,
            );
            let art_area = if let Some(proto) = self.cover_art_proto.as_ref() {
                self.cover_art_area_for_slot(art_slot, proto.area())
            } else {
                art_slot
            };
            self.render_cover_art(frame, art_area);
        } else {
            self.render_spectrum(frame, chunks[4]);
        }
        self.render_seek_bar(frame, chunks[5]);
        self.render_volume(frame, chunks[7]);
        self.render_eq(frame, chunks[8]);
        if self.focus == Focus::Provider {
            self.render_provider_header(frame, chunks[10]);
            self.render_provider_list(frame, chunks[11]);
        } else if self.focus == Focus::Browser {
            self.render_browser_header(frame, chunks[10]);
            self.render_browser(frame, chunks[11]);
        } else {
            self.render_playlist_header(frame, chunks[10]);
            self.render_playlist(frame, chunks[11]);
        }
        self.render_help(frame, chunks[13]);

        // Error / save message at the bottom
        if chunks.len() > 14 {
            self.render_status_line(frame, chunks[14]);
        }
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let line = Line::from(Span::styled("A M P 8 0 8", self.palette.title_style()));
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Left), area);
    }

    fn render_track_info(&self, frame: &mut Frame, area: Rect) {
        let name = if !self.stream_title.is_empty() {
            self.stream_title.clone()
        } else if let Some((track, _)) = self.playlist.current() {
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

        let line = Line::from(Span::styled(display, self.palette.track_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn time_string(&self) -> String {
        let (pos_secs, dur_secs) = self.track_position();
        let pos_min = pos_secs / 60;
        let pos_sec = pos_secs % 60;

        let is_stream = self
            .playlist
            .current()
            .map(|(t, _)| t.stream)
            .unwrap_or(false);

        if is_stream && dur_secs == 0 {
            format!("{pos_min:02}:{pos_sec:02} / --:--")
        } else {
            let dur_min = dur_secs / 60;
            let dur_sec = dur_secs % 60;
            format!("{pos_min:02}:{pos_sec:02} / {dur_min:02}:{dur_sec:02}")
        }
    }

    fn playback_status(&self) -> (String, Style) {
        if self.buffering {
            ("◌ Buffering...".to_string(), self.palette.status_style())
        } else if self.player.is_playing() && self.player.is_paused() {
            ("⏸ Paused".to_string(), self.palette.status_style())
        } else if self.player.is_playing() && self.player.is_streaming() {
            ("● Streaming".to_string(), self.palette.status_style())
        } else if self.player.is_playing() {
            ("▶ Playing".to_string(), self.palette.status_style())
        } else {
            ("■ Stopped".to_string(), self.palette.dim_style())
        }
    }

    fn render_time_only(&self, frame: &mut Frame, area: Rect) {
        let time_span = Span::styled(self.time_string(), self.palette.time_style());
        frame.render_widget(Paragraph::new(Line::from(time_span)), area);
    }

    fn render_status_only(&self, frame: &mut Frame, area: Rect) {
        let (status_text, status_style) = self.playback_status();
        let status_span = Span::styled(
            format!("{status_text} {}", self.bpm.standard_text()),
            status_style,
        );
        frame.render_widget(
            Paragraph::new(Line::from(status_span)).alignment(Alignment::Center),
            area,
        );
    }

    fn render_time_status(&self, frame: &mut Frame, area: Rect) {
        let time_span = Span::styled(self.time_string(), self.palette.time_style());
        let (status_text, status_style) = self.playback_status();
        let status_span = Span::styled(
            format!("{status_text} {}", self.bpm.standard_text()),
            status_style,
        );
        let gap = area
            .width
            .saturating_sub(time_span.width() as u16 + status_span.width() as u16);
        let spaces = Span::raw(" ".repeat(gap as usize));

        let line = Line::from(vec![time_span, spaces, status_span]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_spectrum(&mut self, frame: &mut Frame, area: Rect) {
        let spec_lines = if self.player.is_music_app() {
            let (pos_secs, _) = self.track_position();
            let animate = self.player.is_playing() && !self.player.is_paused();
            let bands = self.vis.synthetic_bands(
                pos_secs as f64,
                self.player.is_playing(),
                self.player.is_paused(),
            );
            if self.vis.mode == VisMode::Retro {
                self.vis
                    .render_retro(&bands, area.width as usize, area.height as usize, animate)
            } else if self.vis.mode == VisMode::Logo {
                self.vis
                    .render_logo(&bands, area.width as usize, area.height as usize, animate)
            } else {
                self.vis.render_synthetic(&bands)
            }
        } else {
            let samples = self.player.samples();
            if self.vis.mode == VisMode::Retro {
                let bands = self.vis.analyze(&samples);
                let animate = self.player.is_playing() && !self.player.is_paused();
                self.vis
                    .render_retro(&bands, area.width as usize, area.height as usize, animate)
            } else if self.vis.mode == VisMode::Logo {
                let bands = self.vis.analyze(&samples);
                let animate = self.player.is_playing() && !self.player.is_paused();
                self.vis
                    .render_logo(&bands, area.width as usize, area.height as usize, animate)
            } else if self.vis.mode == VisMode::Scope {
                self.vis
                    .render_scope(&samples, area.width as usize, area.height as usize)
            } else {
                let bands = self.vis.analyze(&samples);
                self.vis.render(&bands)
            }
        };

        for (row, spec_line) in spec_lines.iter().enumerate() {
            if row >= area.height as usize {
                break;
            }
            let spans: Vec<Span> = spec_line
                .segments
                .iter()
                .flat_map(|seg| vec![Span::styled(&seg.text, self.spectrum_segment_style(seg))])
                .collect();

            let y = area.y + row as u16;
            let line_area = Rect::new(area.x, y, area.width, 1);
            frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    pub fn render_cover_art(&self, frame: &mut Frame, area: Rect) {
        if let Some(ref proto) = self.cover_art_proto {
            self.render_cover_art_proto(frame, area, proto);
        }
    }

    pub fn render_cover_art_proto(
        &self,
        frame: &mut Frame,
        area: Rect,
        proto: &ratatui_image::protocol::Protocol,
    ) {
        let image = ratatui_image::Image::new(proto);
        frame.render_widget(image, area);
    }

    pub fn cover_art_area_for_slot(&self, slot: Rect, proto_area: Rect) -> Rect {
        let w = proto_area.width.min(slot.width);
        let h = proto_area.height.min(slot.height);
        Rect::new(slot.x + slot.width.saturating_sub(w) / 2, slot.y, w, h)
    }

    pub fn cover_art_proto_for_808(
        &self,
        tall_visual_mode: bool,
    ) -> Option<&ratatui_image::protocol::Protocol> {
        if tall_visual_mode {
            self.cover_art_proto_808
                .as_ref()
                .or(self.cover_art_proto.as_ref())
        } else {
            self.cover_art_proto.as_ref()
        }
    }

    fn spectrum_segment_style(&self, seg: &SpectrumSegment) -> Style {
        match seg.kind {
            SpectrumSegmentKind::Gradient => self.palette.spectrum_style(seg.row_bottom),
            SpectrumSegmentKind::RetroGrid => Style::default().fg(self.palette.spectrum_low),
            SpectrumSegmentKind::RetroSun => Style::default()
                .fg(self.palette.spectrum_mid)
                .add_modifier(Modifier::BOLD),
            SpectrumSegmentKind::RetroWave => Style::default()
                .fg(self.palette.spectrum_high)
                .add_modifier(Modifier::BOLD),
        }
    }

    fn render_seek_bar(&self, frame: &mut Frame, area: Rect) {
        let w = area.width as usize;

        // Non-seekable streams: static streaming bar
        if self.player.is_streaming() {
            let label = "━━━ STREAMING ━━━";
            let pad = w.saturating_sub(label.len()) / 2;
            let bar = format!(
                "{}{}{}",
                "━".repeat(pad),
                label,
                "━".repeat(w.saturating_sub(pad + label.len()))
            );
            let line = Line::from(Span::styled(bar, self.palette.seek_dim_style()));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        let (pos_secs, dur_secs) = self.track_position();
        let progress = if dur_secs > 0 {
            (pos_secs as f64 / dur_secs as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let filled = (progress * (w.saturating_sub(1)) as f64) as usize;

        let mut spans = vec![
            Span::styled("━".repeat(filled), self.palette.seek_fill_style()),
            Span::styled("●", self.palette.seek_fill_style()),
        ];
        let remaining = w.saturating_sub(filled + 1);
        if remaining > 0 {
            spans.push(Span::styled(
                "━".repeat(remaining),
                self.palette.seek_dim_style(),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_volume(&self, frame: &mut Frame, area: Rect) {
        let vol = self.player.volume();
        let frac = ((vol + 30.0) / 36.0).clamp(0.0, 1.0);

        let bar_w = 30;
        let filled = (frac * bar_w as f64) as usize;

        let mut spans = vec![
            Span::styled("VOL ", self.palette.label_style()),
            Span::styled("█".repeat(filled), self.palette.vol_bar_style()),
            Span::styled("░".repeat(bar_w - filled), self.palette.dim_style()),
            Span::styled(format!(" {:+.1}dB", vol), self.palette.dim_style()),
        ];

        if self.player.mono() {
            spans.push(Span::styled(" [Mono]", self.palette.active_toggle_style()));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_eq(&self, frame: &mut Frame, area: Rect) {
        let bands = self.player.eq_bands();
        let labels = [
            "70", "180", "320", "600", "1k", "3k", "6k", "12k", "14k", "16k",
        ];

        let mut spans = vec![Span::styled("EQ  ", self.palette.label_style())];

        for (i, label) in labels.iter().enumerate() {
            let display = if bands[i] != 0.0 {
                format!("{:+.0}", bands[i])
            } else {
                label.to_string()
            };

            let style = if self.focus == Focus::EQ && i == self.eq_cursor {
                self.palette.eq_active_style()
            } else {
                self.palette.eq_inactive_style()
            };

            spans.push(Span::styled(display, style));
            if i < 9 {
                spans.push(Span::raw(" "));
            }
        }

        // Preset name
        let preset_name = self.eq_preset_name();
        spans.push(Span::styled(
            format!(" [{preset_name}]"),
            self.palette.dim_style(),
        ));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_playlist_header(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled("── Playlist ── ", self.palette.dim_style())];

        if self.playlist.shuffled() {
            spans.push(Span::styled(
                "[Shuffle]",
                self.palette.active_toggle_style(),
            ));
        } else {
            spans.push(Span::styled("[Shuffle]", self.palette.dim_style()));
        }

        spans.push(Span::raw(" "));

        let repeat_str = format!("[Repeat: {}]", self.playlist.repeat());
        if self.playlist.repeat() != crate::playlist::RepeatMode::Off {
            spans.push(Span::styled(repeat_str, self.palette.active_toggle_style()));
        } else {
            spans.push(Span::styled(repeat_str, self.palette.dim_style()));
        }

        let q_len = self.playlist.queue_len();
        if q_len > 0 {
            spans.push(Span::styled(
                format!(" [Queue: {q_len}]"),
                self.palette.active_toggle_style(),
            ));
        }

        // Show theme name if not default
        let name = self.theme_name();
        if name != theme::DEFAULT_NAME {
            spans.push(Span::styled(
                format!(" [Theme: {name}]"),
                self.palette.active_toggle_style(),
            ));
        }

        spans.push(Span::styled(" ──", self.palette.dim_style()));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_playlist(&self, frame: &mut Frame, area: Rect) {
        let tracks = self.playlist.tracks();
        if tracks.is_empty() {
            let text = if self.player.is_music_app() {
                "  Music.app backend active"
            } else {
                "  No tracks loaded"
            };
            let line = Line::from(Span::styled(text, self.palette.dim_style()));
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
            let mut style = self.palette.playlist_item_style();

            if i == current_idx && self.player.is_playing() {
                prefix = "▶ ";
                style = self.palette.playlist_active_style();
            }

            if self.focus == Focus::Playlist && i == self.pl_cursor {
                style = self.palette.playlist_selected_style();
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
                spans.push(Span::styled(
                    format!(" [Q{qp}]"),
                    self.palette.active_toggle_style(),
                ));
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
            let line = Line::from(Span::styled(text, self.palette.dim_style()));
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
            let mut style = self.palette.playlist_item_style();

            if i == current_idx && self.player.is_playing() {
                prefix = "▶ ";
                style = self.palette.playlist_active_style();
            }

            if j == self.search_cursor {
                style = self.palette.playlist_selected_style();
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

    fn render_provider_header(&self, frame: &mut Frame, area: Rect) {
        let line = Line::from(Span::styled(
            self.provider_header_text(),
            self.palette.dim_style(),
        ));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_provider_list(&self, frame: &mut Frame, area: Rect) {
        if self.prov_loading {
            let name = self.provider_name();
            let target = if self.apple_music_showing_tracks() {
                "tracks"
            } else {
                "playlists"
            };
            let line = Line::from(Span::styled(
                format!("  Loading {name} {target}..."),
                self.palette.dim_style(),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        if self.apple_music_showing_tracks() {
            if self.apple_music_tracks.is_empty() {
                let line = Line::from(Span::styled("  No tracks found", self.palette.dim_style()));
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
                    self.palette.playlist_selected_style()
                } else {
                    self.palette.playlist_item_style()
                };
                let prefix = if i == self.prov_cursor { "> " } else { "  " };
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
                self.palette.dim_style(),
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
                self.palette.playlist_selected_style()
            } else {
                self.palette.playlist_item_style()
            };
            let prefix = if i == self.prov_cursor { "> " } else { "  " };
            lines.push(Line::from(Span::styled(
                format!("{prefix}{} ({} tracks)", pl.name, pl.track_count),
                style,
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_browser_header(&self, frame: &mut Frame, area: Rect) {
        let text = self
            .explorer
            .as_ref()
            .map(|explorer| explorer.header_text())
            .unwrap_or_else(|| {
                "── Browse Playlists ── Press [L] to open local playlist browser".to_string()
            });
        let line = Line::from(Span::styled(text, self.palette.dim_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_browser(&self, frame: &mut Frame, area: Rect) {
        let Some(explorer) = self.explorer.as_ref() else {
            let line = Line::from(Span::styled(
                "  Press [L] to browse .m3u/.m3u8 playlists",
                self.palette.dim_style(),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        };

        frame.render_widget_ref(explorer.widget(), area);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let text = if self.command_mode {
            format!(": {}  [Enter]Run [Esc]Cancel", self.command_input)
        } else if self.focus == Focus::Browser {
            "[↑↓/jk]Select [←/Backspace/h]Parent [→/Enter/l]Open/Load [Esc]Player [Ctrl+H]Hidden [L]Close [Tab]Focus [Q]Quit".to_string()
        } else if self.focus == Focus::Provider {
            if self.apple_music_showing_tracks() {
                "[↑↓]Navigate [Enter]Select Track [Esc]Playlists [:]Cmd [Tab]Focus [Q]Quit"
                    .to_string()
            } else if self.browsing_apple_music() {
                "[↑↓]Navigate [Enter]Open Playlist [:]Cmd [Tab]Focus [Q]Quit".to_string()
            } else {
                "[↑↓]Navigate [Enter]Load Playlist [L]Browse [:]Cmd [Tab]Focus [Q]Quit".to_string()
            }
        } else if self.searching {
            let count = self.search_results.len();
            format!(
                "/ {}  ({count} found)  [↑↓]Navigate [Enter]Play [Esc]Cancel",
                self.search_query
            )
        } else if self.player.is_music_app() {
            "[Spc]⏯ [<>]Track [s]Stop [+-]Vol [t]Theme [v]Vis [8]808 [:]Cmd [Q]Quit".to_string()
        } else if self.player.supports_seek() {
            "[Spc]⏯ [<>]Trk [←→]Seek [S]Save [+-]Vol [m]Mono [e]EQ [t]Theme [v]Vis [c]Art [L]Load [:]Cmd [8]808 [a]Queue [/]Search [Tab]Focus [Q]Quit".to_string()
        } else {
            "[Spc]⏯ [<>]Trk [S]Save [+-]Vol [m]Mono [e]EQ [t]Theme [v]Vis [c]Art [L]Load [:]Cmd [8]808 [a]Queue [/]Search [Tab]Focus [Q]Quit".to_string()
        };

        let line = Line::from(Span::styled(text, self.palette.help_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_status_line(&self, frame: &mut Frame, area: Rect) {
        if self.command_mode {
            let line = Line::from(Span::styled(
                format!(":{}_", self.command_input),
                self.palette.help_style(),
            ));
            frame.render_widget(Paragraph::new(line), area);
        } else if let Some(ref err) = self.err {
            let line = Line::from(Span::styled(
                format!("ERR: {err}"),
                self.palette.error_style(),
            ));
            frame.render_widget(Paragraph::new(line), area);
        } else if !self.save_msg.is_empty() {
            let line = Line::from(Span::styled(&self.save_msg, self.palette.status_style()));
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
            ("t", "Choose theme"),
            ("v", "Cycle visualizer"),
            ("c", "Toggle album art"),
            ("L", "Browse playlists"),
            ("↑ ↓", "Playlist scroll / EQ adjust"),
            ("h l", "EQ cursor left/right"),
            ("Enter", "Play selected track"),
            ("a", "Toggle queue (play next)"),
            ("S", "Save track to ~/Music"),
            ("r", "Cycle repeat"),
            ("z", "Toggle shuffle"),
            ("8", "Toggle 808 mode"),
            (":", "Command mode"),
            ("/", "Search playlist"),
            ("Tab", "Toggle focus"),
            ("Ctrl+K", "This keymap"),
            ("q", "Quit"),
        ];

        let mut lines = vec![
            Line::from(Span::styled("K E Y M A P", self.palette.title_style())),
            Line::from(""),
        ];

        for (key, action) in &keys {
            lines.push(Line::from(Span::styled(
                format!("  {key:<10} {action}"),
                self.palette.dim_style(),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press any key to close",
            self.palette.help_style(),
        )));

        let block = Block::default().borders(Borders::NONE);
        let paragraph = Paragraph::new(lines).block(block);

        // Center
        let w = 50u16.min(area.width);
        let h = 26u16.min(area.height);
        let x = area.width.saturating_sub(w) / 2;
        let y = area.height.saturating_sub(h) / 2;
        let inner = Rect::new(x, y, w, h);

        frame.render_widget(paragraph, inner);
    }

    fn render_theme_picker(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![
            Line::from(Span::styled("T H E M E S", self.palette.title_style())),
            Line::from(""),
        ];

        // Theme list: Default at index 0, then all loaded themes
        let count = self.themes.len() + 1;
        let max_visible = 15;
        let scroll = if self.theme_cursor >= max_visible {
            self.theme_cursor - max_visible + 1
        } else {
            0
        };

        for i in scroll..count.min(scroll + max_visible) {
            let name = if i == 0 {
                if self.mode_808 {
                    "Classic 808 (default)".to_string()
                } else {
                    theme::DEFAULT_NAME.to_string()
                }
            } else {
                self.themes[i - 1].name.clone()
            };

            if i == self.theme_cursor {
                lines.push(Line::from(Span::styled(
                    format!("> {name}"),
                    self.palette.playlist_selected_style(),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("  {name}"),
                    self.palette.dim_style(),
                )));
            }
        }

        if count > max_visible {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}/{count} themes", self.theme_cursor + 1),
                self.palette.dim_style(),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[↑↓]Navigate [Enter]Select [Esc]Cancel",
            self.palette.help_style(),
        )));

        // Center
        let w = 50u16.min(area.width);
        let h = (lines.len() as u16 + 2).min(area.height);
        let x = area.width.saturating_sub(w) / 2;
        let y = area.height.saturating_sub(h) / 2;
        let inner = Rect::new(x, y, w, h);

        let block = Block::default().borders(Borders::NONE);
        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, inner);
    }
}
