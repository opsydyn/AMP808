use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;
use super::command::{CommandInputResult, handle_command_input_key};
use super::eq_presets::EQ_PRESETS;

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

        // Command mode
        if self.command_mode {
            self.handle_command_key(key);
            return false;
        }

        // Search mode
        if self.searching {
            self.handle_search_key(key);
            return false;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Char('q')) => {
                if let Err(e) = self.player.stop() {
                    self.err = Some(e.to_string());
                }
                return true;
            }

            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                self.show_keymap = true;
            }

            (_, KeyCode::Char(' ')) => {
                self.toggle_play_pause();
            }

            (_, KeyCode::Char('s')) => {
                if let Err(e) = self.player.stop() {
                    self.err = Some(e.to_string());
                }
            }

            (_, KeyCode::Char('>')) | (_, KeyCode::Char('.')) => {
                self.next_track();
            }

            (_, KeyCode::Char('<')) | (_, KeyCode::Char(',')) => {
                self.prev_track();
            }

            (_, KeyCode::Left) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                } else if self.focus == Focus::EQ {
                    if self.eq_cursor > 0 {
                        self.eq_cursor -= 1;
                    }
                } else if self.player.supports_seek() {
                    self.seek_relative(-5.0);
                }
            }

            (_, KeyCode::Right) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                } else if self.focus == Focus::EQ {
                    if self.eq_cursor < 9 {
                        self.eq_cursor += 1;
                    }
                } else if self.player.supports_seek() {
                    self.seek_relative(5.0);
                }
            }

            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                } else if self.focus == Focus::Provider {
                    if self.prov_cursor > 0 {
                        self.prov_cursor -= 1;
                    }
                } else if self.focus == Focus::EQ && self.player.supports_eq() {
                    let bands = self.player.eq_bands();
                    if let Err(e) = self
                        .player
                        .set_eq_band(self.eq_cursor, bands[self.eq_cursor] + 1.0)
                    {
                        self.err = Some(e.to_string());
                    } else {
                        self.eq_preset_idx = None; // custom
                    }
                } else if self.pl_cursor > 0 {
                    self.pl_cursor -= 1;
                    self.adjust_scroll();
                }
            }

            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                } else if self.focus == Focus::Provider {
                    if self.prov_cursor < self.provider_item_count().saturating_sub(1) {
                        self.prov_cursor += 1;
                    }
                } else if self.focus == Focus::EQ && self.player.supports_eq() {
                    let bands = self.player.eq_bands();
                    if let Err(e) = self
                        .player
                        .set_eq_band(self.eq_cursor, bands[self.eq_cursor] - 1.0)
                    {
                        self.err = Some(e.to_string());
                    } else {
                        self.eq_preset_idx = None; // custom
                    }
                } else if self.pl_cursor < self.playlist.len().saturating_sub(1) {
                    self.pl_cursor += 1;
                    self.adjust_scroll();
                }
            }

            (_, KeyCode::Enter) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                } else if self.focus == Focus::Provider && self.provider_item_count() > 0 {
                    if self.apple_music_showing_tracks() {
                        self.save_msg = "Apple Music playback handoff not implemented yet".into();
                        self.save_msg_ttl = 60;
                    } else if self.browsing_apple_music() {
                        let pl = self.provider_lists[self.prov_cursor].clone();
                        self.fetch_apple_music_tracks(&pl.id, &pl.name);
                    } else {
                        let id = self.provider_lists[self.prov_cursor].id.clone();
                        self.fetch_provider_tracks(&id);
                    }
                } else if self.focus == Focus::Playlist {
                    self.playlist.set_index(self.pl_cursor);
                    self.play_current_track();
                }
            }

            (_, KeyCode::Esc) | (_, KeyCode::Backspace) => {
                if self.focus == Focus::Browser {
                    if key.code == KeyCode::Esc {
                        self.focus = Focus::Playlist;
                    } else {
                        self.handle_browser_key(key);
                    }
                } else if self.focus == Focus::Provider && self.apple_music_showing_tracks() {
                    self.apple_music_track_context = None;
                    self.apple_music_tracks.clear();
                    self.prov_cursor = 0;
                } else if self.focus == Focus::Playlist && self.has_provider_pane() {
                    self.focus = Focus::Provider;
                }
            }

            (_, KeyCode::Char('+')) | (_, KeyCode::Char('=')) => {
                if let Err(e) = self.player.set_volume(self.player.volume() + 1.0) {
                    self.err = Some(e.to_string());
                }
            }

            (_, KeyCode::Char('-')) => {
                if let Err(e) = self.player.set_volume(self.player.volume() - 1.0) {
                    self.err = Some(e.to_string());
                }
            }

            (_, KeyCode::Char('r')) => {
                self.playlist.cycle_repeat();
                let _ = self.player.clear_preload();
                self.preload_next();
            }

            (_, KeyCode::Char('z')) => {
                self.playlist.toggle_shuffle();
                let _ = self.player.clear_preload();
                self.preload_next();
            }

            (_, KeyCode::Tab) => {
                self.focus = next_focus(
                    self.focus,
                    self.has_provider_pane(),
                    self.explorer.is_some() && self.player.supports_local_playlist(),
                    self.player.supports_eq(),
                );
            }

            (KeyModifiers::CONTROL, KeyCode::Char('h')) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                }
            }

            (_, KeyCode::Char('h')) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                } else if self.focus == Focus::EQ && self.eq_cursor > 0 {
                    self.eq_cursor -= 1;
                }
            }

            (_, KeyCode::Char('l')) => {
                if self.focus == Focus::Browser {
                    self.handle_browser_key(key);
                } else if self.focus == Focus::EQ && self.eq_cursor < 9 {
                    self.eq_cursor += 1;
                }
            }

            (_, KeyCode::Char('e')) => {
                if !self.player.supports_eq() {
                    return false;
                }
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
                self.theme_idx_before_picker = Some(self.theme_idx);
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
                if let Err(e) = self.player.toggle_mono() {
                    self.err = Some(e.to_string());
                }
            }

            (_, KeyCode::Char('/')) => {
                if !self.player.supports_local_playlist() {
                    return false;
                }
                self.searching = true;
                self.search_query.clear();
                self.search_results.clear();
                self.search_cursor = 0;
            }

            (_, KeyCode::Char('L')) => {
                if self.player.supports_local_playlist() {
                    self.toggle_playlist_browser();
                }
            }

            (_, KeyCode::Char(':')) => {
                self.command_mode = true;
                self.command_input.clear();
            }

            (_, KeyCode::Char('v')) => {
                if self.can_cycle_visualizer() {
                    if self.player.is_music_app() {
                        self.vis.cycle_music_app_mode();
                    } else {
                        self.vis.cycle_mode();
                    }
                }
            }

            (_, KeyCode::Char('c')) => {
                if self.player.supports_cover_art() {
                    self.show_cover_art = !self.show_cover_art;
                }
            }

            (_, KeyCode::Char('8')) => {
                self.mode_808 = !self.mode_808;
                self.refresh_palette();
            }

            _ => {}
        }

        false
    }

    fn can_cycle_visualizer(&self) -> bool {
        self.player.supports_visualizer()
    }

    fn handle_theme_key(&mut self, key: KeyEvent) {
        let count = self.themes.len() + 1; // +1 for Default
        match key.code {
            KeyCode::Esc => {
                // Restore original theme on cancel
                if let Some(saved) = self.theme_idx_before_picker.take() {
                    self.apply_theme(saved);
                }
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
                self.theme_idx_before_picker = None; // confirm selection
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

    fn handle_command_key(&mut self, key: KeyEvent) {
        match handle_command_input_key(&mut self.command_input, key) {
            CommandInputResult::Continue => {}
            CommandInputResult::Cancel => {
                self.command_mode = false;
            }
            CommandInputResult::Submit(line) => {
                self.command_mode = false;
                if line.trim().is_empty() {
                    return;
                }
                self.execute_command_line(&line);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Playlist,
    Browser,
    EQ,
    Provider,
}

fn next_focus(current: Focus, has_provider: bool, has_browser: bool, has_eq: bool) -> Focus {
    match current {
        Focus::Playlist => {
            if has_browser {
                Focus::Browser
            } else if has_eq {
                Focus::EQ
            } else if has_provider {
                Focus::Provider
            } else {
                Focus::Playlist
            }
        }
        Focus::Browser => {
            if has_eq {
                Focus::EQ
            } else if has_provider {
                Focus::Provider
            } else {
                Focus::Playlist
            }
        }
        Focus::EQ => {
            if has_provider {
                Focus::Provider
            } else {
                Focus::Playlist
            }
        }
        Focus::Provider => Focus::Playlist,
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use std::{process::Command, thread::sleep, time::Duration};

    #[cfg(target_os = "macos")]
    use anyhow::{Context, Result};
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tokio::sync::mpsc;

    use super::{Focus, next_focus};
    use crate::external::apple_music_api::{AppleMusicClient, AppleMusicConfig, LibraryTrack};
    use crate::external::music_app::{MusicAppPlayerState, MusicAppSnapshot};
    use crate::ui::{styles::Palette, theme};
    use crate::{
        playback_backend::PlaybackBackend,
        playlist::{Playlist, PlaylistInfo},
        resolve::ytdl::YtdlTempTracker,
        ui::{App, AppleMusicTrackContext, visualizer::VisMode},
    };

    #[test]
    fn tab_cycle_includes_browser_when_available() {
        assert_eq!(
            next_focus(Focus::Playlist, false, true, true),
            Focus::Browser
        );
        assert_eq!(next_focus(Focus::Browser, false, true, true), Focus::EQ);
    }

    #[test]
    fn tab_cycle_skips_browser_when_unavailable() {
        assert_eq!(next_focus(Focus::Playlist, false, false, true), Focus::EQ);
    }

    #[test]
    fn tab_cycle_includes_provider_when_available() {
        assert_eq!(next_focus(Focus::EQ, true, true, true), Focus::Provider);
        assert_eq!(
            next_focus(Focus::Provider, true, true, true),
            Focus::Playlist
        );
    }

    #[test]
    fn tab_cycle_stays_on_playlist_without_browser_or_eq() {
        assert_eq!(
            next_focus(Focus::Playlist, false, false, false),
            Focus::Playlist
        );
    }

    fn build_test_music_app() -> App {
        let (tx, _rx) = mpsc::unbounded_channel();
        App::new(
            PlaybackBackend::music_app_for_test(MusicAppSnapshot {
                state: MusicAppPlayerState::Playing,
                volume: 50,
                title: "Alive".into(),
                artist: "Daft Punk".into(),
                album: "Homework".into(),
                position_secs: 12.0,
                duration_secs: 300.0,
            }),
            Playlist::new(),
            YtdlTempTracker::new(),
            tx,
            None,
        )
    }

    fn dummy_apple_music_client() -> AppleMusicClient {
        AppleMusicClient::from_tokens(AppleMusicConfig {
            developer_token: "developer-token".into(),
            user_token: "user-token".into(),
            storefront: Some("gb".into()),
            base_url: "https://example.test/v1".into(),
        })
        .unwrap()
    }

    #[test]
    fn escape_from_apple_music_track_view_returns_to_playlist_list() {
        let mut app = build_test_music_app();
        app.apple_music_client = Some(Arc::new(dummy_apple_music_client()));
        app.provider_lists = vec![PlaylistInfo {
            id: "playlist-1".into(),
            name: "Favorites".into(),
            track_count: 2,
        }];
        app.apple_music_tracks = vec![LibraryTrack {
            id: "track-1".into(),
            title: "Alive".into(),
            artist: "Daft Punk".into(),
            album: "Homework".into(),
        }];
        app.apple_music_track_context = Some(AppleMusicTrackContext {
            playlist_id: "playlist-1".into(),
            playlist_name: "Favorites".into(),
        });
        app.focus = Focus::Provider;
        app.prov_cursor = 1;

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.apple_music_track_context.is_none());
        assert!(app.apple_music_tracks.is_empty());
        assert_eq!(app.prov_cursor, 0);
    }

    #[test]
    fn enter_on_apple_music_track_shows_handoff_message() {
        let mut app = build_test_music_app();
        app.apple_music_client = Some(Arc::new(dummy_apple_music_client()));
        app.apple_music_tracks = vec![LibraryTrack {
            id: "track-1".into(),
            title: "Alive".into(),
            artist: "Daft Punk".into(),
            album: "Homework".into(),
        }];
        app.apple_music_track_context = Some(AppleMusicTrackContext {
            playlist_id: "playlist-1".into(),
            playlist_name: "Favorites".into(),
        });
        app.focus = Focus::Provider;

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            app.save_msg,
            "Apple Music playback handoff not implemented yet"
        );
        assert_eq!(app.save_msg_ttl, 60);
    }

    #[test]
    fn v_cycles_visualizer_in_standard_music_app_mode() {
        let mut app = build_test_music_app();
        assert_eq!(app.vis.mode, VisMode::Bars);

        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

        assert_eq!(app.vis.mode, VisMode::BarsGap);
    }

    #[test]
    fn v_cycles_visualizer_in_808_music_app_mode() {
        let mut app = build_test_music_app();
        app.mode_808 = true;
        assert_eq!(app.vis.mode, VisMode::Bars);

        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

        assert_eq!(app.vis.mode, VisMode::BarsGap);
    }

    #[test]
    fn toggle_808_keeps_selected_theme_palette() {
        let mut app = build_test_music_app();
        let idx = theme::find_by_name(&app.themes, "catppuccin").expect("theme exists");
        let themed = Palette::from_theme(&app.themes[idx]);

        app.apply_theme(Some(idx));
        app.handle_key(KeyEvent::new(KeyCode::Char('8'), KeyModifiers::NONE));

        assert!(app.mode_808);
        assert_eq!(app.palette.title, themed.title);
        assert_eq!(app.palette.text, themed.text);
        assert_eq!(app.palette.spectrum_high, themed.spectrum_high);
    }

    #[cfg(target_os = "macos")]
    fn run_osascript(script: &str) -> Result<String> {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .context("failed to execute osascript")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("osascript failed: {}", stderr.trim());
        }

        String::from_utf8(output.stdout)
            .map(|s| s.trim().to_string())
            .context("osascript output was not valid UTF-8")
    }

    #[cfg(target_os = "macos")]
    fn seed_music_app_transport_test() -> Result<u8> {
        let volume = run_osascript(
            r#"tell application "Music"
  if (count of tracks of library playlist 1) < 3 then error "need at least 3 library tracks"
  set sound volume to 41
  play track 2 of library playlist 1
  delay 0.5
  return sound volume as text
end tell"#,
        )?;

        volume
            .parse::<u8>()
            .with_context(|| format!("invalid Music.app volume: {volume}"))
    }

    #[cfg(target_os = "macos")]
    fn current_track_id() -> Result<String> {
        run_osascript(
            r#"tell application "Music"
  return (persistent ID of current track) as text
end tell"#,
        )
    }

    #[cfg(target_os = "macos")]
    fn current_player_state() -> Result<String> {
        run_osascript(
            r#"tell application "Music"
  return (player state as string)
end tell"#,
        )
    }

    #[cfg(target_os = "macos")]
    fn current_volume() -> Result<u8> {
        let raw = run_osascript(
            r#"tell application "Music"
  return sound volume as text
end tell"#,
        )?;

        raw.parse::<u8>()
            .with_context(|| format!("invalid Music.app volume: {raw}"))
    }

    #[cfg(target_os = "macos")]
    fn current_position_secs() -> Result<f64> {
        let raw = run_osascript(
            r#"tell application "Music"
  return player position as text
end tell"#,
        )?;

        raw.parse::<f64>()
            .with_context(|| format!("invalid Music.app position: {raw}"))
    }

    #[cfg(target_os = "macos")]
    fn set_player_position_secs(seconds: f64) -> Result<()> {
        run_osascript(&format!(
            "tell application \"Music\" to set player position to {}",
            seconds.max(0.0)
        ))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn restore_volume(volume: u8) -> Result<()> {
        run_osascript(&format!(
            "tell application \"Music\" to set sound volume to {}",
            volume.min(100)
        ))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn build_music_app_app() -> Result<App> {
        let (tx, _rx) = mpsc::unbounded_channel();
        Ok(App::new(
            PlaybackBackend::music_app()?,
            Playlist::new(),
            YtdlTempTracker::new(),
            tx,
            None,
        ))
    }

    #[cfg(target_os = "macos")]
    fn press_and_sync(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        sleep(Duration::from_millis(400));
        app.sync_remote_backend();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires macOS Music.app automation access"]
    fn live_music_app_transport_keys_control_music_app() {
        let seeded_volume = seed_music_app_transport_test().unwrap();
        let mut app = build_music_app_app().unwrap();

        let result = (|| -> Result<()> {
            app.sync_remote_backend();
            assert_eq!(current_player_state()?, "playing");

            press_and_sync(&mut app, KeyCode::Char(' '));
            assert_eq!(current_player_state()?, "paused");
            assert!(app.player.is_paused());

            press_and_sync(&mut app, KeyCode::Char(' '));
            assert_eq!(current_player_state()?, "playing");
            assert!(!app.player.is_paused());

            let before_next = current_track_id()?;
            press_and_sync(&mut app, KeyCode::Char('.'));
            let after_next = current_track_id()?;
            assert_ne!(after_next, before_next);

            let before_prev = after_next.clone();
            set_player_position_secs(20.0)?;
            sleep(Duration::from_millis(200));
            let before_prev_pos = current_position_secs()?;
            press_and_sync(&mut app, KeyCode::Char(','));
            let after_prev = current_track_id()?;
            let after_prev_pos = current_position_secs()?;
            assert!(
                after_prev != before_prev || after_prev_pos + 2.0 < before_prev_pos,
                "expected previous to change track or rewind position: id {before_prev} -> {after_prev}, pos {before_prev_pos} -> {after_prev_pos}"
            );

            let before_up = current_volume()?;
            press_and_sync(&mut app, KeyCode::Char('+'));
            let after_up = current_volume()?;
            assert!(
                after_up > before_up,
                "expected volume up: {before_up} -> {after_up}"
            );

            let before_down = current_volume()?;
            press_and_sync(&mut app, KeyCode::Char('-'));
            let after_down = current_volume()?;
            assert!(
                after_down < before_down,
                "expected volume down: {before_down} -> {after_down}"
            );

            Ok(())
        })();

        let restore_result = restore_volume(seeded_volume);
        if let Err(err) = restore_result {
            panic!("failed to restore Music.app volume after test: {err}");
        }
        result.unwrap();
    }
}
