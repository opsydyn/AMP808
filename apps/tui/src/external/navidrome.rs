use std::time::SystemTime;

use crate::app_paths;
use crate::playlist::{PlaylistInfo, Provider, Track};

/// Navidrome/Subsonic API client.
pub struct NavidromeClient {
    url: String,
    user: String,
    password: String,
    client: reqwest::blocking::Client,
}

impl NavidromeClient {
    pub fn new(url: String, user: String, password: String) -> Self {
        Self {
            url,
            user,
            password,
            client: reqwest::blocking::Client::new(),
        }
    }

    /// Create from NAVIDROME_URL, NAVIDROME_USER, NAVIDROME_PASS env vars.
    /// Returns None if any are unset.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("NAVIDROME_URL").ok()?;
        let user = std::env::var("NAVIDROME_USER").ok()?;
        let pass = std::env::var("NAVIDROME_PASS").ok()?;
        if url.is_empty() || user.is_empty() || pass.is_empty() {
            return None;
        }
        Some(Self::new(url, user, pass))
    }

    /// Build an authenticated Subsonic API URL.
    fn build_url(&self, endpoint: &str, extra_params: &[(&str, &str)]) -> String {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let salt = format!("{nanos}");
        let token = format!("{:x}", md5::compute(format!("{}{}", self.password, salt)));

        let mut params = vec![
            ("u", self.user.as_str()),
            ("t", &token),
            ("s", &salt),
            ("v", "1.0.0"),
            ("c", app_paths::NAVIDROME_CLIENT_NAME),
            ("f", "json"),
        ];
        params.extend_from_slice(extra_params);

        let query: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding(v)))
            .collect();

        format!("{}/rest/{}?{}", self.url, endpoint, query.join("&"))
    }

    /// Generate a streaming URL for a track ID.
    fn stream_url(&self, id: &str) -> String {
        self.build_url("stream", &[("id", id), ("format", "mp3")])
    }
}

/// Minimal URL encoding for query parameter values.
fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

impl Provider for NavidromeClient {
    fn name(&self) -> &str {
        "Navidrome"
    }

    fn playlists(&self) -> anyhow::Result<Vec<PlaylistInfo>> {
        let url = self.build_url("getPlaylists", &[]);
        let resp: serde_json::Value = self.client.get(&url).send()?.json()?;

        let playlists = resp
            .pointer("/subsonic-response/playlists/playlist")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(playlists
            .iter()
            .filter_map(|p| {
                Some(PlaylistInfo {
                    id: p.get("id")?.as_str()?.to_string(),
                    name: p.get("name")?.as_str()?.to_string(),
                    track_count: p.get("songCount")?.as_u64()? as usize,
                })
            })
            .collect())
    }

    fn tracks(&self, playlist_id: &str) -> anyhow::Result<Vec<Track>> {
        let url = self.build_url("getPlaylist", &[("id", playlist_id)]);
        let resp: serde_json::Value = self.client.get(&url).send()?.json()?;

        let entries = resp
            .pointer("/subsonic-response/playlist/entry")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(entries
            .iter()
            .filter_map(|e| {
                let id = e.get("id")?.as_str()?;
                Some(Track {
                    path: self.stream_url(id),
                    title: e
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    artist: e
                        .get("artist")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    stream: true,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url_structure() {
        let client = NavidromeClient::new(
            "https://music.example.com".to_string(),
            "testuser".to_string(),
            "testpass".to_string(),
        );
        let url = client.build_url("getPlaylists", &[]);
        assert!(url.starts_with("https://music.example.com/rest/getPlaylists?"));
        assert!(url.contains("u=testuser"));
        assert!(url.contains("v=1.0.0"));
        assert!(url.contains("c=amp808"));
        assert!(url.contains("f=json"));
        assert!(url.contains("t=")); // token
        assert!(url.contains("s=")); // salt
    }

    #[test]
    fn test_stream_url() {
        let client = NavidromeClient::new(
            "https://music.example.com".to_string(),
            "user".to_string(),
            "pass".to_string(),
        );
        let url = client.stream_url("track123");
        assert!(url.contains("rest/stream?"));
        assert!(url.contains("id=track123"));
        assert!(url.contains("format=mp3"));
    }

    #[test]
    fn test_playlist_info_construction() {
        let info = PlaylistInfo {
            id: "pl1".to_string(),
            name: "My Playlist".to_string(),
            track_count: 42,
        };
        assert_eq!(info.id, "pl1");
        assert_eq!(info.name, "My Playlist");
        assert_eq!(info.track_count, 42);
    }
}
