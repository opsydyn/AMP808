pub mod ytdl;

use std::io::BufRead;
use std::path::Path;

use crate::playlist::{self, Track};

/// Supported audio file extensions.
const SUPPORTED_EXTS: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "m4a", "aac", "m4b", "alac", "wma", "opus",
];

/// Result of parsing CLI arguments.
pub struct ResolveResult {
    /// Immediately resolved local file tracks.
    pub tracks: Vec<Track>,
    /// Remote URLs (feeds, M3U, yt-dlp) needing async resolution.
    pub pending: Vec<String>,
}

/// Separate CLI arguments into local tracks and pending remote URLs.
pub fn args(args: &[String]) -> anyhow::Result<ResolveResult> {
    let mut result = ResolveResult {
        tracks: Vec::new(),
        pending: Vec::new(),
    };
    let mut files = Vec::new();

    for arg in args {
        if playlist::is_url(arg) {
            if playlist::is_feed(arg) || playlist::is_m3u(arg) || playlist::is_ytdl(arg) {
                result.pending.push(arg.clone());
            } else {
                files.push(arg.clone());
            }
            continue;
        }
        // Try glob expansion, fall back to literal path
        let matches: Vec<_> = glob::glob(arg)
            .map(|paths| paths.filter_map(Result::ok).collect())
            .unwrap_or_else(|_| vec![std::path::PathBuf::from(arg)]);
        let matches = if matches.is_empty() {
            vec![std::path::PathBuf::from(arg)]
        } else {
            matches
        };
        for path in matches {
            let resolved = collect_audio_files(&path)?;
            files.extend(
                resolved
                    .into_iter()
                    .map(|p| p.to_string_lossy().to_string()),
            );
        }
    }

    for f in files {
        result.tracks.push(playlist::track_from_path(&f));
    }
    Ok(result)
}

/// Resolve remote URLs (feeds, M3U, yt-dlp) to tracks.
pub async fn remote(urls: &[String]) -> anyhow::Result<Vec<Track>> {
    let mut tracks = Vec::new();
    for url in urls {
        if playlist::is_ytdl(url) {
            let t = ytdl::resolve_ytdl_playlist(url).await?;
            tracks.extend(t);
        } else if playlist::is_feed(url) {
            let t = resolve_feed(url).await?;
            tracks.extend(t);
        } else if playlist::is_m3u(url) {
            let urls = resolve_m3u(url).await?;
            for u in urls {
                tracks.push(playlist::track_from_path(&u));
            }
        }
    }
    Ok(tracks)
}

