---
status: accepted
date: 2026-03-01
decision-makers:
---

# Provider trait for external music services

## Context and Problem Statement

The Go version integrates with Navidrome (a self-hosted music server implementing the Subsonic API). We need to support browsable remote playlists in the Rust port, with the architecture open to additional providers (Subsonic-compatible servers, potentially others) without coupling the UI or playlist logic to any specific service.

## Decision

Define a `Provider` trait in the playlist module:

```rust
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn playlists(&self) -> anyhow::Result<Vec<PlaylistInfo>>;
    fn tracks(&self, playlist_id: &str) -> anyhow::Result<Vec<Track>>;
}
```

Key design choices:

1. **Trait in `playlist` module, implementation in `external`**: The `Provider` trait and `PlaylistInfo` struct live in `playlist/mod.rs` alongside `Track` and `Playlist`. Concrete implementations (e.g., `NavidromeClient`) live in `external/navidrome.rs`. This keeps the dependency direction clean: UI depends on the trait, not the implementation.

2. **Blocking API with `spawn_blocking`**: Provider methods are synchronous (blocking HTTP). The UI dispatches calls via `tokio::task::spawn_blocking` and receives results through `AppMsg` variants (`ProviderPlaylists`, `ProviderTracks`, `ProviderError`). This matches the existing pattern used for yt-dlp resolution and HTTP stream playback.

3. **`Arc<dyn Provider>` in App**: The provider is stored as `Option<Arc<dyn Provider>>` so it can be cloned into spawned tasks. Constructed from `Box<dyn Provider>` passed by `main()`.

4. **`Focus::Provider` UI state**: A new focus variant shows the provider playlist browser in place of the track playlist. Navigation: Enter loads a playlist's tracks into the main playlist, Esc/Backspace returns from playlist to provider view, Tab cycles through Playlist/EQ/Provider.

5. **Environment-based configuration**: `NavidromeClient::from_env()` reads `NAVIDROME_URL`, `NAVIDROME_USER`, `NAVIDROME_PASS`. Returns `None` if any are unset. When a provider is available and no CLI args are given, the app starts in provider focus instead of showing the usage message.

6. **Subsonic API auth**: Per-request salt (nanosecond timestamp) + MD5(password + salt) token. Matches the Go implementation and Subsonic API spec.

## Consequences

* Good, because new providers can be added by implementing the `Provider` trait without touching UI or playlist code
* Good, because the trait is minimal (3 methods) — easy to implement for any service that has playlists and tracks
* Good, because tracks from providers are standard `Track` structs with `stream: true`, so they flow through the existing HTTP streaming pipeline
* Bad, because blocking provider calls hold a thread pool thread — fine for a CLI tool, but wouldn't scale to many concurrent providers
* Neutral, because only Navidrome/Subsonic is implemented; the trait's value is proven when a second provider is added

## Alternatives Considered

* **No trait — hardcode Navidrome**: Simpler, but couples the UI to Navidrome specifics and makes adding providers require touching many files.
* **Async trait (`async fn`)**: Would avoid blocking threads, but the rest of the codebase uses `spawn_blocking` for I/O-heavy operations, and async traits add complexity (boxing, lifetime issues) for no practical benefit here.
* **Config file for credentials**: More flexible than env vars, but the Go version uses env vars and it matches twelve-factor app conventions. Config file support can be added later without changing the trait.
