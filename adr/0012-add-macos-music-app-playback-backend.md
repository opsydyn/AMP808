---
status: accepted
date: 2026-03-07
decision-makers: alan
---

# Add a macOS Music.app playback backend

## Context and Problem Statement

`cliamp` currently owns playback through the local `Player` engine in `src/player/mod.rs`, built on
`cpal`, `symphonia`, gapless buffering, local EQ, and waveform tapping. That architecture is correct
for local files and HTTP streams (ADR-0002, ADR-0003, ADR-0004), but it is the wrong boundary for
Apple Music playback on macOS.

Apple Music playback is already implemented by the system `Music.app`, which is scriptable on macOS.
Trying to pull Apple Music playback into the existing Rust decode pipeline would conflict with the
current architecture and create unnecessary risk around playback support, token handling, and media
capabilities.

At the same time, the current UI depends directly on many `Player` methods across `src/ui/mod.rs`,
`src/ui/keys.rs`, `src/ui/view.rs`, and `src/ui/view_808.rs`. We need a minimal way to support an
Apple Music-backed playback mode without rewriting the UI around a large new abstraction.

The project direction here is KISS and brilliant basics:

- add Apple Music support first as playback control on macOS
- keep the existing local player unchanged for local files and streams
- avoid introducing Apple Music API complexity into the playback path
- keep unsupported features visibly disabled rather than half-implemented

## Decision

1. Add a macOS-only `Music.app` playback backend implemented via AppleScript/`osascript` in
   `src/external/music_app.rs`.
2. Introduce a small `PlaybackBackend` enum wrapper rather than a new trait hierarchy.
   - `PlaybackBackend::Local(Player)` wraps the existing local engine.
   - `PlaybackBackend::MusicApp(MusicAppController)` wraps `Music.app` control.
3. Replace direct `Player` ownership in `App` with `PlaybackBackend`, while keeping the public API
   surface close to the methods the UI already uses.
4. Scope the initial `Music.app` backend to transport and state only:
   - read player state
   - read current track metadata needed by the UI
   - play/pause toggle
   - next / previous track
   - stop
   - read and set output volume
   - read current position and duration for display
5. Explicitly do not support these features in `Music.app` mode in v1:
   - local decode or `play(path)` through the internal Rust audio pipeline
   - gapless preload
   - EQ
   - mono
   - seek
   - waveform / oscilloscope samples
   - album art extraction through the existing local decode path
6. The UI must show unsupported controls as disabled or omit them entirely when the backend is
   `MusicApp`.
7. Backend choice must be explicit at startup via `--backend <local|music-app>`.
   - default is `local`
   - no environment-variable backend selection in v1
   - no config-persisted backend selection in v1
8. In `music-app` mode, positional media arguments are invalid in v1.
   - `cliamp --backend music-app` starts in remote-control mode
   - `cliamp --backend music-app Alive.mp3` fails fast with a clear startup error
9. On non-macOS platforms, selecting the `music-app` backend must fail fast with a clear startup
   error.

## Non-goals

- No Apple Music API integration in this ADR.
- No direct Apple Music stream playback inside `cliamp`.
- No attempt to make `Music.app` conform to the current `Provider` trait.
- No playlist/library browsing in v1 of this ADR.
- No cross-platform compatibility for the `Music.app` backend.
- No persistence of Apple account or MusicKit credentials.
- No backend selection through environment variables or saved config in v1.

## Consequences

- Good, because Apple Music playback support stays on the officially supported macOS playback surface instead of the Rust decode path.
- Good, because the local `Player` remains unchanged for files, streams, gapless playback, EQ, and visualizers.
- Good, because an enum wrapper is simpler than introducing a broad object-safe transport trait across the codebase.
- Good, because unsupported features become explicit backend capability differences instead of hidden breakage.
- Good, because startup behavior is deterministic: the user either starts the local player or starts `Music.app` remote mode.
- Bad, because the UI and config paths will need to branch on backend capabilities.
- Bad, because `Music.app` automation is macOS-only and depends on AppleScript/automation behavior.
- Neutral, because this is a playback control mode, not a full Apple Music service integration.

## Implementation Plan

- **Affected paths**:
  - `src/external/mod.rs` (export `music_app` module)
  - `src/external/music_app.rs` (AppleScript adapter + output parsing)
  - `src/playback_backend.rs` (define `PlaybackBackend` enum and capability helpers)
  - `src/main.rs` (parse `--backend`, enforce startup invariants, print usage/errors)
  - `src/ui/mod.rs` (store backend instead of raw `Player`, poll backend state)
  - `src/ui/keys.rs` (disable unsupported transport commands)
  - `src/ui/view.rs` (render backend-aware controls/status)
  - `src/ui/view_808.rs` (render backend-aware controls/status)
  - `README.md` (document `--backend music-app` usage and v1 limitations)
