# Apple Music Capability Status Design

**Status:** Approved design
**Related ADR:** [ADR-0020](../../../adr/0020-reopen-apple-music-api-for-tui-rich-music-app-integration.md)

## Goal

Make the first revived Apple Music TUI slice honest and observable: users can tell whether Apple Music is unavailable because credentials are missing, loading, empty, loaded, or failing, while playlist and track browsing remain read-only and separate from local playback.

## Scope

This slice covers:

- a typed Apple Music capability/status model;
- state transitions for startup, playlist loading, track loading, success, empty results, and API errors;
- fixture coverage for status behavior and existing playlist/track response parsing;
- ignored, environment-gated live tests for library playlists and playlist tracks;
- equivalent semantic rendering in the standard and 808 views;
- README documentation for `APPLE_MUSIC_DEVELOPER_TOKEN`, `APPLE_MUSIC_USER_TOKEN`, and optional `APPLE_MUSIC_STOREFRONT`.

This slice explicitly does not implement catalog search, playlist writes, private-key signing, end-user token acquisition, or Music.app playback handoff. Selecting an Apple Music track continues to show an explicit handoff-not-implemented message.

## Design

### State boundary

Add a small `AppleMusicStatus` model in `src/ui/mod.rs`, owned by the TUI application state. It represents the user-visible capability of the Apple Music metadata browser without changing the generic provider or playback model.

The model must represent these states:

- `Unavailable`: required developer or user token is absent;
- `ClientError`: token/configuration construction failed;
- `Loading`: an Apple Music request is in flight;
- `Loaded`: a request succeeded and returned one or more items;
- `Empty`: a request succeeded with no items;
- `ApiError`: a request failed after the client was constructed.

The state carries a concise display-safe message where appropriate. Raw credentials, private keys, request headers, and full response bodies must never enter TUI state.

### Existing client boundary

Keep `AppleMusicClient` and Apple Music DTOs in `src/external/apple_music_api.rs`. The first slice may add narrow helpers needed to classify configuration and preserve fixture/live testing, but it must not move Apple Music data into `src/player/` or fabricate playable URLs.

Startup in `src/main.rs` continues to construct the client only for `--backend music-app`. Missing tokens remain non-fatal: the application launches and exposes `Unavailable` in the Apple Music browser area.

### Async transitions

Before a playlist or track request starts, the app sets `Loading`. A successful non-empty response sets `Loaded`; a successful empty response sets `Empty`; a failed request sets `ApiError`. A client construction failure sets `ClientError` and a missing-token configuration sets `Unavailable`.

Playlist and track collections remain in their existing Apple-specific fields. They must not be copied into the normal local queue or passed to the local playback backend.

### Rendering

`src/ui/view.rs` and `src/ui/view_808.rs` render the same semantic states with their existing visual language:

- unavailable: show the required environment variables and that setup is optional;
- client error/API error: show a concise error and preserve navigation;
- loading: show a stable loading label;
- empty: show that the library or playlist contains no visible items;
- loaded: render the existing playlist or track rows.

The 808 view may style these states with its canonical colors and panels, but wording and state meaning must remain equivalent to the standard view.

### Interaction

Existing playlist and track navigation remains intact. Enter on an Apple Music track must continue to set `Apple Music playback handoff not implemented yet` or an equivalent explicit message. No Apple Music selection may invoke local playback.

## Testing

Use test-first development:

1. Add failing unit tests for each status transition and missing-token classification.
2. Add or retain fixture tests for `library_playlists` and `library_playlist_tracks` parsing.
3. Add ignored live tests gated by both required token variables for playlist and playlist-track requests.
4. Add UI-state tests for unavailable, loading, empty, loaded, and error outcomes where existing app-message tests provide the appropriate seam.
5. Verify both views compile and render their Apple Music branches through existing testable helpers or focused snapshot-like buffer assertions where available.

Required verification commands are:

```bash
cargo fmt --all --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Live tests must remain ignored and must not run as part of the normal `cargo test` command.

## Documentation

After the live API tests are validated, add a README section showing developer-only setup:

```bash
export APPLE_MUSIC_DEVELOPER_TOKEN="..."
export APPLE_MUSIC_USER_TOKEN="..."
export APPLE_MUSIC_STOREFRONT="gb"
cargo run -- --backend music-app
```

The documentation must state that the developer token belongs to the app/operator, the Music User Token belongs to the authorized Apple Music user, and no end user needs an Apple Developer Program account for the eventual product flow. It must also state that this first slice uses manually supplied environment variables and does not implement user authorization.

## Acceptance criteria

- Starting `--backend music-app` without tokens remains non-fatal and visibly reports unavailable Apple Music metadata access.
- Client construction failures, request failures, empty results, loading, and loaded results are distinguishable in both views.
- Existing fixture parsing and ignored live tests cover library playlists and playlist tracks.
- Apple Music browser state remains separate from the local playback queue.
- Enter on an Apple Music track does not start local playback.
- No Apple private key or token value is written to source, config, logs, or TUI state.
- Formatting, tests, and clippy pass.

