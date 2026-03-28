mod app_paths;
mod cli;
mod config;
mod external;
mod playback_backend;
mod player;
mod playlist;
mod resolve;
mod ui;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::cli::{BackendKind, parse_args};
use crate::config::Config;
use crate::external::apple_music_api::AppleMusicClient;
use crate::playback_backend::PlaybackBackend;
use crate::player::Player;
use crate::playlist::Playlist;
use crate::resolve::ytdl::YtdlTempTracker;
use crate::ui::{App, AppMsg};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = parse_args(std::env::args().skip(1))?;
    let backend_kind = cli.backend;
    let args = cli.inputs;

    // Check for Navidrome provider from env
    let provider: Option<Box<dyn playlist::Provider>> = if backend_kind == BackendKind::Local {
        external::navidrome::NavidromeClient::from_env()
            .map(|c| Box::new(c) as Box<dyn playlist::Provider>)
    } else {
        None
    };

    let (apple_music_client, apple_music_error) = if backend_kind == BackendKind::MusicApp {
        match AppleMusicClient::from_env() {
            Ok(client) => (Some(client), None),
            Err(err) => (None, Some(err.to_string())),
        }
    } else {
        (None, None)
    };

    if args.is_empty() && provider.is_none() && backend_kind == BackendKind::Local {
        let bin = app_paths::BINARY_NAME;
        eprintln!(
            "usage: {bin} [--backend <local|music-app>] <file|folder|url> [...]

    Local files     {bin} track.mp3 song.flac ~/Music
    HTTP stream     {bin} https://example.com/song.mp3
    Radio / M3U     {bin} http://radio.example.com/stream.m3u
    Podcast feed    {bin} https://example.com/podcast/feed.xml
    SoundCloud      {bin} https://soundcloud.com/user/sets/playlist
    YouTube         {bin} https://www.youtube.com/watch?v=...
    Bandcamp        {bin} https://artist.bandcamp.com/album/...
    Navidrome       NAVIDROME_URL=... NAVIDROME_USER=... NAVIDROME_PASS=... {bin}
    Music.app       {bin} --backend music-app

Formats: mp3, wav, flac, ogg, m4a, aac, opus, wma (aac/opus/wma need ffmpeg)
SoundCloud/YouTube/Bandcamp require yt-dlp (brew install yt-dlp)"
        );
        std::process::exit(1);
    }

    // Load config
    let cfg = Config::load().unwrap_or_default();

    // Setup yt-dlp temp tracker for cleanup
    let tracker = YtdlTempTracker::new();
    let tracker_cleanup = tracker.clone();

    // Register cleanup on Ctrl+C
    ctrlc::set_handler(move || {
        tracker_cleanup.cleanup();
        std::process::exit(0);
    })
    .ok();

    // Resolve args into tracks + pending URLs
    let resolved = if backend_kind == BackendKind::Local && !args.is_empty() {
        resolve::args(&args)?
    } else {
        resolve::ResolveResult {
            tracks: Vec::new(),
            pending: Vec::new(),
        }
    };

    // Build playlist
    let mut pl = Playlist::new();
    pl.add(&resolved.tracks);

    // Apply config to playlist
    if cfg.shuffle {
        pl.toggle_shuffle();
    }
    pl.set_repeat(cfg.repeat_mode());

    // Build player
    let player = match backend_kind {
        BackendKind::Local => {
            let player = Player::new()?;
            player.set_volume(cfg.volume);
            if cfg.mono {
                player.toggle_mono();
            }
            for (i, &gain) in cfg.eq.iter().enumerate() {
                player.set_eq_band(i, gain);
            }
            PlaybackBackend::local(player)
        }
        BackendKind::MusicApp => PlaybackBackend::music_app()?,
    };

    // Message channel for async tasks
    let (msg_tx, msg_rx) = mpsc::unbounded_channel::<AppMsg>();

    // Resolve pending URLs (feeds, M3U) in background
    if backend_kind == BackendKind::Local && !resolved.pending.is_empty() {
        let urls = resolved.pending.clone();
        let tx = msg_tx.clone();
        tokio::spawn(async move {
            match resolve::remote(&urls).await {
                Ok(tracks) => {
                    let _ = tx.send(AppMsg::FeedsLoaded(tracks));
                }
                Err(e) => {
                    let _ = tx.send(AppMsg::FeedError(e));
                }
            }
        });
    }

    // Build app
    let mut app = App::new(player, pl, tracker.clone(), msg_tx, provider);
    if backend_kind == BackendKind::MusicApp {
        app.configure_apple_music_browser(apple_music_client, apple_music_error);
    }

    if app.player.is_music_app() {
        app.sync_remote_backend();
    }

    // Apply EQ preset from config
    if app.player.supports_eq() && !cfg.eq_preset.is_empty() && cfg.eq_preset != "Custom" {
        for (i, preset) in ui::eq_presets::EQ_PRESETS.iter().enumerate() {
            if preset.name.eq_ignore_ascii_case(&cfg.eq_preset) {
                app.eq_preset_idx = Some(i);
                app.apply_eq_preset();
                break;
            }
        }
    }

    // Apply theme from config
    if !cfg.theme.is_empty() {
        let idx = ui::theme::find_by_name(&app.themes, &cfg.theme);
        app.theme_idx = idx;
    }

    // Apply 808 mode from config
    if cfg.mode_808 {
        app.mode_808 = true;
    }

    app.refresh_palette();

    // Auto-play first track if we have any
    if !app.playlist.is_empty() {
        app.play_current_track();
    }

    // Run TUI — returns App so we can save state
    let app = ui::run(app, msg_rx).await?;

    // Save config on exit — persist current player/playlist state
    let save_cfg = Config {
        volume: if app.player.is_music_app() {
            cfg.volume
        } else {
            app.player.volume()
        },
        repeat: match app.playlist.repeat() {
            playlist::RepeatMode::Off => "off".to_string(),
            playlist::RepeatMode::All => "all".to_string(),
            playlist::RepeatMode::One => "one".to_string(),
        },
        shuffle: app.playlist.shuffled(),
        mono: if app.player.is_music_app() {
            cfg.mono
        } else {
            app.player.mono()
        },
        eq_preset: if app.player.is_music_app() {
            cfg.eq_preset.clone()
        } else {
            app.eq_preset_idx.map_or_else(
                || "Custom".to_string(),
                |idx| {
                    ui::eq_presets::EQ_PRESETS
                        .get(idx)
                        .map_or("Custom".to_string(), |p| p.name.to_string())
                },
            )
        },
        theme: app
            .theme_idx
            .and_then(|i| app.themes.get(i))
            .map_or_else(String::new, |t| t.name.clone()),
        eq: if app.player.is_music_app() {
            cfg.eq.clone()
        } else {
            app.player.eq_bands().to_vec()
        },
        mode_808: app.mode_808,
    };
    if let Err(e) = save_cfg.save() {
        eprintln!("warning: failed to save config: {e}");
    }

    // Cleanup yt-dlp temp files
    tracker.cleanup();

    Ok(())
}
