pub mod eq_presets;
pub mod keys;
pub mod styles;
pub mod theme;
pub mod view;
pub mod view_808;
pub mod visualizer;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use self::eq_presets::EQ_PRESETS;
use self::keys::Focus;
use self::styles::Palette;
use self::theme::Theme;
use self::visualizer::Visualizer;
use crate::player::Player;
use crate::playlist::{self, Playlist, PlaylistInfo, Provider, Track};
use crate::resolve;
use crate::resolve::ytdl::YtdlTempTracker;

/// Messages sent from async tasks to the main event loop.
pub enum AppMsg {
    YtdlResolved { index: usize, track: Track },
    YtdlError { error: anyhow::Error },
    FeedsLoaded(Vec<Track>),
    FeedError(anyhow::Error),
    StreamPlayed { error: Option<String> },
    ProviderPlaylists(Vec<PlaylistInfo>),
    ProviderTracks(Vec<Track>),
    ProviderError(String),
}

/// The main application state.
pub struct App {
    pub player: Player,
    pub playlist: Playlist,
    pub vis: Visualizer,
    pub focus: Focus,
    pub eq_cursor: usize,
    pub eq_preset_idx: Option<usize>,
    pub pl_cursor: usize,
    pub pl_scroll: usize,
    pub pl_visible: usize,
    pub title_off: usize,
    pub err: Option<String>,
    pub buffering: bool,
    pub save_msg: String,
    pub save_msg_ttl: usize,
    pub show_keymap: bool,
    pub searching: bool,
    pub search_query: String,
    pub search_results: Vec<usize>,
    pub search_cursor: usize,
    pub themes: Vec<Theme>,
    pub theme_idx: Option<usize>,
    pub theme_cursor: usize,
    pub show_themes: bool,
    pub theme_idx_before_picker: Option<Option<usize>>,
    pub mode_808: bool,
    pub palette: Palette,
    pub stream_title: String,
    pub provider: Option<std::sync::Arc<dyn Provider>>,
    pub provider_lists: Vec<PlaylistInfo>,
    pub prov_cursor: usize,
    pub prov_loading: bool,
    tracker: YtdlTempTracker,
    msg_tx: mpsc::UnboundedSender<AppMsg>,
}

impl App {
    pub fn new(
        player: Player,
        playlist: Playlist,
        tracker: YtdlTempTracker,
        msg_tx: mpsc::UnboundedSender<AppMsg>,
        provider: Option<Box<dyn Provider>>,
    ) -> Self {
        let sr = player.sample_rate() as f64;
        let themes = theme::load_all();
        let has_provider = provider.is_some();
        let provider: Option<std::sync::Arc<dyn Provider>> = provider.map(std::sync::Arc::from);
        let initial_focus = if has_provider && playlist.is_empty() {
            Focus::Provider
        } else {
            Focus::Playlist
        };
        let app = Self {
            player,
            playlist,
            vis: Visualizer::new(sr),
            focus: initial_focus,
            eq_cursor: 0,
            eq_preset_idx: None,
            pl_cursor: 0,
            pl_scroll: 0,
            pl_visible: 5,
            title_off: 0,
            err: None,
            buffering: false,
            save_msg: String::new(),
            save_msg_ttl: 0,
            show_keymap: false,
            searching: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_cursor: 0,
            themes,
            theme_idx: None,
            theme_cursor: 0,
            show_themes: false,
            theme_idx_before_picker: None,
            mode_808: false,
            palette: Palette::default(),
            stream_title: String::new(),
            provider,
            provider_lists: Vec::new(),
            prov_cursor: 0,
            prov_loading: has_provider,
            tracker,
            msg_tx,
        };

        // Fetch playlists in background if provider exists
        if has_provider {
            app.fetch_provider_playlists();
        }

        app
    }

