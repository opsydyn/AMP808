use std::sync::{Arc, RwLock};

use anyhow::{Result, bail};
use tokio::sync::mpsc;

use crate::external::music_app::{MusicAppController, MusicAppPlayerState, MusicAppSnapshot};
use crate::player::{DEFAULT_SAMPLE_RATE, Player, decode::CoverArt};
use crate::ui::AppMsg;

const MUSIC_APP_REFRESH_MS: u64 = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackBackendKind {
    Local,
    MusicApp,
}

pub struct MusicAppBackend {
    controller: MusicAppController,
    snapshot: Arc<RwLock<MusicAppSnapshot>>,
}

pub enum PlaybackBackend {
    Local(Player),
    MusicApp(MusicAppBackend),
}

impl PlaybackBackend {
    pub fn local(player: Player) -> Self {
        Self::Local(player)
    }

    pub fn music_app() -> Result<Self> {
        let controller = MusicAppController::new();
        let snapshot = controller.snapshot()?;
        Ok(Self::MusicApp(MusicAppBackend {
            controller,
            snapshot: Arc::new(RwLock::new(snapshot)),
        }))
    }

    pub fn kind(&self) -> PlaybackBackendKind {
        match self {
            Self::Local(_) => PlaybackBackendKind::Local,
            Self::MusicApp(_) => PlaybackBackendKind::MusicApp,
        }
    }

    pub fn is_music_app(&self) -> bool {
        self.kind() == PlaybackBackendKind::MusicApp
    }

    pub fn refresh_interval_ms(&self) -> u64 {
        match self {
            Self::Local(_) => 0,
            Self::MusicApp(_) => MUSIC_APP_REFRESH_MS,
        }
    }

    pub fn refresh_remote_state(&self) -> Result<()> {
        let Self::MusicApp(backend) = self else {
            return Ok(());
        };

        let snapshot = backend.controller.snapshot()?;
        *backend.snapshot.write().unwrap() = snapshot;
        Ok(())
    }

    pub fn now_playing_title(&self) -> Option<String> {
        let Self::MusicApp(backend) = self else {
            return None;
        };

        let snapshot = backend.snapshot.read().unwrap();
        if snapshot.title.is_empty() {
            return None;
        }
        if snapshot.artist.is_empty() {
            return Some(snapshot.title.clone());
        }

        Some(format!("{} - {}", snapshot.artist, snapshot.title))
    }

    pub fn supports_seek(&self) -> bool {
        match self {
            Self::Local(player) => player.seekable(),
            Self::MusicApp(_) => false,
        }
    }

