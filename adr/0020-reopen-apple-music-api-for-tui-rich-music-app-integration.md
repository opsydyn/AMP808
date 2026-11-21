---
status: accepted
date: 2026-06-24
decision-makers: alan
---

# Reopen Apple Music API for TUI-rich Music.app integration

## Context and Problem Statement

ADR-0012 established `Music.app` as AMP808's supported Apple Music playback surface on macOS.
That decision remains correct: Apple Music playback should be controlled through the system
`Music.app`, not decoded or streamed through AMP808's local Rust player.

ADR-0013 then introduced an Apple Music API metadata client for read-only library browsing, but
ADR-0014 paused productized Apple Music API work because the Apple Developer Program cost and token
delivery story were not justified for that product phase. The project now has Apple Developer
Program access, so the cost and capability premise behind that pause has changed.

The remaining technical boundary has not changed: neither the Apple Music API nor `Music.app`
automation gives AMP808 direct PCM/sample access for true waveform, FFT, EQ, or BPM analysis of
Apple Music playback. The TUI can still become much richer by combining:

- Apple Music API metadata, catalog, library, recommendation, and playlist operations
- `Music.app` automation for playback, state, and local library handoff
- synthetic, transport-linked visualizers that remain honest about not being sample-derived

This ADR reopens Apple Music API work for the native/TUI experience only. AMP808 Web remains
governed by ADR-0017 through ADR-0019 and is not part of this decision.

## Decision Drivers

- TUI is the primary product surface for this integration.
- The project now has Apple Developer Program access.
- `Music.app` is already implemented and accepted as the Apple Music playback backend.
- The existing Apple Music API client and TUI browser scaffold can be revived instead of replaced.
- Apple Music functionality must not contaminate the local file/HTTP/SoundCloud playback pipeline.
- Users should see a rich, capable Apple Music mode without false claims about audio analysis.
- Token handling needs a clear boundary before any write or playlist-management features are
  productized.

## Decision Outcome

Chosen option: reopen Apple Music API integration as a dedicated TUI metadata and discovery layer
paired with `Music.app` playback and handoff.

1. Keep `Music.app` as the only supported Apple Music playback backend in AMP808 native/TUI.
2. Reopen Apple Music API work for native/TUI user-visible features:
   - library playlist browsing
   - library playlist track browsing
   - catalog search
   - recently played and recommendation views
   - rich now-playing metadata where the API can map to the current item
   - future read/write library operations after read-only behavior is proven
3. Keep Apple Music API types separate from the normal `Track` and `Provider` playback model until
   a concrete handoff rule is proven.
4. Introduce an Apple-specific service boundary, conceptually `AppleMusicService`, that coordinates:
   - Apple Music API metadata calls
   - optional token/config loading
   - conversion to TUI browser view models
   - future `Music.app` handoff attempts
5. Treat the existing `src/external/apple_music_api.rs` client as the starting point, not as a final
   product boundary.
6. Keep `src/player/*` free of Apple Music API dependencies, credentials, and DTOs.
7. Keep Apple Music visualizers synthetic and transport-linked in `music-app` mode.
8. Do not claim true waveform, FFT, BPM, EQ, or analyser behavior for Apple Music playback.
9. Do not include AMP808 Web MusicKit playback in this ADR. Any MusicKit Web experience requires a
   separate ADR because it changes the browser/provider boundary from ADR-0019.

This ADR supersedes the Apple Music API product pause in ADR-0014. It does not supersede ADR-0014's
honesty boundary for synthetic visualizers, and it does not supersede ADR-0012's `Music.app`
playback decision.

## Supported Product Shape

The target TUI experience is an Apple Music command center inside `--backend music-app`:

- A provider/browser pane for Apple Music library playlists and playlist tracks.
- A catalog search path that can browse songs, albums, artists, playlists, stations, and charts.
- Discovery views for recently played, recommendations, heavy rotation, or Replay-style summaries
  where the Apple Music API exposes them.
- A selected-item readout with title, artist, album, duration, source kind, and identifiers useful
  for debugging handoff.
