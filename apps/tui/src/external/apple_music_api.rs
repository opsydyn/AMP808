#![allow(
    dead_code,
    reason = "Apple Music integration is scaffolded ahead of full UI/backend wiring."
)]

use anyhow::{Context, bail};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://api.music.apple.com/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleMusicConfig {
    pub developer_token: String,
    pub user_token: String,
    pub storefront: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPlaylist {
    pub id: String,
    pub name: String,
    pub track_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
}

#[derive(Debug, Clone)]
pub struct AppleMusicClient {
    developer_token: String,
    user_token: String,
    storefront: Option<String>,
    base_url: String,
    client: reqwest::Client,
}

impl AppleMusicClient {
    pub fn from_tokens(config: AppleMusicConfig) -> anyhow::Result<Self> {
        if config.developer_token.trim().is_empty() {
            bail!("developer token is required");
        }
        if config.user_token.trim().is_empty() {
            bail!("user token is required");
        }

        let storefront = config
            .storefront
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let base_url = normalize_base_url(&config.base_url);

        Ok(Self {
            developer_token: config.developer_token,
            user_token: config.user_token,
            storefront,
            base_url,
            client: reqwest::Client::new(),
        })
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let developer_token = required_env("APPLE_MUSIC_DEVELOPER_TOKEN")?;
        let user_token = required_env("APPLE_MUSIC_USER_TOKEN")?;
        let storefront = std::env::var("APPLE_MUSIC_STOREFRONT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Self::from_tokens(AppleMusicConfig {
            developer_token,
            user_token,
            storefront,
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    pub async fn library_playlists(&self) -> anyhow::Result<Vec<LibraryPlaylist>> {
        let url = format!("{}/me/library/playlists", self.base_url);
        let response = self
            .client
            .get(url)
            .headers(self.auth_headers()?)
            .send()
            .await
            .context("failed to request library playlists")?;
        let response = ensure_success(response, "library playlists").await?;
        let payload: LibraryPlaylistsResponse = response
            .json()
            .await
            .context("failed to parse library playlists response")?;

        Ok(payload
            .data
            .into_iter()
            .map(|playlist| LibraryPlaylist {
                id: playlist.id,
                name: playlist.attributes.name,
                track_count: playlist.relationships.and_then(|rels| {
                    rels.tracks
                        .and_then(|tracks| tracks.meta.map(|meta| meta.total))
                }),
            })
            .collect())
    }

    pub async fn library_playlist_tracks(
        &self,
        playlist_id: &str,
    ) -> anyhow::Result<Vec<LibraryTrack>> {
        let url = format!(
            "{}/me/library/playlists/{}?include=tracks",
            self.base_url, playlist_id
        );
        let response = self
            .client
            .get(url)
            .headers(self.auth_headers()?)
            .send()
            .await
            .context("failed to request library playlist tracks")?;
        let response = ensure_success(response, "library playlist tracks").await?;
        let payload: LibraryPlaylistTracksResponse = response
            .json()
            .await
            .context("failed to parse library playlist tracks response")?;

        let tracks = payload
            .data
            .into_iter()
            .next()
            .and_then(|playlist| playlist.relationships)
            .and_then(|rels| rels.tracks)
            .map(|tracks| {
                tracks
                    .data
                    .into_iter()
                    .map(|track| LibraryTrack {
                        id: track.id,
                        title: track.attributes.name,
                        artist: track.attributes.artist_name,
                        album: track.attributes.album_name,
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(tracks)
    }

    pub fn storefront(&self) -> Option<&str> {
        self.storefront.as_deref()
    }

    fn auth_headers(&self) -> anyhow::Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", self.developer_token))
            .context("developer token contains invalid header characters")?;
        let user_value = HeaderValue::from_str(&self.user_token)
            .context("user token contains invalid header characters")?;

        headers.insert(AUTHORIZATION, auth_value);
        headers.insert("music-user-token", user_value);
        Ok(headers)
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).with_context(|| format!("{name} is required"))?;
    if value.trim().is_empty() {
        bail!("{name} is required");
    }
    Ok(value)
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

async fn ensure_success(
    response: reqwest::Response,
    operation: &str,
) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    bail!("{operation} request failed with {status}: {body}");
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistsResponse {
    data: Vec<LibraryPlaylistItem>,
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistTracksResponse {
    data: Vec<LibraryPlaylistWithTracksItem>,
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistItem {
    id: String,
    attributes: LibraryPlaylistAttributes,
    relationships: Option<LibraryPlaylistRelationships>,
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistWithTracksItem {
    relationships: Option<LibraryPlaylistTracksRelationships>,
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistAttributes {
    name: String,
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistRelationships {
    tracks: Option<LibraryPlaylistMetaRelationship>,
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistMetaRelationship {
    meta: Option<LibraryTrackMeta>,
}

#[derive(Debug, Deserialize)]
struct LibraryTrackMeta {
    total: usize,
}

#[derive(Debug, Deserialize)]
struct LibraryPlaylistTracksRelationships {
    tracks: Option<LibraryTracksDataRelationship>,
}

#[derive(Debug, Deserialize)]
struct LibraryTracksDataRelationship {
    data: Vec<LibraryTrackItem>,
}

#[derive(Debug, Deserialize)]
struct LibraryTrackItem {
    id: String,
    attributes: LibraryTrackAttributes,
}

#[derive(Debug, Deserialize)]
struct LibraryTrackAttributes {
    name: String,
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(rename = "albumName")]
    album_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_config() -> AppleMusicConfig {
        AppleMusicConfig {
            developer_token: "dev-token".to_string(),
            user_token: "user-token".to_string(),
            storefront: None,
            base_url: "https://api.music.apple.com/v1".to_string(),
        }
    }

    #[test]
    fn from_tokens_rejects_empty_developer_token() {
        let mut config = sample_config();
        config.developer_token.clear();

        let err = AppleMusicClient::from_tokens(config).unwrap_err();

        assert!(err.to_string().contains("developer token"));
    }

    #[test]
    fn from_tokens_rejects_empty_user_token() {
        let mut config = sample_config();
        config.user_token.clear();

        let err = AppleMusicClient::from_tokens(config).unwrap_err();

        assert!(err.to_string().contains("user token"));
    }

    #[test]
    fn from_env_reads_required_tokens() {
        let _guard = test_env_lock().lock().unwrap();
        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::set_var("APPLE_MUSIC_DEVELOPER_TOKEN", "dev-token");
            std::env::set_var("APPLE_MUSIC_USER_TOKEN", "user-token");
            std::env::remove_var("APPLE_MUSIC_STOREFRONT");
        }

        let result = AppleMusicClient::from_env();

        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::remove_var("APPLE_MUSIC_DEVELOPER_TOKEN");
            std::env::remove_var("APPLE_MUSIC_USER_TOKEN");
            std::env::remove_var("APPLE_MUSIC_STOREFRONT");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn from_env_rejects_missing_developer_token() {
        let _guard = test_env_lock().lock().unwrap();
        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::remove_var("APPLE_MUSIC_DEVELOPER_TOKEN");
            std::env::set_var("APPLE_MUSIC_USER_TOKEN", "user-token");
        }

        let err = AppleMusicClient::from_env().unwrap_err();

        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::remove_var("APPLE_MUSIC_DEVELOPER_TOKEN");
            std::env::remove_var("APPLE_MUSIC_USER_TOKEN");
        }

        assert!(err.to_string().contains("APPLE_MUSIC_DEVELOPER_TOKEN"));
    }

    #[test]
    fn from_env_rejects_missing_user_token() {
        let _guard = test_env_lock().lock().unwrap();
        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::set_var("APPLE_MUSIC_DEVELOPER_TOKEN", "dev-token");
            std::env::remove_var("APPLE_MUSIC_USER_TOKEN");
        }

        let err = AppleMusicClient::from_env().unwrap_err();

        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::remove_var("APPLE_MUSIC_DEVELOPER_TOKEN");
            std::env::remove_var("APPLE_MUSIC_USER_TOKEN");
        }

        assert!(err.to_string().contains("APPLE_MUSIC_USER_TOKEN"));
    }

    #[test]
    fn from_env_uses_optional_storefront() {
        let _guard = test_env_lock().lock().unwrap();
        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::set_var("APPLE_MUSIC_DEVELOPER_TOKEN", "dev-token");
            std::env::set_var("APPLE_MUSIC_USER_TOKEN", "user-token");
            std::env::set_var("APPLE_MUSIC_STOREFRONT", "gb");
        }

        let client = AppleMusicClient::from_env().unwrap();

        // SAFETY: these tests serialize process environment mutation behind `test_env_lock`.
        unsafe {
            std::env::remove_var("APPLE_MUSIC_DEVELOPER_TOKEN");
            std::env::remove_var("APPLE_MUSIC_USER_TOKEN");
            std::env::remove_var("APPLE_MUSIC_STOREFRONT");
        }

        assert_eq!(client.storefront(), Some("gb"));
    }

    #[tokio::test]
    async fn library_playlists_sends_expected_headers_and_parses_fixture_response() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/apple_music/library_playlists.json"
        ));
        let (base_url, request_rx) = spawn_single_response_server(fixture).await;
        let client = AppleMusicClient::from_tokens(AppleMusicConfig {
            developer_token: "dev-token".to_string(),
            user_token: "user-token".to_string(),
            storefront: None,
            base_url,
        })
        .unwrap();

        let playlists = client.library_playlists().await.unwrap();
        let request = request_rx.await.unwrap();

        assert!(request.starts_with("GET /v1/me/library/playlists HTTP/1.1\r\n"));
        assert!(request.contains("authorization: Bearer dev-token\r\n"));
        assert!(request.contains("music-user-token: user-token\r\n"));
        assert_eq!(
            playlists,
            vec![
                LibraryPlaylist {
                    id: "p.playlist-1".to_string(),
                    name: "Chill Mix".to_string(),
                    track_count: Some(12),
                },
                LibraryPlaylist {
                    id: "p.playlist-2".to_string(),
                    name: "Focus Mix".to_string(),
                    track_count: Some(5),
                },
            ]
        );
    }

    #[tokio::test]
    async fn library_playlist_tracks_requests_playlist_and_parses_tracks() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/apple_music/library_playlist_tracks.json"
        ));
        let (base_url, request_rx) = spawn_single_response_server(fixture).await;
        let client = AppleMusicClient::from_tokens(AppleMusicConfig {
            developer_token: "dev-token".to_string(),
            user_token: "user-token".to_string(),
            storefront: None,
            base_url,
        })
        .unwrap();

        let tracks = client
            .library_playlist_tracks("p.playlist-1")
            .await
            .unwrap();
        let request = request_rx.await.unwrap();

        assert!(
            request.starts_with(
                "GET /v1/me/library/playlists/p.playlist-1?include=tracks HTTP/1.1\r\n"
            )
        );
        assert!(request.contains("authorization: Bearer dev-token\r\n"));
        assert!(request.contains("music-user-token: user-token\r\n"));
        assert_eq!(
            tracks,
            vec![
                LibraryTrack {
                    id: "t.track-1".to_string(),
                    title: "Around the World".to_string(),
                    artist: "Daft Punk".to_string(),
                    album: "Homework".to_string(),
                },
                LibraryTrack {
                    id: "t.track-2".to_string(),
                    title: "Digital Love".to_string(),
                    artist: "Daft Punk".to_string(),
                    album: "Discovery".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn library_playlists_returns_error_on_non_success_status() {
        let (base_url, _request_rx) =
            spawn_response_server("HTTP/1.1 500 Internal Server Error", "boom").await;
        let client = AppleMusicClient::from_tokens(AppleMusicConfig {
            developer_token: "dev-token".to_string(),
            user_token: "user-token".to_string(),
            storefront: None,
            base_url,
        })
        .unwrap();

        let err = client.library_playlists().await.unwrap_err();

        assert!(err.to_string().contains("library playlists"));
    }

    #[tokio::test]
    async fn library_playlist_tracks_returns_error_on_invalid_json() {
        let (base_url, _request_rx) = spawn_single_response_server("{not-json").await;
        let client = AppleMusicClient::from_tokens(AppleMusicConfig {
            developer_token: "dev-token".to_string(),
            user_token: "user-token".to_string(),
            storefront: None,
            base_url,
        })
        .unwrap();

        let err = client
            .library_playlist_tracks("p.playlist-1")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("library playlist tracks"));
    }

    #[tokio::test]
    #[ignore = "requires APPLE_MUSIC_DEVELOPER_TOKEN and APPLE_MUSIC_USER_TOKEN"]
    async fn live_library_playlists_smoke_test() {
        if std::env::var("APPLE_MUSIC_DEVELOPER_TOKEN").is_err()
            || std::env::var("APPLE_MUSIC_USER_TOKEN").is_err()
        {
            return;
        }

        let client = AppleMusicClient::from_env().unwrap();
        let playlists = client.library_playlists().await.unwrap();

        for playlist in playlists {
            assert!(!playlist.id.is_empty());
            assert!(!playlist.name.is_empty());
        }
    }

    async fn spawn_single_response_server(
        body: &'static str,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        spawn_response_server("HTTP/1.1 200 OK", body).await
    }

    async fn spawn_response_server(
        status: &'static str,
        body: &'static str,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio::sync::oneshot;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let read = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..read]).into_owned();
            let _ = request_tx.send(request);

            let response = format!(
                "{status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });

        (format!("http://{addr}/v1"), request_rx)
    }
}
