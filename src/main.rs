mod config;
mod player;
mod playlist;
mod resolve;
mod ui;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::player::Player;
use crate::playlist::Playlist;
use crate::resolve::ytdl::YtdlTempTracker;
use crate::ui::{App, AppMsg};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!(
            "usage: cliamp <file|folder|url> [...]

  Local files     cliamp track.mp3 song.flac ~/Music
  HTTP stream     cliamp https://example.com/song.mp3
  Radio / M3U     cliamp http://radio.example.com/stream.m3u
  Podcast feed    cliamp https://example.com/podcast/feed.xml
  SoundCloud      cliamp https://soundcloud.com/user/sets/playlist
  YouTube         cliamp https://www.youtube.com/watch?v=...
  Bandcamp        cliamp https://artist.bandcamp.com/album/...

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
    let resolved = resolve::args(&args)?;

    // Build playlist
    let mut pl = Playlist::new();
    pl.add(&resolved.tracks);

    // Apply config to playlist
    if cfg.shuffle {
        pl.toggle_shuffle();
    }
    pl.set_repeat(cfg.repeat_mode());

    // Build player
    let player = Player::new()?;
    player.set_volume(cfg.volume);
    if cfg.mono {
        player.toggle_mono();
    }
    // Apply EQ bands from config
    for (i, &gain) in cfg.eq.iter().enumerate() {
        player.set_eq_band(i, gain);
    }

    // Message channel for async tasks
    let (msg_tx, msg_rx) = mpsc::unbounded_channel::<AppMsg>();

    // Resolve pending URLs (feeds, M3U) in background
    if !resolved.pending.is_empty() {
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
    let mut app = App::new(player, pl, tracker.clone(), msg_tx);

    // Apply EQ preset from config
    if !cfg.eq_preset.is_empty() && cfg.eq_preset != "Custom" {
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
        app.apply_theme(idx);
    }

    // Apply 808 mode from config
    if cfg.mode_808 {
        app.mode_808 = true;
        app.palette = ui::styles::Palette::tr808();
    }

    // Auto-play first track if we have any
    if !app.playlist.is_empty() {
        app.play_current_track();
    }

    // Run TUI — returns App so we can save state
    let app = ui::run(app, msg_rx).await?;

    // Save config on exit — persist current player/playlist state
    let save_cfg = Config {
        volume: app.player.volume(),
        repeat: match app.playlist.repeat() {
            playlist::RepeatMode::Off => "off".to_string(),
            playlist::RepeatMode::All => "all".to_string(),
            playlist::RepeatMode::One => "one".to_string(),
        },
        shuffle: app.playlist.shuffled(),
        mono: app.player.mono(),
        eq_preset: app.eq_preset_idx.map_or_else(
            || "Custom".to_string(),
            |idx| {
                ui::eq_presets::EQ_PRESETS
                    .get(idx)
                    .map_or("Custom".to_string(), |p| p.name.to_string())
            },
        ),
        theme: app
            .theme_idx
            .and_then(|i| app.themes.get(i))
            .map_or_else(String::new, |t| t.name.clone()),
        eq: app.player.eq_bands().to_vec(),
        mode_808: app.mode_808,
    };
    if let Err(e) = save_cfg.save() {
        eprintln!("warning: failed to save config: {e}");
    }

    // Cleanup yt-dlp temp files
    tracker.cleanup();

    Ok(())
}
