use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tokio::process::Command;

use crate::app_paths;
use crate::playlist::Track;

struct YtdlTempTrackerInner {
    dirs: Mutex<Vec<PathBuf>>,
}

impl Drop for YtdlTempTrackerInner {
    fn drop(&mut self) {
        let dirs = std::mem::take(self.dirs.get_mut().unwrap());
        for dir in dirs {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// JSON fields from `yt-dlp --flat-playlist -j` output (one per line).
#[derive(Deserialize, Debug)]
struct YtdlFlatEntry {
    url: Option<String>,
    webpage_url: Option<String>,
    title: Option<String>,
    uploader: Option<String>,
    playlist_uploader: Option<String>,
    webpage_url_basename: Option<String>,
}

/// JSON fields from `yt-dlp --print-json` output (download mode).
#[derive(Deserialize, Debug)]
struct YtdlFullEntry {
    title: Option<String>,
    uploader: Option<String>,
    #[serde(rename = "_filename")]
    filename: Option<String>,
}

fn parse_download_output(stdout: &str) -> (Option<YtdlFullEntry>, Option<PathBuf>) {
    let mut entry = None;
    let mut printed_path = None;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Ok(parsed) = serde_json::from_str::<YtdlFullEntry>(line) {
            entry = Some(parsed);
            continue;
        }

        let candidate = PathBuf::from(line);
        if candidate.exists() {
            printed_path = Some(candidate);
        }
    }

    let json_path = entry
        .as_ref()
        .and_then(|e| e.filename.as_deref())
        .map(PathBuf::from)
        .filter(|p| p.exists());

    (entry, printed_path.or(json_path))
}

#[derive(thiserror::Error, Debug)]
pub enum YtdlError {
    #[error("yt-dlp not found in PATH — see https://github.com/yt-dlp/yt-dlp#installation")]
    NotFound,

    #[error("yt-dlp: {0}")]
    ProcessError(String),

    #[error("yt-dlp: no file downloaded for {0}")]
    NoFileDownloaded(String),

    #[error("creating temp dir: {0}")]
    TempDir(#[from] std::io::Error),
}

/// Tracks temp directories created by yt-dlp downloads for cleanup.
#[derive(Clone)]
pub struct YtdlTempTracker {
    inner: Arc<YtdlTempTrackerInner>,
}

impl Default for YtdlTempTracker {
    fn default() -> Self {
        Self {
            inner: Arc::new(YtdlTempTrackerInner {
                dirs: Mutex::new(Vec::new()),
            }),
        }
    }
}

impl YtdlTempTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a temp directory for cleanup tracking.
    fn register(&self, dir: PathBuf) {
        self.inner.dirs.lock().unwrap().push(dir);
    }

    /// Remove all tracked temp directories.
    pub fn cleanup(&self) {
        let dirs = {
            let mut guard = self.inner.dirs.lock().unwrap();
            std::mem::take(&mut *guard)
        };
        for dir in dirs {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Check that yt-dlp is available in PATH.
async fn check_ytdlp() -> Result<(), YtdlError> {
    let status = Command::new("yt-dlp")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| YtdlError::NotFound)?;
    if !status.success() {
        return Err(YtdlError::NotFound);
    }
    Ok(())
}

/// Phase 1: Fast enumerate via `yt-dlp --flat-playlist -j <URL>`.
/// Returns tracks with page URLs as Path, marked stream=true.
pub async fn resolve_ytdl_playlist(page_url: &str) -> Result<Vec<Track>, YtdlError> {
    check_ytdlp().await?;

    let output = Command::new("yt-dlp")
        .args(["--flat-playlist", "-j", page_url])
        .output()
        .await
        .map_err(|e| YtdlError::ProcessError(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            return Err(YtdlError::ProcessError(stderr));
        }
        return Err(YtdlError::ProcessError(format!(
            "exit code {}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut tracks = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<YtdlFlatEntry>(line) else {
            continue;
        };

        let track_url = entry
            .webpage_url
            .as_deref()
            .or(entry.url.as_deref())
            .unwrap_or("")
            .to_string();
        if track_url.is_empty() {
            continue;
        }

        let title = entry
            .title
            .filter(|t| !t.is_empty())
            .or_else(|| entry.webpage_url_basename.map(|b| humanize_basename(&b)))
            .unwrap_or_else(|| track_url.clone());

        let artist = entry
            .uploader
            .filter(|u| !u.is_empty())
            .or(entry.playlist_uploader)
            .unwrap_or_default();

        tracks.push(Track {
            path: track_url,
            title,
            artist,
            stream: true,
        });
    }

    Ok(tracks)
}

/// Phase 2: Lazy download single track via yt-dlp.
/// Downloads to a temp directory and returns a Track pointing to the local file.
pub async fn resolve_ytdl_track(
    page_url: &str,
    tracker: &YtdlTempTracker,
) -> Result<Track, YtdlError> {
    // Create temp directory
    let tmp_dir = std::env::temp_dir().join(format!(
        "{}-{}",
        app_paths::YTDL_TEMP_PREFIX,
        std::process::id() as u64
            ^ (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64)
    ));
    std::fs::create_dir_all(&tmp_dir)?;
    tracker.register(tmp_dir.clone());

    let out_template = tmp_dir.join("%(id)s.%(ext)s");

    let output = Command::new("yt-dlp")
        .args([
            "-f",
            "bestaudio[protocol=https]/bestaudio[protocol=http]/bestaudio",
            "--no-playlist",
            "--print-json",
            "--print",
            "after_move:filepath",
            "-o",
            &out_template.to_string_lossy(),
            page_url,
        ])
        .output()
        .await
        .map_err(|e| YtdlError::ProcessError(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            return Err(YtdlError::ProcessError(stderr));
        }
        return Err(YtdlError::ProcessError(format!(
            "exit code {}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (entry, file_path) = parse_download_output(&stdout);
    let file_path = file_path.or_else(|| find_first_file(&tmp_dir));

    let Some(file_path) = file_path else {
        return Err(YtdlError::NoFileDownloaded(page_url.to_string()));
    };

    let title = entry
        .as_ref()
        .and_then(|e| e.title.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| page_url.to_string());

    let artist = entry
        .as_ref()
        .and_then(|e| e.uploader.clone())
        .unwrap_or_default();

    Ok(Track {
        path: file_path.to_string_lossy().to_string(),
        title,
        artist,
        stream: false,
    })
}

/// Find the first file in a directory (fallback when JSON parsing fails).
fn find_first_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.find_map(|entry| {
        let entry = entry.ok()?;
        if entry.file_type().ok()?.is_file() {
            Some(entry.path())
        } else {
            None
        }
    })
}

/// Convert a URL basename like "clr-podcast-467" into "clr podcast 467".
fn humanize_basename(s: &str) -> String {
    s.replace('-', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_humanize_basename() {
        assert_eq!(humanize_basename("clr-podcast-467"), "clr podcast 467");
        assert_eq!(humanize_basename("no-dashes-here"), "no dashes here");
        assert_eq!(humanize_basename("simple"), "simple");
    }

    #[test]
    fn test_parse_flat_entry() {
        let json = r#"{"url":"https://youtube.com/watch?v=abc","webpage_url":"https://youtube.com/watch?v=abc","title":"Cool Song","uploader":"Artist Name","playlist_uploader":"","webpage_url_basename":"abc"}"#;
        let entry: YtdlFlatEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.title.as_deref(), Some("Cool Song"));
        assert_eq!(entry.uploader.as_deref(), Some("Artist Name"));
        assert_eq!(
            entry.webpage_url.as_deref(),
            Some("https://youtube.com/watch?v=abc")
        );
    }

    #[test]
    fn test_parse_flat_entry_minimal() {
        let json = r#"{"url":"https://example.com/track"}"#;
        let entry: YtdlFlatEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.url.as_deref(), Some("https://example.com/track"));
        assert!(entry.title.is_none());
        assert!(entry.webpage_url.is_none());
    }

    #[test]
    fn test_parse_full_entry() {
        let json = r#"{"title":"Song Title","uploader":"Channel","_filename":"/tmp/amp808-ytdl-123/abc.webm"}"#;
        let entry: YtdlFullEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.title.as_deref(), Some("Song Title"));
        assert_eq!(entry.uploader.as_deref(), Some("Channel"));
        assert_eq!(
            entry.filename.as_deref(),
            Some("/tmp/amp808-ytdl-123/abc.webm")
        );
    }

    #[test]
    fn test_parse_full_entry_missing_fields() {
        let json = r#"{}"#;
        let entry: YtdlFullEntry = serde_json::from_str(json).unwrap();
        assert!(entry.title.is_none());
        assert!(entry.filename.is_none());
    }

    #[test]
    fn test_extract_download_output_prefers_after_move_filepath() {
        let tmp = std::env::temp_dir().join("amp808-test-ytdlp-after-move.opus");
        std::fs::write(&tmp, b"fake audio").unwrap();

        let stdout = format!(
            "{{\"title\":\"Twin Peaks Theme\",\"uploader\":\"DJ Dado\",\"_filename\":\"/tmp/placeholder.opus\"}}\n{}\n",
            tmp.display()
        );

        let (entry, file_path) = parse_download_output(&stdout);
        assert_eq!(entry.and_then(|e| e.title), Some("Twin Peaks Theme".into()));
        assert_eq!(file_path.as_deref(), Some(tmp.as_path()));

        std::fs::remove_file(tmp).unwrap();
    }

    #[test]
    fn test_extract_download_output_falls_back_to_json_filename() {
        let tmp = std::env::temp_dir().join("amp808-test-ytdlp-json-filename.opus");
        std::fs::write(&tmp, b"fake audio").unwrap();

        let stdout = format!(
            "{{\"title\":\"Twin Peaks Theme\",\"uploader\":\"DJ Dado\",\"_filename\":\"{}\"}}\n",
            tmp.display()
        );

        let (entry, file_path) = parse_download_output(&stdout);
        assert_eq!(entry.and_then(|e| e.uploader), Some("DJ Dado".into()));
        assert_eq!(file_path.as_deref(), Some(tmp.as_path()));

        std::fs::remove_file(tmp).unwrap();
    }

    #[test]
    fn test_temp_tracker_register_and_cleanup() {
        let tracker = YtdlTempTracker::new();

        // Create an actual temp dir
        let tmp = std::env::temp_dir().join("amp808-test-ytdl-tracker");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("test.txt"), "data").unwrap();

        tracker.register(tmp.clone());
        assert!(tmp.exists());

        tracker.cleanup();
        assert!(!tmp.exists());
    }

    #[test]
    fn test_temp_tracker_cleanup_idempotent() {
        let tracker = YtdlTempTracker::new();
        tracker.cleanup(); // no dirs registered, should not panic
        tracker.cleanup(); // call again, still fine
    }

    #[test]
    fn test_temp_tracker_clone_drop_does_not_cleanup() {
        let tracker = YtdlTempTracker::new();

        let tmp = std::env::temp_dir().join("amp808-test-ytdl-clone-drop");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("test.txt"), "data").unwrap();

        tracker.register(tmp.clone());
        {
            let clone = tracker.clone();
            drop(clone);
        }

        assert!(tmp.exists());

        tracker.cleanup();
        assert!(!tmp.exists());
    }

    #[test]
    fn test_temp_tracker_last_drop_cleans_up() {
        let tmp = std::env::temp_dir().join("amp808-test-ytdl-last-drop");

        {
            let tracker = YtdlTempTracker::new();
            std::fs::create_dir_all(&tmp).unwrap();
            std::fs::write(tmp.join("test.txt"), "data").unwrap();
            tracker.register(tmp.clone());
            assert!(tmp.exists());
        }

        assert!(!tmp.exists());
    }

    #[test]
    fn test_find_first_file() {
        let tmp = std::env::temp_dir().join("amp808-test-find-first");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("audio.webm"), "fake audio").unwrap();

        let found = find_first_file(&tmp);
        assert!(found.is_some());
        assert_eq!(found.unwrap().file_name().unwrap(), "audio.webm");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_find_first_file_empty_dir() {
        let tmp = std::env::temp_dir().join("amp808-test-find-empty");
        std::fs::create_dir_all(&tmp).unwrap();

        let found = find_first_file(&tmp);
        assert!(found.is_none());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_find_first_file_nonexistent_dir() {
        let found = find_first_file(Path::new("/nonexistent/dir"));
        assert!(found.is_none());
    }

    #[test]
    fn test_ytdl_error_display() {
        let e = YtdlError::NotFound;
        assert!(e.to_string().contains("yt-dlp not found"));

        let e = YtdlError::ProcessError("something failed".into());
        assert_eq!(e.to_string(), "yt-dlp: something failed");

        let e = YtdlError::NoFileDownloaded("https://example.com".into());
        assert!(e.to_string().contains("no file downloaded"));
    }

    /// Test the full flat-entry to Track mapping logic (without subprocess).
    #[test]
    fn test_flat_entry_to_track_mapping() {
        // Simulate the mapping logic from resolve_ytdl_playlist
        let json_lines = [
            r#"{"webpage_url":"https://youtube.com/watch?v=1","title":"Song A","uploader":"Artist X"}"#,
            r#"{"url":"https://youtube.com/watch?v=2","title":"","webpage_url_basename":"cool-track-name"}"#,
            r#"{"url":"https://youtube.com/watch?v=3","uploader":"","playlist_uploader":"Playlist Author"}"#,
            r#"{}"#, // no URL, should be skipped
        ];

        let mut tracks = Vec::new();
        for line in &json_lines {
            let Ok(entry) = serde_json::from_str::<YtdlFlatEntry>(line) else {
                continue;
            };
            let track_url = entry
                .webpage_url
                .as_deref()
                .or(entry.url.as_deref())
                .unwrap_or("")
                .to_string();
            if track_url.is_empty() {
                continue;
            }
            let title = entry
                .title
                .filter(|t| !t.is_empty())
                .or_else(|| entry.webpage_url_basename.map(|b| humanize_basename(&b)))
                .unwrap_or_else(|| track_url.clone());
            let artist = entry
                .uploader
                .filter(|u| !u.is_empty())
                .or(entry.playlist_uploader)
                .unwrap_or_default();

            tracks.push(Track {
                path: track_url,
                title,
                artist,
                stream: true,
            });
        }

        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].title, "Song A");
        assert_eq!(tracks[0].artist, "Artist X");
        assert_eq!(tracks[1].title, "cool track name"); // humanized
        assert_eq!(tracks[2].artist, "Playlist Author"); // fallback
    }
}