    /// Apply a theme by index (None = default ANSI theme).
    pub fn apply_theme(&mut self, idx: Option<usize>) {
        self.theme_idx = idx;
        self.palette = match idx {
            Some(i) if i < self.themes.len() => Palette::from_theme(&self.themes[i]),
            _ => Palette::default(),
        };
    }

    /// Get the current theme name.
    pub fn theme_name(&self) -> &str {
        match self.theme_idx {
            Some(i) if i < self.themes.len() => &self.themes[i].name,
            _ => theme::DEFAULT_NAME,
        }
    }

    /// Handle a tick: check gapless transitions, decay messages.
    pub fn on_tick(&mut self) {
        // Expire save message
        if self.save_msg_ttl > 0 {
            self.save_msg_ttl -= 1;
            if self.save_msg_ttl == 0 {
                self.save_msg.clear();
            }
        }

        // Check gapless transition
        if self.player.gapless_advanced() {
            self.playlist.next();
            self.pl_cursor = self.playlist.index().unwrap_or(0);
            self.adjust_scroll();
            self.title_off = 0;
            self.preload_next();
        }

        // Poll ICY stream title
        let title = self.player.stream_title();
        if !title.is_empty() {
            self.stream_title = title;
        }

        // Check if drained (end of current, no next)
        if self.player.is_playing()
            && !self.player.is_paused()
            && self.player.drained()
            && !self.buffering
        {
            self.next_track();
        }

        self.title_off += 1;
    }

