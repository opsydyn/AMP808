pub mod command;
pub mod eq_presets;
pub mod explorer;
pub mod keys;
pub mod styles;
pub mod theme;
pub mod view;
pub mod view_808;
pub mod visualizer;

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use self::command::{UiCommand, parse_command};
use self::eq_presets::EQ_PRESETS;
use self::explorer::PlaylistExplorer;
use self::keys::Focus;
use self::styles::Palette;
use self::theme::Theme;
use self::visualizer::Visualizer;
use crate::external::apple_music_api::{AppleMusicClient, LibraryTrack};
use crate::playback_backend::PlaybackBackend;
use crate::playlist::{self, Playlist, PlaylistInfo, Provider, Track};
use crate::resolve;
use crate::resolve::ytdl::YtdlTempTracker;

/// Messages sent from async tasks to the main event loop.
pub enum AppMsg {
    YtdlResolved {
        index: usize,
        track: Track,
    },
    YtdlError {
        error: anyhow::Error,
    },
    FeedsLoaded(Vec<Track>),
    FeedError(anyhow::Error),
    StreamPlayed {
        error: Option<String>,
    },
    ProviderPlaylists(Vec<PlaylistInfo>),
    ProviderTracks(Vec<Track>),
    ProviderError(String),
    AppleMusicPlaylists(Vec<PlaylistInfo>),
    AppleMusicTracks {
        playlist_id: String,
        playlist_name: String,
        tracks: Vec<LibraryTrack>,
    },
    AppleMusicError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleMusicTrackContext {
    pub playlist_id: String,
    pub playlist_name: String,
}

/// The main application state.
pub struct App {
    pub player: PlaybackBackend,
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
    pub explorer: Option<PlaylistExplorer>,
    pub command_mode: bool,
    pub command_input: String,
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
    pub show_cover_art: bool,
    pub cover_art_proto: Option<ratatui_image::protocol::Protocol>,
    cover_art_loaded: bool,
    image_picker: Option<ratatui_image::picker::Picker>,
    pub provider: Option<std::sync::Arc<dyn Provider>>,
    pub provider_lists: Vec<PlaylistInfo>,
    pub prov_cursor: usize,
    pub prov_loading: bool,
    pub apple_music_client: Option<std::sync::Arc<AppleMusicClient>>,
    pub apple_music_tracks: Vec<LibraryTrack>,
    pub apple_music_track_context: Option<AppleMusicTrackContext>,
    pub apple_music_error: Option<String>,
    pub fx_808_border: Option<tachyonfx::Effect>,
    pub fx_808_header: Option<tachyonfx::Effect>,
    pub fx_808_focus: Option<tachyonfx::Effect>,
    pub fx_last_frame: Instant,
    last_backend_refresh: Instant,
    tracker: YtdlTempTracker,
    msg_tx: mpsc::UnboundedSender<AppMsg>,
    last_playlist_file: Option<PathBuf>,
}

impl App {
    pub fn new(
        player: PlaybackBackend,
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
        let image_picker = ratatui_image::picker::Picker::from_query_stdio().ok();
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
            explorer: None,
            command_mode: false,
            command_input: String::new(),
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
            show_cover_art: true,
            cover_art_proto: None,
            cover_art_loaded: false,
            image_picker,
            provider,
            provider_lists: Vec::new(),
            prov_cursor: 0,
            prov_loading: has_provider,
            apple_music_client: None,
            apple_music_tracks: Vec::new(),
            apple_music_track_context: None,
            apple_music_error: None,
            fx_808_border: None,
            fx_808_header: None,
            fx_808_focus: None,
            fx_last_frame: Instant::now(),
            last_backend_refresh: Instant::now(),
            tracker,
            msg_tx,
            last_playlist_file: None,
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

        if self.player.is_music_app() {
            let refresh_every = Duration::from_millis(self.player.refresh_interval_ms());
            if self.last_backend_refresh.elapsed() >= refresh_every {
                self.sync_remote_backend();
            }
        } else {
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

            // Poll cover art (once per track)
            if !self.cover_art_loaded
                && let Some(art) = self.player.cover_art()
            {
                if let Ok(img) = image::load_from_memory(&art.data)
                    && let Some(ref picker) = self.image_picker
                    && let Ok(proto) = picker.new_protocol(
                        img,
                        ratatui::layout::Rect::new(0, 0, 24, 5),
                        ratatui_image::Resize::Fit(None),
                    )
                {
                    self.cover_art_proto = Some(proto);
                }
                self.cover_art_loaded = true;
            }

            // Check if drained (end of current, no next)
            if self.player.is_playing()
                && !self.player.is_paused()
                && self.player.drained()
                && !self.buffering
            {
                self.next_track();
            }
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
            AppMsg::AppleMusicPlaylists(lists) => {
                self.prov_loading = false;
                self.provider_lists = lists;
                self.prov_cursor = 0;
                self.apple_music_tracks.clear();
                self.apple_music_track_context = None;
                self.apple_music_error = None;
            }
            AppMsg::AppleMusicTracks {
                playlist_id,
                playlist_name,
                tracks,
            } => {
                self.prov_loading = false;
                self.prov_cursor = 0;
                self.apple_music_tracks = tracks;
                self.apple_music_track_context = Some(AppleMusicTrackContext {
                    playlist_id,
                    playlist_name,
                });
                self.apple_music_error = None;
            }
            AppMsg::AppleMusicError(e) => {
                self.prov_loading = false;
                self.apple_music_error = Some(e.clone());
                self.err = Some(e);
            }
        }
    }