/// Recursively collect audio files from a path.
/// If path is a file with supported extension, returns it.
/// If path is a directory, walks recursively collecting supported files.
fn collect_audio_files(path: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_dir() {
        if is_supported_ext(path) {
            return Ok(vec![path.to_path_buf()]);
        }
        return Ok(vec![]);
    }

    let mut files = Vec::new();
    collect_audio_files_recursive(path, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_audio_files_recursive(
    dir: &Path,
    files: &mut Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_audio_files_recursive(&path, files)?;
        } else if is_supported_ext(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_supported_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SUPPORTED_EXTS.contains(&ext.to_lowercase().as_str()))
}

/// Fetch and parse a podcast RSS feed, extracting audio enclosure URLs.
async fn resolve_feed(feed_url: &str) -> anyhow::Result<Vec<Track>> {
    let resp = reqwest::get(feed_url).await?;
    let body = resp.bytes().await?;

    #[derive(Deserialize, Debug)]
    struct Rss {
        channel: Channel,
    }
    #[derive(Deserialize, Debug)]
    struct Channel {
        title: Option<String>,
        #[serde(default)]
        item: Vec<Item>,
    }
    #[derive(Deserialize, Debug)]
    struct Item {
        title: Option<String>,
        enclosure: Option<Enclosure>,
    }
    #[derive(Deserialize, Debug)]
    struct Enclosure {
        #[serde(rename = "@url")]
        url: Option<String>,
    }

    use serde::Deserialize;

    let rss: Rss = quick_xml::de::from_reader(body.as_ref())?;
    let channel_title = rss.channel.title.unwrap_or_default();

    let mut tracks = Vec::new();
    for item in rss.channel.item {
        let Some(enc) = item.enclosure else {
            continue;
        };
        let Some(url) = enc.url else { continue };
        if url.is_empty() {
            continue;
        }
        tracks.push(Track {
            path: url,
            title: item.title.unwrap_or_default(),
            artist: channel_title.clone(),
            stream: true,
        });
    }
    Ok(tracks)
}

/// Fetch and parse an M3U playlist, extracting stream URLs.
async fn resolve_m3u(m3u_url: &str) -> anyhow::Result<Vec<String>> {
    let resp = reqwest::get(m3u_url).await?;
    let body = resp.bytes().await?;

    let mut urls = Vec::new();
    for line in body.as_ref().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        urls.push(line.to_string());
    }
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported_ext() {
        assert!(is_supported_ext(Path::new("song.mp3")));
        assert!(is_supported_ext(Path::new("song.MP3")));
        assert!(is_supported_ext(Path::new("song.flac")));
        assert!(is_supported_ext(Path::new("song.ogg")));
        assert!(is_supported_ext(Path::new("song.m4a")));
        assert!(is_supported_ext(Path::new("song.opus")));
        assert!(!is_supported_ext(Path::new("song.txt")));
        assert!(!is_supported_ext(Path::new("song.jpg")));
        assert!(!is_supported_ext(Path::new("noext")));
    }

    #[test]
    fn test_collect_audio_files_single_file() {
        let tmp = std::env::temp_dir().join("cliamp-test-collect-single");
        std::fs::create_dir_all(&tmp).unwrap();
        let mp3 = tmp.join("test.mp3");
        std::fs::write(&mp3, "fake").unwrap();
        let txt = tmp.join("notes.txt");
        std::fs::write(&txt, "not audio").unwrap();

        let result = collect_audio_files(&mp3).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], mp3);

        let result = collect_audio_files(&txt).unwrap();
        assert!(result.is_empty());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_collect_audio_files_directory() {
        let tmp = std::env::temp_dir().join("cliamp-test-collect-dir");
        let sub = tmp.join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(tmp.join("b.mp3"), "fake").unwrap();
        std::fs::write(tmp.join("a.flac"), "fake").unwrap();
        std::fs::write(sub.join("c.ogg"), "fake").unwrap();
        std::fs::write(tmp.join("readme.txt"), "not audio").unwrap();

        let result = collect_audio_files(&tmp).unwrap();
        assert_eq!(result.len(), 3);
        // Should be sorted
        let names: Vec<_> = result
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.flac"));
        assert!(names.contains(&"b.mp3"));
        assert!(names.contains(&"c.ogg"));

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_args_local_files() {
        let tmp = std::env::temp_dir().join("cliamp-test-args");
        std::fs::create_dir_all(&tmp).unwrap();
        let mp3 = tmp.join("song.mp3");
        std::fs::write(&mp3, "fake").unwrap();

        let result = args(&[mp3.to_string_lossy().to_string()]).unwrap();
        assert_eq!(result.tracks.len(), 1);
        assert_eq!(result.tracks[0].title, "song");
        assert!(result.pending.is_empty());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_args_urls_categorized() {
        let result = args(&[
            "https://youtube.com/watch?v=abc".into(),
            "https://example.com/feed.xml".into(),
            "https://example.com/playlist.m3u".into(),
            "https://example.com/direct.mp3".into(),
        ])
        .unwrap();

        // yt-dlp, feed, m3u go to pending
        assert_eq!(result.pending.len(), 3);
        assert!(
            result
                .pending
                .contains(&"https://youtube.com/watch?v=abc".to_string())
        );
        assert!(
            result
                .pending
                .contains(&"https://example.com/feed.xml".to_string())
        );
        assert!(
            result
                .pending
                .contains(&"https://example.com/playlist.m3u".to_string())
        );

        // Direct URL becomes a track
        assert_eq!(result.tracks.len(), 1);
        assert_eq!(result.tracks[0].path, "https://example.com/direct.mp3");
    }

    #[test]
    fn test_m3u_parsing() {
        // Test the M3U line parsing logic directly
        let content = b"#EXTM3U\n#EXTINF:-1,Station\nhttps://stream1.example.com\n\nhttps://stream2.example.com\n";
        let mut urls = Vec::new();
        for line in content.as_ref().lines() {
            let line = line.unwrap();
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            urls.push(line.to_string());
        }
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://stream1.example.com");
        assert_eq!(urls[1], "https://stream2.example.com");
    }
}
