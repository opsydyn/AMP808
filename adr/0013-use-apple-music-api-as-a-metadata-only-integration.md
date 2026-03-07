---
status: superseded
date: 2026-03-07
decision-makers: alan
---

# Use the Apple Music API as a metadata-only integration

## Context and Problem Statement

If `cliamp` grows beyond local files, HTTP streams, and Subsonic-style providers, Apple Music is an
obvious service to consider. Apple exposes Apple Music catalog, library, and playlist APIs through
MusicKit / Apple Music API, but those APIs are not a clean substitute for the existing local player.
They expose metadata and library operations, not a drop-in audio stream source for the current Rust
playback engine.

This repo already has a useful separation for remote browsing via the `Provider` trait in
`src/playlist/mod.rs` (ADR-0007), but that trait currently assumes that provider tracks become
ordinary `Track` values that can be loaded into the local playback pipeline. Apple Music does not
fit that assumption cleanly.

We want to keep the first Apple Music API integration simple:

- metadata and library operations only
- explicit token-based configuration
- no attempt to stream Apple Music audio through `cliamp`
- no premature rewrite of the existing provider and track models

This ADR therefore separates Apple Music API integration from playback. Playback is handled, if at
all, by ADR-0012's `Music.app` backend. The Apple Music API is for library/catalog metadata.

## Decision

1. Treat Apple Music API integration as a separate metadata client, not as part of the local player.
2. Scope the first Apple Music API integration to read-only library metadata:
   - list library playlists
   - fetch tracks for a library playlist
   - read basic catalog/library metadata needed by the UI
3. Phase 1 is an internal, non-user-visible integration slice.
   - no normal `cliamp` startup path initializes the Apple Music API client in phase 1
   - no TUI mode, provider mode, or CLI flag is added in phase 1
   - the client is constructed only by explicit code paths such as tests or future follow-up slices
4. Require explicit user-provided credentials/configuration when the client is constructed:
   - `APPLE_MUSIC_DEVELOPER_TOKEN`
   - `APPLE_MUSIC_USER_TOKEN`
   - optional `APPLE_MUSIC_STOREFRONT`
   - no config-file fallback in phase 1
5. Do not generate developer tokens inside `cliamp` in v1.
6. Do not force Apple Music API types through the existing `Track` struct in v1.
7. Add separate Apple Music DTOs for playlist and track metadata in the client layer until there is a
   concrete playback or handoff path.
8. Phase 2 makes Apple Music metadata user-visible only inside `--backend music-app`.
   - normal local playback startup remains unchanged
   - if Apple Music tokens are absent or invalid, `music-app` transport mode still starts and the
     metadata browser remains unavailable
9. The first user-visible Apple Music UI is a read-only browser in the existing provider pane:
   - library playlists list first
   - Enter on a playlist fetches its tracks
   - Esc from the track list returns to the playlist list
   - track selection does not trigger playback in this slice
10. Apple Music metadata exposure must not force Apple Music items through the existing `Track` model
    until playback handoff is explicitly implemented.
11. In phase 2, selecting an Apple Music track in the browser shows a clear “playback handoff not
    implemented yet” message rather than failing through the local playback path.
12. Any future transition from read-only metadata browser to playback handoff requires another ADR
    update defining:
    - the selected-track handoff behavior into ADR-0012's `Music.app` backend
    - how Apple Music metadata maps into UI playlist state, if at all
    - failure behavior when the selected metadata item cannot be resolved in `Music.app`

## Non-goals

- No Apple Music audio playback through `cliamp`.
- No automatic MusicKit developer token creation or private-key signing in v1.
- No write operations such as playlist creation, playlist modification, ratings, likes, or library mutation in v1.
- No reuse of the current `Provider` trait until we have a concrete playback/handoff design.
- No cross-service abstraction that merges Apple Music and Subsonic data models prematurely.
- No local-backend startup integration for Apple Music metadata.
- No Apple Music track playback handoff in phase 2.
- No Apple Music metadata mapped into the normal `Playlist`/`Track` playback queue in phase 2.
- No environment-driven auto-enable behavior in phase 1.

## Consequences

- Good, because Apple Music API complexity stays out of the audio playback path.
- Good, because explicit token input is simpler and safer than embedding JWT signing logic in the terminal app.
- Good, because separate DTOs avoid corrupting the current `Track` model with Apple-specific assumptions too early.
- Good, because this keeps sequencing clear: metadata first, playback handoff only when ADR-0012 or equivalent exists.
- Good, because phase 1 can be implemented and tested without changing normal startup, TUI flow, or playback behavior.
- Good, because phase 2 gives the user visible Apple Music value without breaking the transport/playback boundary.
- Good, because missing Apple Music tokens degrade gracefully to transport-only `music-app` mode.
- Bad, because Apple Music metadata will not immediately fit into the current `Provider` UI.
- Bad, because user setup is more manual: developer token and music user token must already exist.
- Bad, because phase 2 introduces a second remote-browsing path in the UI that is adjacent to, but not identical to, the accepted `Provider` flow from ADR-0007.
- Neutral, because this ADR intentionally delays write operations and broad catalog features.

## Implementation Plan