    /// Handle an async message from a spawned task.
    pub fn on_msg(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::YtdlResolved { index, track } => {
                self.buffering = false;
                self.playlist.set_track(index, track.clone());
                self.play_track_owned(track);
            }
            AppMsg::YtdlError { error } => {
                self.buffering = false;
                self.err = Some(error.to_string());
            }
            AppMsg::FeedsLoaded(tracks) => {
                self.playlist.add(&tracks);
                if !self.playlist.is_empty() && !self.player.is_playing() {
                    self.play_current_track();
                }
            }
            AppMsg::FeedError(error) => {
                self.err = Some(error.to_string());
            }
            AppMsg::StreamPlayed { error } => {
                self.buffering = false;
                if let Some(e) = error {
                    self.err = Some(e);
                } else {
                    self.err = None;
                    self.preload_next();
                }
            }
            AppMsg::ProviderPlaylists(lists) => {
                self.prov_loading = false;
                self.provider_lists = lists;
            }
            AppMsg::ProviderTracks(tracks) => {
                self.prov_loading = false;
                self.playlist.add(&tracks);
                self.focus = Focus::Playlist;
                if !self.player.is_playing() {
                    self.play_current_track();
                }
            }
            AppMsg::ProviderError(e) => {
                self.prov_loading = false;
                self.err = Some(e);
            }
        }
    }

    // --- Playback control ---

    pub fn toggle_play_pause(&mut self) {
        if !self.player.is_playing() {
            self.play_current_track();
        } else {
            self.player.toggle_pause();
        }
    }

    pub fn next_track(&mut self) {
        // next() returns Option<(&Track, bool)> — clone the track to avoid borrow
        let track = self.playlist.next().map(|(t, _)| t.clone());
        if let Some(track) = track {
            self.pl_cursor = self.playlist.index().unwrap_or(0);
            self.adjust_scroll();
            self.play_track_owned(track);
        } else {
            self.player.stop();
        }
    }

    pub fn prev_track(&mut self) {
        // If >3s into track, restart instead of going back
        let (pos, _) = self.track_position();
        if pos > 3 {
            self.seek_to(0.0);
            return;
        }

        let track = self.playlist.prev().cloned();
        if let Some(track) = track {
            self.pl_cursor = self.playlist.index().unwrap_or(0);
            self.adjust_scroll();
            self.play_track_owned(track);
        }
    }

    pub fn play_current_track(&mut self) {
        let track = self.playlist.current().map(|(t, _)| t.clone());
        if let Some(track) = track {
            self.title_off = 0;
            self.play_track_owned(track);
        }
    }

    /// Play a track (takes ownership to avoid borrow conflicts).
    pub fn play_track_owned(&mut self, track: Track) {
        self.stream_title.clear();

        // Lazy-resolve yt-dlp URLs
        if playlist::is_ytdl(&track.path) {
            self.buffering = true;
            self.err = None;
            let idx = self.playlist.index().unwrap_or(0);
            let url = track.path.clone();
            let tx = self.msg_tx.clone();
            let tracker = self.tracker.clone();
            tokio::spawn(async move {
                match resolve::ytdl::resolve_ytdl_track(&url, &tracker).await {
                    Ok(resolved) => {
                        let _ = tx.send(AppMsg::YtdlResolved {
                            index: idx,
                            track: resolved,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppMsg::YtdlError { error: e.into() });
                    }
                }
            });
            return;
        }

        // HTTP stream tracks — play asynchronously (HTTP connect + Symphonia probe blocks)
        if track.stream && playlist::is_url(&track.path) {
            self.buffering = true;
            self.err = None;
            self.title_off = 0;
            self.player
                .play_async(track.path.clone(), self.msg_tx.clone());
            return;
        }

        if let Err(e) = self.player.play(&track.path) {
            self.err = Some(e.to_string());
        } else {
            self.err = None;
            self.preload_next();
        }
    }

    pub fn preload_next(&mut self) {
        let path = self.playlist.peek_next().map(|t| t.path.clone());
        if let Some(path) = path {
            if playlist::is_ytdl(&path) {
                return;
            }
            let _ = self.player.preload(&path);
        }
    }

    pub fn seek_relative(&mut self, seconds: f64) {
        if let Err(e) = self.player.seek_relative(seconds) {
            self.err = Some(e.to_string());
        } else {
            // Seek invalidates preload (handled in gapless), re-preload
            self.preload_next();
        }
    }

    pub fn seek_to(&mut self, seconds: f64) {
        if let Err(e) = self.player.seek_to(seconds) {
            self.err = Some(e.to_string());
        } else {
            self.preload_next();
        }
    }

    /// Get (position_seconds, duration_seconds) of the current track.
    pub fn track_position(&self) -> (u64, u64) {
        self.player.track_position()
    }

    // --- EQ ---

    pub fn eq_preset_name(&self) -> String {
        match self.eq_preset_idx {
            Some(idx) if idx < EQ_PRESETS.len() => EQ_PRESETS[idx].name.to_string(),
            _ => "Custom".to_string(),
        }
    }

    pub fn apply_eq_preset(&mut self) {
        if let Some(idx) = self.eq_preset_idx
            && idx < EQ_PRESETS.len()
        {
            let bands = EQ_PRESETS[idx].bands;
            for (i, &gain) in bands.iter().enumerate() {
                self.player.set_eq_band(i, gain);
            }
        }
    }

    // --- Search ---

    pub fn update_search(&mut self) {
        self.search_results.clear();
        self.search_cursor = 0;
        if self.search_query.is_empty() {
            return;
        }
        let query = self.search_query.to_lowercase();
        for (i, track) in self.playlist.tracks().iter().enumerate() {
            if track.display_name().to_lowercase().contains(&query) {
                self.search_results.push(i);
            }
        }
    }

    // --- Scroll ---

    pub fn adjust_scroll(&mut self) {
        if self.pl_cursor < self.pl_scroll {
            self.pl_scroll = self.pl_cursor;
        }
        if self.pl_cursor >= self.pl_scroll + self.pl_visible {
            self.pl_scroll = self.pl_cursor - self.pl_visible + 1;
        }
    }

    // --- Save ---

    // --- Provider ---

    pub fn fetch_provider_playlists(&self) {
        if let Some(ref prov) = self.provider {
            let prov = std::sync::Arc::clone(prov);
            let tx = self.msg_tx.clone();
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || prov.playlists()).await;
                match result {
                    Ok(Ok(lists)) => {
                        let _ = tx.send(AppMsg::ProviderPlaylists(lists));
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(AppMsg::ProviderError(e.to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(AppMsg::ProviderError(e.to_string()));
                    }
                }
            });
        }
    }

    pub fn fetch_provider_tracks(&mut self, playlist_id: &str) {
        if let Some(ref prov) = self.provider {
            self.prov_loading = true;
            let prov = std::sync::Arc::clone(prov);
            let id = playlist_id.to_string();
            let tx = self.msg_tx.clone();
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || prov.tracks(&id)).await;
                match result {
                    Ok(Ok(tracks)) => {
                        let _ = tx.send(AppMsg::ProviderTracks(tracks));
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(AppMsg::ProviderError(e.to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(AppMsg::ProviderError(e.to_string()));
                    }
                }
            });
        }
    }

    pub fn provider_name(&self) -> &str {
        self.provider.as_ref().map_or("Provider", |p| p.name())
    }

    // --- Save ---

    pub fn save_track(&mut self) {
        let track = match self.playlist.current() {
            Some((t, _)) => t.clone(),
            None => {
                self.save_msg = "Nothing to save".to_string();
                self.save_msg_ttl = 40;
                return;
            }
        };

        // Only save temp files (yt-dlp downloads)
        let tmp = std::env::temp_dir();
        if track.stream || !track.path.starts_with(tmp.to_str().unwrap_or("/tmp")) {
            self.save_msg = "Only downloaded tracks can be saved".to_string();
            self.save_msg_ttl = 40;
            return;
        }

        let home = match dirs::home_dir() {
            Some(h) => h,
            None => {
                self.save_msg = "Save failed: no home directory".to_string();
                self.save_msg_ttl = 40;
                return;
            }
        };

        let save_dir = home.join("Music").join("cliamp");
        if let Err(e) = std::fs::create_dir_all(&save_dir) {
            self.save_msg = format!("Save failed: {e}");
            self.save_msg_ttl = 40;
            return;
        }

        let ext = std::path::Path::new(&track.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let mut name = track.title.clone();
        if !track.artist.is_empty() {
            name = format!("{} - {}", track.artist, name);
        }
        // Sanitize filename
        let name: String = name
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect();

        let filename = if ext.is_empty() {
            name.clone()
        } else {
            format!("{name}.{ext}")
        };
        let dest = save_dir.join(&filename);

        match std::fs::copy(&track.path, &dest) {
            Ok(_) => {
                self.save_msg = format!("Saved to ~/Music/cliamp/{filename}");
                self.save_msg_ttl = 60;
            }
            Err(e) => {
                self.save_msg = format!("Save failed: {e}");
                self.save_msg_ttl = 40;
            }
        }
    }
}

/// Run the TUI event loop. Returns the App so callers can extract state (e.g. for config save).
pub async fn run(mut app: App, mut msg_rx: mpsc::UnboundedReceiver<AppMsg>) -> anyhow::Result<App> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(50);

    loop {
        terminal.draw(|f| app.render(f))?;

        // Poll for events with tick timeout
        tokio::select! {
            // Async messages from spawned tasks
            msg = msg_rx.recv() => {
                match msg {
                    Some(msg) => app.on_msg(msg),
                    None => break, // channel closed
                }
            }

            // Terminal events (crossterm) — poll with timeout for ticks
            _ = tokio::time::sleep(tick_rate) => {
                // Process any pending crossterm events
                while event::poll(Duration::ZERO)? {
                    if let Event::Key(key) = event::read()?
                        && key.kind == KeyEventKind::Press
                            && app.handle_key(key) {
                                // Quit requested
                                disable_raw_mode()?;
                                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                return Ok(app);
                            }
                }

                // Tick
                app.on_tick();
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::eq_presets::*;

    #[test]
    fn test_eq_preset_lookup() {
        assert_eq!(EQ_PRESETS[0].name, "Flat");
        assert_eq!(EQ_PRESETS[1].name, "Rock");
    }
}
