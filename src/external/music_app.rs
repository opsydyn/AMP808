#![allow(
    dead_code,
    reason = "Music.app control helpers are scaffolded ahead of full command exposure."
)]

use std::process::Command;

use anyhow::{Context, bail};

const FIELD_DELIM: char = '\x1f';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicAppPlayerState {
    Stopped,
    Paused,
    Playing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MusicAppSnapshot {
    pub state: MusicAppPlayerState,
    pub volume: u8,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub position_secs: f64,
    pub duration_secs: f64,
}

#[derive(Debug, Default)]
pub struct MusicAppController;

impl MusicAppController {
    pub fn new() -> Self {
        Self
    }

    pub fn snapshot(&self) -> anyhow::Result<MusicAppSnapshot> {
        let output = run_script(snapshot_script())?;
        parse_snapshot_output(&output)
    }

    pub fn playpause(&self) -> anyhow::Result<()> {
        run_script(playpause_script()).map(|_| ())
    }

    pub fn next_track(&self) -> anyhow::Result<()> {
        run_script(next_track_script()).map(|_| ())
    }

    pub fn previous_track(&self) -> anyhow::Result<()> {
        run_script(previous_track_script()).map(|_| ())
    }

    pub fn stop(&self) -> anyhow::Result<()> {
        run_script(stop_script()).map(|_| ())
    }

    pub fn set_volume(&self, volume: u8) -> anyhow::Result<()> {
        run_script(&set_volume_script(volume)).map(|_| ())
    }
}

fn parse_player_state(value: &str) -> anyhow::Result<MusicAppPlayerState> {
    match value.trim() {
        "stopped" => Ok(MusicAppPlayerState::Stopped),
        "paused" => Ok(MusicAppPlayerState::Paused),
        "playing" => Ok(MusicAppPlayerState::Playing),
        other => bail!("unknown Music.app player state: {other}"),
    }
}

fn parse_snapshot_output(output: &str) -> anyhow::Result<MusicAppSnapshot> {
    let trimmed = output.trim_end_matches(['\r', '\n']);
    let fields: Vec<&str> = trimmed.split(FIELD_DELIM).collect();
    if fields.len() != 7 {
        bail!(
            "expected 7 fields from Music.app snapshot, got {}",
            fields.len()
        );
    }

    let state = parse_player_state(fields[0])?;
    let volume = fields[1]
        .parse::<u8>()
        .with_context(|| format!("invalid Music.app volume: {}", fields[1]))?;
    let position_secs = fields[5]
        .parse::<f64>()
        .with_context(|| format!("invalid Music.app position: {}", fields[5]))?;
    let duration_secs = fields[6]
        .parse::<f64>()
        .with_context(|| format!("invalid Music.app duration: {}", fields[6]))?;

    Ok(MusicAppSnapshot {
        state,
        volume,
        title: fields[2].to_string(),
        artist: fields[3].to_string(),
        album: fields[4].to_string(),
        position_secs,
        duration_secs,
    })
}

fn snapshot_script() -> &'static str {
    r#"tell application "Music"
  set d to ASCII character 31
  if player state is stopped then
    return (player state as string) & d & (sound volume as text) & d & "" & d & "" & d & "" & d & "0" & d & "0"
  else
    return (player state as string) & d & (sound volume as text) & d & (name of current track) & d & (artist of current track) & d & (album of current track) & d & (player position as text) & d & (duration of current track as text)
  end if
end tell"#
}

fn playpause_script() -> &'static str {
    "tell application \"Music\" to playpause"
}

fn next_track_script() -> &'static str {
    "tell application \"Music\" to next track"
}

fn previous_track_script() -> &'static str {
    "tell application \"Music\" to previous track"
}

fn stop_script() -> &'static str {
    "tell application \"Music\" to stop"
}

fn set_volume_script(volume: u8) -> String {
    format!(
        "tell application \"Music\" to set sound volume to {}",
        volume.min(100)
    )
}

fn ensure_supported_platform_with(is_macos: bool) -> anyhow::Result<()> {
    if !is_macos {
        bail!("Music.app control is only supported on macOS");
    }

    Ok(())
}