- Clear action states:
  - available
  - unauthenticated
  - token invalid or expired
  - API unavailable
  - handoff unavailable
- Later, after read-only behavior is proven, library actions such as favorite, add to library,
  create playlist, or add selected tracks to a playlist may be added behind explicit tests and UI
  confirmation.

## Token and Authorization Boundary

The first revived slices use explicit developer/operator-provided credentials:

- `APPLE_MUSIC_DEVELOPER_TOKEN`
- `APPLE_MUSIC_USER_TOKEN`
- optional `APPLE_MUSIC_STOREFRONT`

AMP808 must not embed Apple private keys, generate developer tokens from a private key, or run a
token-issuing service in the first revived slices.

A later ADR may introduce a local auth helper or token-issuer architecture. That decision must
address private key storage, developer token lifetime, Music User Token acquisition, secure local
storage, and failure behavior.

## Non-goals

- No Apple Music audio playback through AMP808's local Rust decode pipeline.
- No direct Apple Music stream URLs exposed to `src/player`.
- No true sample-derived waveform, FFT, EQ, or BPM for Apple Music playback.
- No attempt to capture PCM output from `Music.app`.
- No MusicKit Web or Ratzilla web playback changes in this ADR.
- No private-key signing, developer-token issuer, or end-user auth setup in the first revived
  implementation slice.
- No playlist writes, favorites, ratings, or library mutations until read-only browsing, search, and
  error states are proven.
- No forced merge of Apple Music API data into the generic `Provider` trait or normal `Track`
  playlist model until playback handoff has a tested rule.

## Consequences

- Good, because Apple Developer Program access can now produce visible TUI value.
- Good, because the accepted `Music.app` playback boundary remains intact.
- Good, because the dormant Apple Music API scaffold can be used instead of rebuilt.
- Good, because Apple Music browsing can become richer than the current remote-control mode without
  claiming unsupported audio-analysis capabilities.
- Good, because keeping Apple DTOs separate avoids corrupting the existing local/provider playback
  model.
- Bad, because token setup remains manual until a separate auth/token ADR is approved.
- Bad, because track handoff from API metadata to `Music.app` may be unreliable until proven against
  real library/catalog items.
- Bad, because the TUI will still have two categories of visuals: real analyser visuals for local
  playback and synthetic visuals for Apple Music playback.
- Neutral, because Apple Music write operations remain intentionally delayed.

## Implementation Plan

- **Affected paths**:
  - `adr/0014-pause-apple-music-api-rollout-and-use-synthetic-visualizers.md`: add a future note
    that this ADR supersedes only the Apple API product pause.
  - `README.md`: document the revived Apple Music API setup only after the first implementation
    slice lands.
  - `src/external/apple_music_api.rs`: extend the current client with read-only catalog/search and
    richer DTOs.
  - `src/external/music_app.rs`: add tested handoff helpers only after a deterministic script is
    proven.
  - `src/main.rs`: keep Apple Music client construction limited to `--backend music-app`.
  - `src/ui/mod.rs`: keep Apple Music state separate from normal provider and playlist state.
  - `src/ui/keys.rs`: route Apple Music browser actions without invoking local playback.
  - `src/ui/view.rs`: render standard-view Apple Music browser states.
  - `src/ui/view_808.rs`: render 808-view Apple Music browser states.
  - `tests/fixtures/apple_music/*`: add fixture responses for every new API shape.
- **Patterns to follow**:
  - Keep Apple Music API code in `src/external/` or a narrow service module, not `src/player/`.
  - Keep all live Apple API tests ignored and env-gated.
  - Use fixture tests for DTO parsing, API error parsing, view-model mapping, and UI state changes.
  - Keep `music-app` startup graceful when Apple Music tokens are absent.
  - Treat unauthenticated, invalid token, API failure, empty library, and loaded states distinctly.
  - Keep UI copy explicit when an operation is metadata-only or handoff-only.
  - Prove handoff in isolation before making Enter on a track play anything.