- **Affected paths**:
  - `src/external/mod.rs` (export `apple_music_api` module)
  - `src/external/apple_music_api.rs` (HTTP client, request builders, response DTOs)
  - `src/main.rs` (construct optional Apple Music client only for `--backend music-app`)
  - `src/ui/mod.rs` (App state for Apple Music browser, async fetch + message handling)
  - `src/ui/keys.rs` (provider-pane key routing for read-only Apple Music browsing)
  - `src/ui/view.rs` (render Apple Music playlists/tracks in provider pane)
  - `src/ui/view_808.rs` (render Apple Music playlists/tracks in provider pane)
  - tests in `src/ui/*` and/or `src/external/apple_music_api.rs` for message flow and rendering states
  - `README.md` only if the token-driven `music-app` metadata browser needs runtime documentation
  - `src/player/*` and `src/playlist/mod.rs` remain unchanged in phase 2
- **Patterns to follow**:
  - Keep Apple Music API code in a dedicated client module.
  - Provide a narrow constructor split such as `from_tokens(...)` and `from_env()` so token handling stays explicit and testable.
  - Use explicit request/response DTOs instead of reusing `Track` too early.
  - Keep the first slice read-only and library-focused.
  - Fail fast on missing tokens with clear construction errors.
  - Use fixture-based tests for response parsing and make any live Apple API smoke tests ignored and env-gated by default.
  - Reuse the existing provider-pane layout and focus model where it fits, but keep Apple Music state separate from `Provider` and `Track`.
  - Degrade gracefully: if Apple Music tokens are not present, `music-app` mode still works as a transport-only backend.
- **Patterns to avoid**:
  - Do not fabricate playable URLs from Apple Music API responses.
  - Do not add Apple Music-specific branches inside `src/player/`.
  - Do not extend `Provider` or `Track` until the playback/handoff story is explicit.
  - Do not implement write endpoints before read-only flows are proven.
  - Do not auto-start playback when browsing Apple Music metadata.
  - Do not route Apple Music track selection into the local `Playlist` in phase 2.

### Verification

- [x] Constructing the client without required tokens returns a clear error from the client constructor.
- [x] Read-only client methods for library playlists and playlist tracks have fixture-based parsing coverage.
- [x] Any live Apple Music API smoke test is ignored by default and only runs when tokens are explicitly present.
- [x] No code in `src/player/` depends on Apple Music API types or tokens.
- [x] `Track` and `Provider` remain unchanged in the first Apple Music API slice.
- [x] Phase 1 adds no `src/ui/*` or `src/main.rs` coupling to Apple Music API types.
- [ ] In `--backend music-app`, Apple Music library playlists are shown in the existing provider pane when valid tokens are present.
- [ ] Enter on an Apple Music playlist fetches and renders its tracks in the same pane.
- [x] Esc from Apple Music track view returns to Apple Music playlist view.
- [x] Enter on an Apple Music track does not call the local playback path and instead shows a clear “playback handoff not implemented yet” message.
- [x] Starting `cliamp --backend music-app` without Apple Music tokens still launches transport-only mode without crashing.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [x] `cargo test` passes.

## Alternatives Considered

- **Force Apple Music API into the existing `Provider` trait now**: tempting, but incorrect until we know how Apple Music items map to playback/handoff.
- **Add Apple Music API write support immediately**: useful later, but too much surface area before read-only metadata is proven.
- **Generate developer tokens inside `cliamp`**: more self-contained, but increases key-management risk and implementation complexity.
- **Ignore Apple Music API and rely only on `Music.app` scripting**: simplest for playback, but gives up catalog and library metadata capabilities.

## More Information

Related existing decisions:

- ADR-0004: Tokio async with dedicated cpal audio thread
- ADR-0007: Provider trait for external music services
- ADR-0012: Add a macOS Music.app playback backend

External references checked 2026-03-07:

- <https://developer.apple.com/musickit/>
- <https://developer.apple.com/help/account/services/musickit>
- <https://developer.apple.com/help/account/capabilities/create-a-media-identifier-and-private-key/>
- <https://developer.apple.com/documentation/applemusicapi/get-a-library-playlist>
- <https://developer.apple.com/documentation/applemusicapi/libraryplaylistcreationrequest>

ADR update 2026-03-07:

- Phase 2 now allows a user-visible, read-only Apple Music metadata browser only in `--backend music-app`.
- This browser is intentionally separate from the accepted `Provider -> Track` playback pipeline in ADR-0007.
- Playback handoff remains out of scope for this ADR version.
- First slice implemented in:
  - `src/main.rs`
  - `src/ui/mod.rs`
  - `src/ui/keys.rs`
  - `src/ui/view.rs`
  - `src/ui/view_808.rs`
- Verified in code:
  - `ui::mod::tests::apple_music_tracks_message_enters_track_browser`
  - `ui::mod::tests::apple_music_playlists_message_resets_track_browser`
  - `ui::keys::tests::escape_from_apple_music_track_view_returns_to_playlist_list`
  - `ui::keys::tests::enter_on_apple_music_track_shows_handoff_message`
- Verified manually:
  - `env -u APPLE_MUSIC_DEVELOPER_TOKEN -u APPLE_MUSIC_USER_TOKEN -u APPLE_MUSIC_STOREFRONT target/debug/cliamp --backend music-app`
  - the TUI still starts in transport-only `music-app` mode when Apple Music API tokens are absent
- Still intentionally open:
  - live validation of Apple Music playlist loading with valid tokens
  - live validation that Enter on a playlist fetches tracks from the Apple Music API and renders them in-pane

Superseded 2026-03-07:

- Superseded by ADR-0014 for supported product direction.
- The internal Apple Music API client may remain in the codebase, but user-visible/productized Apple Music metadata work is paused until the economics and token-delivery story justify revisiting it.