fn run_script(script: &str) -> anyhow::Result<String> {
    ensure_supported_platform_with(cfg!(target_os = "macos"))?;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .context("failed to execute osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Music.app command failed: {}", stderr.trim());
    }

    String::from_utf8(output.stdout).context("Music.app output was not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_player_state_recognizes_all_known_values() {
        assert_eq!(
            parse_player_state("playing").unwrap(),
            MusicAppPlayerState::Playing
        );
        assert_eq!(
            parse_player_state("paused").unwrap(),
            MusicAppPlayerState::Paused
        );
        assert_eq!(
            parse_player_state("stopped").unwrap(),
            MusicAppPlayerState::Stopped
        );
    }

    #[test]
    fn parse_player_state_rejects_unknown_value() {
        let err = parse_player_state("buffering").unwrap_err();
        assert!(err.to_string().contains("unknown Music.app player state"));
    }

    #[test]
    fn parse_snapshot_output_handles_stopped_state_without_track() {
        let output = "stopped\x1f41\x1f\x1f\x1f\x1f0\x1f0";

        let snapshot = parse_snapshot_output(output).unwrap();

        assert_eq!(snapshot.state, MusicAppPlayerState::Stopped);
        assert_eq!(snapshot.volume, 41);
        assert_eq!(snapshot.title, "");
        assert_eq!(snapshot.artist, "");
        assert_eq!(snapshot.album, "");
        assert_eq!(snapshot.position_secs, 0.0);
        assert_eq!(snapshot.duration_secs, 0.0);
    }

    #[test]
    fn parse_snapshot_output_handles_active_track_state() {
        let output = "playing\x1f55\x1fAround the World\x1fDaft Punk\x1fHomework\x1f12.5\x1f431";

        let snapshot = parse_snapshot_output(output).unwrap();

        assert_eq!(snapshot.state, MusicAppPlayerState::Playing);
        assert_eq!(snapshot.volume, 55);
        assert_eq!(snapshot.title, "Around the World");
        assert_eq!(snapshot.artist, "Daft Punk");
        assert_eq!(snapshot.album, "Homework");
        assert_eq!(snapshot.position_secs, 12.5);
        assert_eq!(snapshot.duration_secs, 431.0);
    }

    #[test]
    fn parse_snapshot_output_rejects_wrong_field_count() {
        let err = parse_snapshot_output("stopped\x1f41").unwrap_err();
        assert!(err.to_string().contains("expected 7 fields"));
    }

    #[test]
    fn snapshot_script_uses_safe_branch_and_custom_delimiter() {
        let script = snapshot_script();
        assert!(script.contains("ASCII character 31"));
        assert!(script.contains("if player state is stopped then"));
        assert!(script.contains("player position"));
        assert!(script.contains("duration of current track"));
    }

    #[test]
    fn transport_scripts_match_music_commands() {
        assert_eq!(
            playpause_script(),
            "tell application \"Music\" to playpause"
        );
        assert_eq!(
            next_track_script(),
            "tell application \"Music\" to next track"
        );
        assert_eq!(
            previous_track_script(),
            "tell application \"Music\" to previous track"
        );
        assert_eq!(stop_script(), "tell application \"Music\" to stop");
    }

    #[test]
    fn set_volume_script_is_clamped() {
        assert_eq!(
            set_volume_script(42),
            "tell application \"Music\" to set sound volume to 42"
        );
        assert_eq!(
            set_volume_script(u8::MAX),
            "tell application \"Music\" to set sound volume to 100"
        );
    }

    #[test]
    fn platform_guard_rejects_non_macos() {
        let err = ensure_supported_platform_with(false).unwrap_err();
        assert!(
            err.to_string()
                .contains("Music.app control is only supported on macOS")
        );
    }

    #[test]
    fn platform_guard_accepts_macos() {
        ensure_supported_platform_with(true).unwrap();
    }

    #[test]
    #[ignore = "requires macOS Music.app automation access"]
    fn live_snapshot_smoke_test() {
        let controller = MusicAppController::new();
        let snapshot = controller.snapshot().unwrap();

        assert!(snapshot.volume <= 100);
    }
}