    // --- Playback control ---

    pub fn toggle_play_pause(&mut self) {
        if !self.player.is_playing() && !self.player.is_music_app() {
            self.play_current_track();
            return;
        }

        if let Err(e) = self.player.toggle_pause() {
            self.err = Some(e.to_string());
        } else if self.player.is_music_app() {
            self.sync_remote_backend();
        }
    }

    pub fn next_track(&mut self) {
        if self.player.is_music_app() {
            if let Err(e) = self.player.skip_next() {
                self.err = Some(e.to_string());
            } else {
                self.sync_remote_backend();
            }
            return;
        }

        // next() returns Option<(&Track, bool)> — clone the track to avoid borrow
        let track = self.playlist.next().map(|(t, _)| t.clone());
        if let Some(track) = track {
            self.pl_cursor = self.playlist.index().unwrap_or(0);
            self.adjust_scroll();
            self.play_track_owned(track);
        } else {
            if let Err(e) = self.player.stop() {
                self.err = Some(e.to_string());
            }
        }
    }

    pub fn prev_track(&mut self) {
        if self.player.is_music_app() {
            if let Err(e) = self.player.skip_previous() {
                self.err = Some(e.to_string());
            } else {
                self.sync_remote_backend();
            }
            return;
        }

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
        if self.player.is_music_app() {
            self.err = Some("music-app backend cannot play local files or URLs".to_string());
            return;
        }

        self.stream_title.clear();
        self.cover_art_proto = None;
        self.cover_art_loaded = false;

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

    pub fn sync_remote_backend(&mut self) {
        if !self.player.is_music_app() {
            return;
        }

        match self.player.refresh_remote_state() {
            Ok(()) => {
                self.last_backend_refresh = Instant::now();
                self.stream_title = self.player.now_playing_title().unwrap_or_default();
            }
            Err(e) => {
                self.err = Some(e.to_string());
            }
        }
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
                if let Err(e) = self.player.set_eq_band(i, gain) {
                    self.err = Some(e.to_string());
                    break;
                }
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

    // --- Command mode ---

    pub fn execute_command_line(&mut self, line: &str) {
        match parse_command(line) {
            Ok(UiCommand::Load { path }) => self.load_playlist_from_file(&path),
            Err(err) => {
                self.err = Some(err);
            }
        }
    }

    fn load_playlist_from_file(&mut self, path: &str) {
        self.load_playlist_from_path(expand_tilde(path));
    }

    pub fn load_playlist_from_path(&mut self, path_buf: PathBuf) {
        if !self.player.supports_local_playlist() {
            self.err = Some("music-app backend does not support local playlist loading".into());
            return;
        }

        let tracks = match resolve::load_local_playlist_file(&path_buf) {
            Ok(tracks) => tracks,
            Err(err) => {
                self.err = Some(format!("load failed: {err}"));
                return;
            }
        };

        let mut next = Playlist::new();
        next.add(&tracks);
        next.set_repeat(self.playlist.repeat());
        if self.playlist.shuffled() {
            next.toggle_shuffle();
        }

        if let Err(e) = self.player.stop() {
            self.err = Some(e.to_string());
            return;
        }
        self.playlist = next;
        self.focus = Focus::Playlist;
        self.pl_cursor = 0;
        self.pl_scroll = 0;
        self.searching = false;
        self.search_query.clear();
        self.search_results.clear();
        self.search_cursor = 0;
        self.last_playlist_file = Some(path_buf.clone());
        self.err = None;
        self.save_msg = format!("Loaded {} tracks from {}", tracks.len(), path_buf.display());
        self.save_msg_ttl = 60;
        self.playlist.set_index(0);
        self.play_current_track();
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

    pub fn configure_apple_music_browser(
        &mut self,
        client: Option<AppleMusicClient>,
        setup_error: Option<String>,
    ) {
        self.apple_music_client = client.map(std::sync::Arc::new);
        self.apple_music_error = setup_error;
        self.apple_music_tracks.clear();
        self.apple_music_track_context = None;
        self.provider_lists.clear();
        self.prov_cursor = 0;

        if self.apple_music_client.is_some() {
            self.prov_loading = true;
            if self.playlist.is_empty() {
                self.focus = Focus::Provider;
            }
            self.fetch_apple_music_playlists();
        } else {
            self.prov_loading = false;
            if let Some(err) = self.apple_music_error.as_ref() {
                self.save_msg = format!("Apple Music metadata unavailable: {err}");
                self.save_msg_ttl = 80;
            }
        }
    }

    pub fn has_provider_pane(&self) -> bool {
        self.provider.is_some() || self.apple_music_client.is_some()
    }

    pub fn browsing_apple_music(&self) -> bool {
        self.player.is_music_app() && self.apple_music_client.is_some()
    }

    pub fn apple_music_showing_tracks(&self) -> bool {
        self.browsing_apple_music() && self.apple_music_track_context.is_some()
    }

    pub fn provider_item_count(&self) -> usize {
        if self.apple_music_showing_tracks() {
            self.apple_music_tracks.len()
        } else {
            self.provider_lists.len()
        }
    }

    pub fn fetch_apple_music_playlists(&self) {
        if let Some(ref client) = self.apple_music_client {
            let client = std::sync::Arc::clone(client);
            let tx = self.msg_tx.clone();
            tokio::spawn(async move {
                match client.library_playlists().await {
                    Ok(playlists) => {
                        let lists = playlists
                            .into_iter()
                            .map(|playlist| PlaylistInfo {
                                id: playlist.id,
                                name: playlist.name,
                                track_count: playlist.track_count.unwrap_or(0),
                            })
                            .collect();
                        let _ = tx.send(AppMsg::AppleMusicPlaylists(lists));
                    }
                    Err(err) => {
                        let _ = tx.send(AppMsg::AppleMusicError(err.to_string()));
                    }
                }
            });
        }
    }

    pub fn fetch_apple_music_tracks(&mut self, playlist_id: &str, playlist_name: &str) {
        if let Some(ref client) = self.apple_music_client {
            self.prov_loading = true;
            let client = std::sync::Arc::clone(client);
            let tx = self.msg_tx.clone();
            let playlist_id = playlist_id.to_string();
            let playlist_name = playlist_name.to_string();
            tokio::spawn(async move {
                match client.library_playlist_tracks(&playlist_id).await {
                    Ok(tracks) => {
                        let _ = tx.send(AppMsg::AppleMusicTracks {
                            playlist_id,
                            playlist_name,
                            tracks,
                        });
                    }
                    Err(err) => {
                        let _ = tx.send(AppMsg::AppleMusicError(err.to_string()));
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
        if self.browsing_apple_music() {
            "Apple Music"
        } else {
            self.provider.as_ref().map_or("Provider", |p| p.name())
        }
    }

    pub fn provider_header_text(&self) -> String {
        if let Some(ctx) = self.apple_music_track_context.as_ref() {
            format!("── {}: {} ──", self.provider_name(), ctx.playlist_name)
        } else {
            format!("── {} Playlists ──", self.provider_name())
        }
    }

    // --- Save ---

    pub fn save_track(&mut self) {
        if !self.player.supports_local_playlist() {
            self.save_msg = "Music.app backend does not support saving local tracks".to_string();
            self.save_msg_ttl = 40;
            return;
        }

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

fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    std::path::PathBuf::from(path)
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
    use super::{App, AppMsg, AppleMusicTrackContext};
    use crate::external::apple_music_api::LibraryTrack;
    use crate::external::music_app::{MusicAppPlayerState, MusicAppSnapshot};
    use crate::playback_backend::PlaybackBackend;
    use crate::playlist::{Playlist, PlaylistInfo};
    use crate::resolve::ytdl::YtdlTempTracker;
    use tokio::sync::mpsc;

    #[test]
    fn test_eq_preset_lookup() {
        assert_eq!(EQ_PRESETS[0].name, "Flat");
        assert_eq!(EQ_PRESETS[1].name, "Rock");
    }

    fn build_music_app_test_app() -> App {
        let (tx, _rx) = mpsc::unbounded_channel();
        App::new(
            PlaybackBackend::music_app_for_test(MusicAppSnapshot {
                state: MusicAppPlayerState::Stopped,
                volume: 50,
                title: String::new(),
                artist: String::new(),
                album: String::new(),
                position_secs: 0.0,
                duration_secs: 0.0,
            }),
            Playlist::new(),
            YtdlTempTracker::new(),
            tx,
            None,
        )
    }

    #[test]
    fn apple_music_tracks_message_enters_track_browser() {
        let mut app = build_music_app_test_app();

        app.on_msg(AppMsg::AppleMusicTracks {
            playlist_id: "playlist-1".into(),
            playlist_name: "Favorites".into(),
            tracks: vec![LibraryTrack {
                id: "track-1".into(),
                title: "Alive".into(),
                artist: "Daft Punk".into(),
                album: "Homework".into(),
            }],
        });

        assert_eq!(
            app.apple_music_track_context,
            Some(AppleMusicTrackContext {
                playlist_id: "playlist-1".into(),
                playlist_name: "Favorites".into(),
            })
        );
        assert_eq!(app.apple_music_tracks.len(), 1);
        assert_eq!(app.prov_cursor, 0);
    }

    #[test]
    fn apple_music_playlists_message_resets_track_browser() {
        let mut app = build_music_app_test_app();
        app.apple_music_track_context = Some(AppleMusicTrackContext {
            playlist_id: "playlist-1".into(),
            playlist_name: "Favorites".into(),
        });
        app.apple_music_tracks = vec![LibraryTrack {
            id: "track-1".into(),
            title: "Alive".into(),
            artist: "Daft Punk".into(),
            album: "Homework".into(),
        }];

        app.on_msg(AppMsg::AppleMusicPlaylists(vec![PlaylistInfo {
            id: "playlist-1".into(),
            name: "Favorites".into(),
            track_count: 1,
        }]));

        assert!(app.apple_music_track_context.is_none());
        assert!(app.apple_music_tracks.is_empty());
        assert_eq!(app.provider_lists.len(), 1);
        assert_eq!(app.prov_cursor, 0);
    }
}