    pub fn supports_eq(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    pub fn supports_visualizer(&self) -> bool {
        matches!(self, Self::Local(_) | Self::MusicApp(_))
    }

    pub fn supports_cover_art(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    pub fn supports_local_playlist(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    pub fn sample_rate(&self) -> u32 {
        match self {
            Self::Local(player) => player.sample_rate(),
            Self::MusicApp(_) => DEFAULT_SAMPLE_RATE,
        }
    }

    pub fn samples(&self) -> Vec<f64> {
        match self {
            Self::Local(player) => player.samples(),
            Self::MusicApp(_) => Vec::new(),
        }
    }

    pub fn is_playing(&self) -> bool {
        match self {
            Self::Local(player) => player.is_playing(),
            Self::MusicApp(backend) => {
                backend.snapshot.read().unwrap().state != MusicAppPlayerState::Stopped
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        match self {
            Self::Local(player) => player.is_paused(),
            Self::MusicApp(backend) => {
                backend.snapshot.read().unwrap().state == MusicAppPlayerState::Paused
            }
        }
    }

    pub fn is_streaming(&self) -> bool {
        matches!(self, Self::Local(player) if player.is_playing() && !player.seekable())
    }

    pub fn stream_title(&self) -> String {
        match self {
            Self::Local(player) => player.stream_title(),
            Self::MusicApp(_) => String::new(),
        }
    }

    pub fn cover_art(&self) -> Option<CoverArt> {
        match self {
            Self::Local(player) => player.cover_art(),
            Self::MusicApp(_) => None,
        }
    }

    pub fn clear_preload(&self) -> Result<()> {
        match self {
            Self::Local(player) => {
                player.clear_preload();
                Ok(())
            }
            Self::MusicApp(_) => Ok(()),
        }
    }

    pub fn toggle_pause(&self) -> Result<()> {
        match self {
            Self::Local(player) => {
                player.toggle_pause();
                Ok(())
            }
            Self::MusicApp(backend) => {
                backend.controller.playpause()?;
                self.refresh_remote_state()
            }
        }
    }

    pub fn stop(&self) -> Result<()> {
        match self {
            Self::Local(player) => {
                player.stop();
                Ok(())
            }
            Self::MusicApp(backend) => {
                backend.controller.stop()?;
                self.refresh_remote_state()
            }
        }
    }

    pub fn skip_next(&self) -> Result<()> {
        match self {
            Self::Local(_) => bail!("skip_next is only supported by the Music.app backend"),
            Self::MusicApp(backend) => {
                backend.controller.next_track()?;
                self.refresh_remote_state()
            }
        }
    }

    pub fn skip_previous(&self) -> Result<()> {
        match self {
            Self::Local(_) => bail!("skip_previous is only supported by the Music.app backend"),
            Self::MusicApp(backend) => {
                backend.controller.previous_track()?;
                self.refresh_remote_state()
            }
        }
    }

    pub fn gapless_advanced(&self) -> bool {
        match self {
            Self::Local(player) => player.gapless_advanced(),
            Self::MusicApp(_) => false,
        }
    }

    pub fn drained(&self) -> bool {
        match self {
            Self::Local(player) => player.drained(),
            Self::MusicApp(_) => false,
        }
    }

    pub fn play(&self, path: &str) -> Result<()> {
        match self {
            Self::Local(player) => player.play(path),
            Self::MusicApp(_) => bail!("music-app backend cannot play local files or URLs"),
        }
    }

    pub fn preload(&self, path: &str) -> Result<()> {
        match self {
            Self::Local(player) => player.preload(path),
            Self::MusicApp(_) => Ok(()),
        }
    }

    pub fn play_async(&self, path: String, tx: mpsc::UnboundedSender<AppMsg>) {
        match self {
            Self::Local(player) => player.play_async(path, tx),
            Self::MusicApp(_) => {
                let _ = tx.send(AppMsg::StreamPlayed {
                    error: Some("music-app backend cannot play local files or URLs".to_string()),
                });
            }
        }
    }

    pub fn volume(&self) -> f64 {
        match self {
            Self::Local(player) => player.volume(),
            Self::MusicApp(backend) => music_volume_to_db(backend.snapshot.read().unwrap().volume),
        }
    }

    pub fn set_volume(&self, db: f64) -> Result<()> {
        match self {
            Self::Local(player) => {
                player.set_volume(db);
                Ok(())
            }
            Self::MusicApp(backend) => {
                backend.controller.set_volume(db_to_music_volume(db))?;
                self.refresh_remote_state()
            }
        }
    }

    pub fn eq_bands(&self) -> [f64; 10] {
        match self {
            Self::Local(player) => player.eq_bands(),
            Self::MusicApp(_) => [0.0; 10],
        }
    }

    pub fn set_eq_band(&self, band: usize, db: f64) -> Result<()> {
        match self {
            Self::Local(player) => {
                player.set_eq_band(band, db);
                Ok(())
            }
            Self::MusicApp(_) => Ok(()),
        }
    }

    pub fn mono(&self) -> bool {
        match self {
            Self::Local(player) => player.mono(),
            Self::MusicApp(_) => false,
        }
    }

    pub fn toggle_mono(&self) -> Result<()> {
        match self {
            Self::Local(player) => {
                player.toggle_mono();
                Ok(())
            }
            Self::MusicApp(_) => Ok(()),
        }
    }

    pub fn track_position(&self) -> (u64, u64) {
        match self {
            Self::Local(player) => player.track_position(),
            Self::MusicApp(backend) => {
                let snapshot = backend.snapshot.read().unwrap();
                (
                    snapshot.position_secs.max(0.0).floor() as u64,
                    snapshot.duration_secs.max(0.0).floor() as u64,
                )
            }
        }
    }

    pub fn seek_to(&self, seconds: f64) -> Result<()> {
        match self {
            Self::Local(player) => player.seek_to(seconds),
            Self::MusicApp(_) => bail!("music-app backend does not support seeking yet"),
        }
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<()> {
        match self {
            Self::Local(player) => player.seek_relative(seconds),
            Self::MusicApp(_) => bail!("music-app backend does not support seeking yet"),
        }
    }

    #[cfg(test)]
    pub(crate) fn music_app_for_test(snapshot: MusicAppSnapshot) -> Self {
        Self::MusicApp(MusicAppBackend {
            controller: MusicAppController::new(),
            snapshot: Arc::new(RwLock::new(snapshot)),
        })
    }
}

fn music_volume_to_db(volume: u8) -> f64 {
    -30.0 + (f64::from(volume) / 100.0) * 36.0
}

fn db_to_music_volume(db: f64) -> u8 {
    (((db.clamp(-30.0, 6.0) + 30.0) / 36.0) * 100.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::{
        MUSIC_APP_REFRESH_MS, PlaybackBackend, PlaybackBackendKind, db_to_music_volume,
        music_volume_to_db,
    };
    use crate::external::music_app::{MusicAppPlayerState, MusicAppSnapshot};

    fn remote_backend(snapshot: MusicAppSnapshot) -> PlaybackBackend {
        PlaybackBackend::music_app_for_test(snapshot)
    }

    #[test]
    fn music_app_backend_reports_limited_capabilities() {
        let backend = remote_backend(MusicAppSnapshot {
            state: MusicAppPlayerState::Playing,
            volume: 50,
            title: "Alive".to_string(),
            artist: "Daft Punk".to_string(),
            album: "Homework".to_string(),
            position_secs: 12.0,
            duration_secs: 300.0,
        });

        assert_eq!(backend.kind(), PlaybackBackendKind::MusicApp);
        assert!(backend.is_music_app());
        assert!(!backend.supports_seek());
        assert!(!backend.supports_eq());
        assert!(backend.supports_visualizer());
        assert!(!backend.supports_cover_art());
        assert!(!backend.supports_local_playlist());
        assert_eq!(backend.refresh_interval_ms(), MUSIC_APP_REFRESH_MS);
    }

    #[test]
    fn music_app_backend_uses_cached_snapshot_for_state() {
        let backend = remote_backend(MusicAppSnapshot {
            state: MusicAppPlayerState::Paused,
            volume: 25,
            title: "Alive".to_string(),
            artist: "Daft Punk".to_string(),
            album: "Homework".to_string(),
            position_secs: 62.9,
            duration_secs: 315.2,
        });

        assert!(backend.is_playing());
        assert!(backend.is_paused());
        assert_eq!(backend.track_position(), (62, 315));
        assert_eq!(
            backend.now_playing_title().as_deref(),
            Some("Daft Punk - Alive")
        );
    }

    #[test]
    fn music_app_volume_conversion_round_trips_reasonably() {
        for volume in [0u8, 25, 50, 75, 100] {
            let db = music_volume_to_db(volume);
            assert_eq!(db_to_music_volume(db), volume);
        }
    }
}
