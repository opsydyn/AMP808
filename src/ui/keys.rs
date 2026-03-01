use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;
use super::eq_presets::EQ_PRESETS;
use super::styles::Palette;

impl App {
    /// Process a key event and return whether the app should quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Dismiss keymap overlay on any key
        if self.show_keymap {
            self.show_keymap = false;
            return false;
        }

        // Theme picker mode
        if self.show_themes {
            self.handle_theme_key(key);
            return false;
        }

        // Search mode
        if self.searching {
            self.handle_search_key(key);
            return false;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Char('q')) => {
                self.player.stop();
                return true;
            }

            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                self.show_keymap = true;
            }

            (_, KeyCode::Char(' ')) => {
                self.toggle_play_pause();
            }

            (_, KeyCode::Char('s')) => {
                self.player.stop();
            }

            (_, KeyCode::Char('>')) | (_, KeyCode::Char('.')) => {
                self.next_track();
            }

            (_, KeyCode::Char('<')) | (_, KeyCode::Char(',')) => {
                self.prev_track();
            }

            (_, KeyCode::Left) => {
                if self.focus == Focus::EQ {
                    if self.eq_cursor > 0 {
                        self.eq_cursor -= 1;
                    }
                } else {
                    self.seek_relative(-5.0);
                }
            }

            (_, KeyCode::Right) => {
                if self.focus == Focus::EQ {
                    if self.eq_cursor < 9 {
                        self.eq_cursor += 1;
                    }
                } else {
                    self.seek_relative(5.0);
                }
            }

            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if self.focus == Focus::EQ {
                    let bands = self.player.eq_bands();
                    self.player
                        .set_eq_band(self.eq_cursor, bands[self.eq_cursor] + 1.0);
                    self.eq_preset_idx = None; // custom
                } else if self.pl_cursor > 0 {
                    self.pl_cursor -= 1;
                    self.adjust_scroll();
                }
            }

            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if self.focus == Focus::EQ {
                    let bands = self.player.eq_bands();
                    self.player
                        .set_eq_band(self.eq_cursor, bands[self.eq_cursor] - 1.0);
                    self.eq_preset_idx = None; // custom
                } else if self.pl_cursor < self.playlist.len().saturating_sub(1) {
                    self.pl_cursor += 1;
                    self.adjust_scroll();
                }
            }

            (_, KeyCode::Enter) => {
                if self.focus == Focus::Playlist {
                    self.playlist.set_index(self.pl_cursor);
                    self.play_current_track();
                }
            }

            (_, KeyCode::Char('+')) | (_, KeyCode::Char('=')) => {
                self.player.set_volume(self.player.volume() + 1.0);
            }

            (_, KeyCode::Char('-')) => {
                self.player.set_volume(self.player.volume() - 1.0);
            }

            (_, KeyCode::Char('r')) => {
                self.playlist.cycle_repeat();
                self.player.clear_preload();
                self.preload_next();
            }

            (_, KeyCode::Char('z')) => {
                self.playlist.toggle_shuffle();
                self.player.clear_preload();
                self.preload_next();
            }

            (_, KeyCode::Tab) => {
                self.focus = match self.focus {
                    Focus::Playlist => Focus::EQ,
                    Focus::EQ => Focus::Playlist,
                };
            }

            (_, KeyCode::Char('h')) => {
                if self.focus == Focus::EQ && self.eq_cursor > 0 {
                    self.eq_cursor -= 1;
                }
            }

            (_, KeyCode::Char('l')) => {
                if self.focus == Focus::EQ && self.eq_cursor < 9 {
                    self.eq_cursor += 1;
                }
            }

            (_, KeyCode::Char('e')) => {
                let idx = self.eq_preset_idx.map(|i| i + 1).unwrap_or(0);
                let idx = if idx >= EQ_PRESETS.len() { 0 } else { idx };
                self.eq_preset_idx = Some(idx);
                self.apply_eq_preset();
            }

            (_, KeyCode::Char('t')) => {
                // Set cursor to current theme position
                self.theme_cursor = match self.theme_idx {
                    Some(i) => i + 1, // +1 because Default is at 0
                    None => 0,
                };
                self.show_themes = true;
            }

            (_, KeyCode::Char('a')) => {
                if self.focus == Focus::Playlist && !self.playlist.dequeue(self.pl_cursor) {
                    self.playlist.queue(self.pl_cursor);
                }
            }

            (_, KeyCode::Char('S')) => {
                self.save_track();
            }

            (_, KeyCode::Char('m')) => {
                self.player.toggle_mono();
            }

            (_, KeyCode::Char('/')) => {
                self.searching = true;
                self.search_query.clear();
                self.search_results.clear();
                self.search_cursor = 0;
            }

            (_, KeyCode::Char('v')) => {
                self.vis.cycle_mode();
            }

            (_, KeyCode::Char('8')) => {
                self.mode_808 = !self.mode_808;
                if self.mode_808 {
                    self.palette = Palette::tr808();
                } else {
                    self.apply_theme(self.theme_idx);
                }
            }

            _ => {}
        }

        false
    }

    fn handle_theme_key(&mut self, key: KeyEvent) {
        let count = self.themes.len() + 1; // +1 for Default
        match key.code {
            KeyCode::Esc => {
                self.show_themes = false;
            }

            KeyCode::Up => {
                if self.theme_cursor > 0 {
                    self.theme_cursor -= 1;
                    // Live preview
                    self.apply_theme_from_cursor();
                }
            }

            KeyCode::Down => {
                if self.theme_cursor < count - 1 {
                    self.theme_cursor += 1;
                    // Live preview
                    self.apply_theme_from_cursor();
                }
            }

            KeyCode::Enter => {
                self.apply_theme_from_cursor();
                self.show_themes = false;
            }

            _ => {}
        }
    }

    fn apply_theme_from_cursor(&mut self) {
        if self.theme_cursor == 0 {
            self.apply_theme(None);
        } else {
            self.apply_theme(Some(self.theme_cursor - 1));
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.searching = false;
            }

            KeyCode::Enter => {
                if !self.search_results.is_empty() {
                    let idx = self.search_results[self.search_cursor];
                    self.playlist.set_index(idx);
                    self.pl_cursor = idx;
                    self.adjust_scroll();
                    self.play_current_track();
                }
                self.searching = false;
                self.focus = Focus::Playlist;
            }

            KeyCode::Up => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                }
            }

            KeyCode::Down => {
                if self.search_cursor < self.search_results.len().saturating_sub(1) {
                    self.search_cursor += 1;
                }
            }

            KeyCode::Backspace => {
                self.search_query.pop();
                self.update_search();
            }

            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.update_search();
            }

            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Playlist,
    EQ,
}