- **Patterns to follow**:
  - Keep `Music.app` integration in `src/external/`, not in `src/player/`.
  - Preserve existing local-player behavior when backend is `Local`.
  - Add narrow forwarding methods on `PlaybackBackend` for only the UI-used operations.
  - Represent unsupported operations via backend capability checks rather than silent no-ops.
  - Keep AppleScript parsing deterministic and unit-testable from captured output strings.
  - Validate CLI startup before constructing the app so unsupported backend/arg combinations fail early.
- **Patterns to avoid**:
  - Do not route Apple Music playback through `decode::decode_source`.
  - Do not expand the existing `Provider` trait to cover playback transport.
  - Do not add partial EQ/seek/visualizer behavior that implies parity where none exists.
  - Do not auto-switch backends based on environment heuristics.
  - Do not accept positional media arguments in `music-app` mode until a later ADR defines that behavior.

### Verification

- [x] Starting `cliamp` with the default backend preserves current local playback behavior.
- [x] `cliamp foo.mp3` behaves exactly as today and uses the local backend.
- [x] Starting `cliamp` with `music-app` on macOS shows current Music.app playback state in the UI.
- [x] `cliamp --backend music-app` starts without requiring positional media arguments.
- [x] `cliamp --backend music-app Alive.mp3` fails with a clear startup error.
- [x] `Space`, next-track, previous-track, and volume commands control `Music.app` instead of the local engine.
- [x] Seek, EQ, mono, waveform/scope, and cover-art paths are hidden or disabled in `music-app` mode.
- [x] Non-macOS startup with `music-app` fails immediately with a clear message.
- [x] Unit tests cover AppleScript output parsing and backend capability checks.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [x] `cargo test` passes.

## Alternatives Considered

- **Keep `cliamp` local-only**: simplest, but provides no Apple Music support at all.
- **Use the Apple Music API for playback**: not a good fit for this Rust terminal player and does not map cleanly to the current local decode architecture.
- **Introduce a broad `Transport` trait**: more extensible, but overkill for two known backends and a UI that already expects a fairly concrete playback API.
- **Overload the `Provider` trait to include playback control**: wrong abstraction; providers supply browsable remote data, not transport commands.

## More Information

Related existing decisions:

- ADR-0002: Use cpal directly for audio output
- ADR-0003: Symphonia decode with FFmpeg fallback
- ADR-0004: Tokio async with dedicated cpal audio thread
- ADR-0007: Provider trait for external music services

External references checked 2026-03-07:

- <https://developer.apple.com/musickit/>
- <https://docs.rs/apple-music/latest/apple_music/>
- macOS `Music.app` scripting dictionary (`sdef /System/Applications/Music.app`)

Implementation update 2026-03-07:

- Phase 1 is implemented in:
  - `src/external/music_app.rs`
  - `src/playback_backend.rs`
  - `src/main.rs`
  - `src/ui/mod.rs`
  - `src/ui/keys.rs`
  - `src/ui/view.rs`
  - `src/ui/view_808.rs`
  - `README.md`
- Startup contract is now live:
  - `cliamp --backend music-app`
  - `cliamp --backend music-app <media>` fails fast by design
- Automated verification completed:
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test -q`
  - `cargo test -q platform_guard_`
  - `cargo test -q live_music_app_transport_keys_control_music_app -- --ignored`
- Manual smoke verification completed on macOS:
  - the TUI starts in `music-app` mode
  - current Music.app playback state is shown in the UI
  - play/pause of an already playing track works in phase 1
  - local backend release smoke with `./target/release/cliamp Alive.mp3` starts and renders `▶ Playing`
  - `./target/release/cliamp --backend music-app Alive.mp3` fails with `music-app backend does not accept media paths or URLs`
  - `music-app` mode renders the reduced control surface, omitting local-only playlist/search/EQ/visualizer/art actions
  - ignored live test `ui::keys::tests::live_music_app_transport_keys_control_music_app` drives `Space`, `.`, `,`, `+`, and `-` through `App.handle_key(...)` and confirms they affect `Music.app`
  - unit tests `platform_guard_rejects_non_macos` and `platform_guard_accepts_macos` cover the platform gate used by `music-app` startup
- ADR-0014 partially supersedes the phase-1 “hide visualizer” restriction by allowing synthetic
  visualizers only, while keeping true audio-derived waveform/scope out of scope for `music-app`
  mode.