- **Patterns to avoid**:
  - Do not fabricate playable URLs from Apple Music API responses.
  - Do not pass Apple Music catalog/library items into `PlaybackBackend::Local`.
  - Do not hide failed handoff behind local search or synthetic state.
  - Do not add Apple private-key material to config files or source control.
  - Do not add playlist writes before read-only browsing and search have live validation.

### First Implementation Slice

1. Add a small Apple Music capability/status model for the TUI:
   - unavailable because no tokens
   - token/client construction error
   - loading
   - loaded
   - API error
2. Add fixture tests for the status model and existing playlist/track parsing.
3. Add or refresh ignored live tests for:
   - `library_playlists`
   - `library_playlist_tracks`
4. Improve provider-pane empty/error rendering in standard and 808 views.
5. Document the developer-only env-token setup after live validation succeeds.
6. Leave track selection as a clear handoff-not-implemented state.

### Follow-up Slices

1. Add Apple Music catalog search DTOs, client calls, fixtures, and TUI rendering.
2. Add a selected Apple item readout with stable IDs, source kind, and handoff eligibility.
3. Explore `Music.app` handoff:
   - play an existing local/library track by persistent ID or database ID when available
   - search the Music library for a matching item
   - open Apple Music URLs only when that behavior is deterministic and acceptable
4. Add read-only discovery views such as recently played and recommendations.
5. Revisit playlist/library write operations in a separate slice once read-only UX is stable.
6. If token management should become productized for non-developer users, create a new ADR for the
   auth/token strategy before implementing it.

## Verification

- [ ] `cargo test` covers Apple Music DTO parsing and TUI state transitions.
- [ ] Ignored live tests for Apple Music API playlist and playlist-track loading are env-gated by
      `APPLE_MUSIC_DEVELOPER_TOKEN` and `APPLE_MUSIC_USER_TOKEN`.
- [ ] `cargo run -- --backend music-app` still starts when Apple Music env vars are absent.
- [ ] Missing-token state is visible and non-fatal in both standard and 808 views.
- [ ] Invalid-token/API-error state is visible and non-fatal in both standard and 808 views.
- [ ] Apple Music playlist and track browser state remains separate from the normal local playlist
      queue.
- [ ] Enter on an Apple Music track does not call local playback until a later handoff slice proves
      and tests the behavior.
- [ ] `src/player/*` does not import Apple Music API types or credentials.
- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.

## Alternatives Considered

### Keep ADR-0014's Apple API pause

This is safest operationally, but it no longer matches the project's capabilities now that Apple
Developer Program access exists. It would leave the TUI stuck at remote-control parity while the
existing API scaffold remains unused.

### Force Apple Music into the generic Provider and Track model

This would make the UI look uniform, but it would be premature. The existing `Provider` contract
assumes provider tracks become regular playback `Track` values. Apple Music items do not fit that
model until handoff behavior is explicit and tested.

### Add a token issuer or private-key signing now

This would make setup smoother, but it adds security, storage, and operational complexity before
the read-only UX and handoff value have been proven.

### Build MusicKit Web first

MusicKit Web may be useful later for AMP808 Web, but the user-facing focus for this decision is the
native/TUI app. Web MusicKit would also interact with ADR-0019's static browser/provider boundary
and needs its own decision.

## More Information

Related existing decisions:

- ADR-0007: Provider trait for external music services
- ADR-0012: Add a macOS Music.app playback backend
- ADR-0013: Use the Apple Music API as a metadata-only integration
- ADR-0014: Pause Apple Music API rollout and use synthetic visualizers for Music.app parity
- ADR-0019: Define the web external provider and ffmpeg.wasm boundary

External references checked 2026-06-24:

- <https://developer.apple.com/musickit/>
- <https://developer.apple.com/documentation/applemusicapi/>
- <https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens>
- <https://developer.apple.com/documentation/applemusicapi/user-authentication-for-musickit>
- <https://developer.apple.com/documentation/applemusicapi/get-all-library-songs>
- <https://developer.apple.com/documentation/applemusicapi/add-tracks-to-a-library-playlist>

Local reference checked 2026-06-24:

- macOS `Music.app` scripting dictionary via `sdef /System/Applications/Music.app`
