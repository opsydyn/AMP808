# `cliamp-main-new` discovery and parity plan

Date: 2026-03-24

## Purpose

This document captures what the original Go codebase now mirrored in-repo at `cliamp-go/` grew into, how it is structured, which features matter for `amp808`, and a recommended parity plan.

This is intended as a posterity/reference document for future `amp808` planning so we do not have to re-discover the original implementation from scratch.

The `cliamp-go/` directory is a reference snapshot added to this repository specifically so file-level comparisons stay stable and easy to cite. Unless stated otherwise, Go file paths mentioned below are relative to `cliamp-go/`.

## Scope and method

This discovery pass used:

- top-level repo structure inspection
- direct reads from the vendored reference tree in `cliamp-go/`
- direct reads of representative implementation files
- direct reads of the original product docs in `docs/`
- comparison against the current Rust codebase in `/Users/alan/Projects/cliamp-rs`

The goal was not to reverse-engineer every line, but to preserve the architecture, file map, feature surface, and the parity priorities that actually matter.

## Executive summary

The original Go player evolved into a broader **provider-centric terminal music platform** with:

- multiple authenticated providers: Spotify, YouTube Music, YouTube, Plex, Navidrome
- a built-in Radio provider plus user-defined stations
- large visualizer breadth
- lyrics overlay
- MPRIS integration on Linux
- local playlist management
- runtime URL loading and interactive search flows
- more exposed audio-engine tuning via config/CLI flags

`amp808` is already stronger in a few important ways:

- distinct product identity and TR-808 presentation
- album art support
- incremental BPM display and shared BPM state across views
- macOS `Music.app` backend
- smaller, cleaner Rust core with good tests and simpler maintenance

The recommendation is **not** to clone the old app wholesale.

The right parity order is:

1. MPRIS2
2. Lyrics
3. Runtime URL loading
4. Persisted visualizer mode
5. A curated visualizer expansion
6. Playlist/queue management
7. Richer Navidrome browsing
8. Additional providers (Plex first, Spotify much later)

## Feature inventory from the original Go app

### Providers and source types

The original supports:

- local files and directories
- HTTP streams
- M3U / M3U8 playlists
- PLS playlists
- podcast RSS feeds
- Xiaoyuzhou episode URLs
- yt-dlp-backed URLs and searches
- Radio provider
- Navidrome provider
- Plex provider
- Spotify provider
- YouTube provider
- YouTube Music provider
- local TOML playlists as managed provider content

### Visualizers and waveform modes

The original visualizer subsystem includes these named modes in `ui/visualizer.go`:

- `Bars`
- `BarsDot`
- `Rain`
- `BarsOutline`
- `Bricks`
- `Columns`
- `Wave`
- `Scatter`
- `Flame`
- `Retro`
- `Pulse`
- `Matrix`
- `Binary`
- `Sakura`
- `Firework`
- `Logo`
- `Terrain`
- `Glitch`
- `Scope`
- `Heartbeat`
- `Butterfly`
- `Lightning`
- `None`

This is much broader than current `amp808`, which presently exposes a smaller, more curated set.

### Interaction surface

The original TUI supports:

- `Ctrl+K` keymap overlay
- full-screen visualizer (`V`)
- lyrics overlay (`y`)
- track info overlay (`i`)
- URL input (`u`)
- YouTube search (`f`)
- SoundCloud search (`F`)
- playlist manager (`p`)
- queue manager (`A`)
- file browser (`o`)
- jump-to-time (`J`)
- playlist expand/collapse (`x`)
- playlist reorder via `Shift+Up/Down`
- provider browser and provider-pill switching
- full Navidrome browser (`N`)

### Platform and system integration

The original also includes:

- Linux MPRIS2 D-Bus service
- resume-state persistence across restarts
- self-upgrade command
- built-in and user-defined theme system
- config-driven audio quality tuning

## High-value differences vs current `amp808`

### Areas where the Go app is ahead

- provider breadth
- provider-specific library browsing depth
- runtime utility overlays and managers
- visualizer breadth
- MPRIS support
- lyrics
- richer CLI/config surface

### Areas where `amp808` is ahead or more distinctive

- TR-808 mode is a genuine differentiator, not just a theme
- album art support is stronger and more integrated
- BPM estimation/display is an original `amp808` strength
- the macOS `Music.app` backend is unique to the Rust line
- the Rust codebase is leaner and easier to evolve deliberately

## Recommended parity plan

### Phase 1 — daily-driver parity

1. **MPRIS2**
   - transport control
   - metadata push
   - position and volume sync
   - ICY metadata propagation
2. **Lyrics**
   - unsynced first
   - synced when timestamps exist
   - local and Navidrome-first scope
3. **Runtime URL input**
   - direct stream URLs
   - feeds
   - M3U / PLS
   - yt-dlp-supported links
4. **Persisted visualizer mode**
   - config save/load
   - maybe per-view defaults later

### Phase 2 — curated visualizer parity

Recommended first additions:

- `Wave`
- `Columns`
- `Scatter`
- `Flame`
- `Retro`
- `None`
- optional richer scope controls
- optional full-screen visualizer mode

Not recommended initially: porting every novelty mode immediately.

### Phase 3 — management workflows

- local playlist manager
- queue manager
- file browser
- track info overlay
- playlist expand/collapse if still useful after Rust UX review

### Phase 4 — Navidrome parity

- album browser
- artist browser
- artist → album drill-down
- append/replace actions
- album sort modes
- optional scrobbling hooks

### Phase 5 — additional providers

Recommended order:

1. Plex
2. Radio/provider polish
3. YouTube / YouTube Music library provider
4. Spotify

Spotify should be treated as a deliberate product decision, not a default parity checkbox.

## Original Go architecture map

The following sections capture the important Go files and their roles.

## Entry point and composition

| File | What it does |
| --- | --- |
| `cliamp-go/main.go` | Main composition root. Loads config and CLI overrides, constructs providers (`radio`, `navidrome`, `plex`, `spotify`, `ytmusic`), resolves CLI inputs, configures the player, wires Bubble Tea, MPRIS, resume state, theme, visualizer, autoplay, and upgrade/help/version flows. |

## Configuration and CLI surface

| File | What it does |
| --- | --- |
| `cliamp-go/config/config.go` | Loads and saves `~/.config/cliamp/config.toml`, defines `Config`, provider sub-configs (`NavidromeConfig`, `SpotifyConfig`, `YouTubeMusicConfig`, `PlexConfig`), clamps values, and persists targeted keys like theme and Navidrome browse sort. |
| `cliamp-go/config/flags.go` | Parses CLI flags into `Overrides`, supports mixed flags/positionals, action flags (`help`, `version`, `upgrade`), and config overrides like provider/theme/visualizer/audio quality. |
| `cliamp-go/config/config_seek_test.go` | Tests seek-step related config behavior. |

## Playlist domain and metadata helpers

| File | What it does |
| --- | --- |
| `cliamp-go/playlist/provider.go` | Defines the provider abstraction: `Provider`, `PlaylistInfo`, and optional `Authenticator` support for interactive sign-in providers. |
| `cliamp-go/playlist/playlist.go` | Core ordered playlist model: tracks, queue, shuffle, repeat, move/reorder, current/next/prev, URL/site classification helpers, and `Track` metadata shape. |
| `cliamp-go/playlist/tags.go` | Reads embedded media metadata for local files before falling back to filename parsing. |
| `cliamp-go/playlist/encoding.go` | Encoding/text helpers used by playlist and metadata handling. |
| `cliamp-go/playlist/playlist_test.go` | Tests playlist behavior. |

## Resolver pipeline

| File | What it does |
| --- | --- |
| `cliamp-go/resolve/resolve.go` | Main CLI argument resolver. Splits inputs into immediate tracks vs deferred remote URLs, resolves feeds/M3U/PLS/YouTube/yt-dlp/Xiaoyuzhou, handles local globbing and recursive file discovery, and exposes `ResolveYTDLBatch` for incremental loading. |
| `cliamp-go/resolve/m3u.go` | Parses M3U / M3U8 playlists including metadata and relative-path handling. |
| `cliamp-go/resolve/pls.go` | Parses PLS playlists. |
| `cliamp-go/resolve/resolve_test.go` | Resolver coverage. |
| `cliamp-go/resolve/xiaoyuzhou.go` | Resolves Xiaoyuzhou episode URLs into playable tracks. |

## Audio engine and decoding

| File | What it does |
| --- | --- |
| `cliamp-go/player/player.go` | Main playback engine. Owns speaker setup, current/next pipeline state, gapless transitions, volume/EQ/mono/tap state, local and stream playback, preloading, pause/stop, seek, yt-dlp restart-based seek, position/duration, and custom URI streamer support (for Spotify). |
| `cliamp-go/player/pipeline.go` | Builds `trackPipeline` objects, opens sources, selects decode path, creates navBuffer/ffmpeg/native/chained-Ogg pipelines, tracks seekability and content length, and manages resource cleanup. |
| `cliamp-go/player/decode.go` | Decodes local files and HTTP sources, maps content types to extensions, decides when ffmpeg is required, and opens HTTP sources with ICY metadata support. |
| `cliamp-go/player/ffmpeg.go` | FFmpeg-backed decode paths for unsupported formats and stream cases. |
| `cliamp-go/player/gapless.go` | Gapless streamer implementation and next-track transition plumbing. |
| `cliamp-go/player/eq.go` | 10-band biquad EQ implementation. |
| `cliamp-go/player/volume.go` | dB volume and mono-downmix stage. |
| `cliamp-go/player/tap.go` | Sample tap used by visualizers and downstream analysis. |
| `cliamp-go/player/icy.go` | ICY metadata reader for live streams. |
| `cliamp-go/player/nav_buffer.go` | Background-buffered Navidrome stream reader used for seek/restart behavior without direct reconnect semantics. |
| `cliamp-go/player/chained_ogg.go` | Handles chained OGG/Vorbis streams so radio continues across logical bitstreams. |
| `cliamp-go/player/ytdl.go` | yt-dlp integration inside the playback layer, including stream/save helpers. |
| `cliamp-go/player/device_darwin.go` | Platform-specific output-device sample-rate detection for macOS. |
| `cliamp-go/player/device_other.go` | Non-macOS sample-rate/device helpers. |

## Provider implementations

### Local playlists

| File | What it does |
| --- | --- |
| `cliamp-go/external/local/provider.go` | Local playlist provider backed by `~/.config/cliamp/playlists/*.toml`. Supports playlist listing, track loading, append, overwrite, delete, and remove-track operations. |

### Navidrome

| File | What it does |
| --- | --- |
| `cliamp-go/external/navidrome/client.go` | Subsonic/Navidrome client and provider. Handles auth token construction, playlist listing, playlist track loading, artist listing, album listing, artist-album traversal, album-track loading, and sort-mode support for browsing. |

### Plex

| File | What it does |
| --- | --- |
| `cliamp-go/external/plex/client.go` | Plex HTTP client that talks to Plex Media Server APIs, enumerates music sections/albums/tracks, and builds authenticated stream URLs. |
| `cliamp-go/external/plex/provider.go` | Adapts the Plex client to the generic `playlist.Provider` interface and caches album/track results. |
| `cliamp-go/external/plex/client_test.go` | Plex client tests. |
| `cliamp-go/external/plex/provider_test.go` | Plex provider tests. |

### Radio

| File | What it does |
| --- | --- |
| `cliamp-go/external/radio/provider.go` | Built-in Radio provider with predefined cliamp stations plus user-defined stations from `~/.config/cliamp/radios.toml`. Exposes stations as single-track playlists. |

### Spotify

| File | What it does |
| --- | --- |
| `cliamp-go/external/spotify/provider.go` | Spotify provider implementation using Spotify Web API for library/playlists/tracks and go-librespot session integration for playback URIs. Handles auth deferral, interactive auth, playlist filtering, caching, and track metadata. |
| `cliamp-go/external/spotify/session.go` | Spotify OAuth/session lifecycle and credential persistence for the provider. |
| `cliamp-go/external/spotify/streamer.go` | Converts Spotify track URIs into streamers consumable by the player pipeline via go-librespot. |
| `cliamp-go/external/spotify/stub_windows.go` | Windows build stub for Spotify support. |
| `cliamp-go/external/spotify/provider_test.go` | Spotify provider tests. |

### YouTube / YouTube Music

| File | What it does |
| --- | --- |
| `cliamp-go/external/ytmusic/provider.go` | Shared YouTube/YouTube Music provider logic. Handles session bootstrap, playlist fetch, classification, track loading, duration lookup, cache interaction, and exposes `YouTubeMusicProvider`, `YouTubeProvider`, and `YouTubeAllProvider`. |
| `cliamp-go/external/ytmusic/session.go` | OAuth session management and credential persistence for YouTube/Google APIs. |
| `cliamp-go/external/ytmusic/classify.go` | Music-vs-video playlist classification logic used to split YouTube and YouTube Music providers. |
| `cliamp-go/external/ytmusic/cache.go` | Disk cache for playlist lists, classification results, and track lists. |
| `cliamp-go/external/ytmusic/fallback.go` | Built-in/fallback credential support. |

## UI and visualizer layer

### UI composition and state

| File | What it does |
| --- | --- |
| `cliamp-go/ui/model.go` | Central Bubble Tea model. Owns focus state, provider state, overlays, visualizer, MPRIS integration, playback coordination, buffering, reconnect behavior, scrobbling, theme and config application, and async message handling. |
| `cliamp-go/ui/state.go` | Shared UI state structs for overlays and transient modes. |
| `cliamp-go/ui/keys.go` | Primary key handling for the TUI, including playback, overlays, manager toggles, provider interaction, save behavior, lyrics, file browser, URL input, visualizer mode persistence, and quit/resume behavior. |
| `cliamp-go/ui/keys_nav.go` | Key handling specific to the Navidrome browser workflow. |
| `cliamp-go/ui/keymap.go` | The searchable keymap overlay model and data. |
| `cliamp-go/ui/commands.go` | Bubble Tea commands for async tasks such as provider fetches, streaming, saves, searches, and background actions. |

### Rendering and layout

| File | What it does |
| --- | --- |
| `cliamp-go/ui/view.go` | Main player rendering. |
| `cliamp-go/ui/view_nav.go` | Provider and Navidrome-specific rendering paths. |
| `cliamp-go/ui/view_overlays.go` | Overlay rendering for managers, search, lyrics, keymap, etc. |
| `cliamp-go/ui/view_helpers.go` | Shared rendering helpers and formatting logic. |
| `cliamp-go/ui/styles.go` | Shared Lip Gloss style definitions and palette constants. |
| `cliamp-go/ui/seek.go` | Seek UI state and rendering/behavior helpers. |
| `cliamp-go/ui/jump.go` | Jump-to-time parsing and mode behavior. |
| `cliamp-go/ui/textinput.go` | Shared text input helpers for overlays and prompts. |

### Playlist and file workflows

| File | What it does |
| --- | --- |
| `cliamp-go/ui/filebrowser.go` | In-app file browser for loading local files/directories during runtime. |
| `cliamp-go/ui/ytdl_batch.go` | Incremental yt-dlp playlist/radio loading support used by the UI. |

### EQ and presets

| File | What it does |
| --- | --- |
| `cliamp-go/ui/eq_presets.go` | Built-in EQ preset definitions. |

### Visualizer core

| File | What it does |
| --- | --- |
| `cliamp-go/ui/visualizer.go` | FFT analysis, visualizer mode enum, mode dispatch, mode-name mapping, smoothing, rendering entry points, and shared helpers for the entire visualizer subsystem. |

### Individual visualizer renderers

| File | What it does |
| --- | --- |
| `cliamp-go/ui/vis_bars.go` | Standard bar visualizer. |
| `cliamp-go/ui/vis_bars_dot.go` | Dotted/stippled bars visualizer. |
| `cliamp-go/ui/vis_bars_outline.go` | Outline-style bars visualizer. |
| `cliamp-go/ui/vis_binary.go` | Binary rain / binary-themed visualizer. |
| `cliamp-go/ui/vis_bricks.go` | Brick/block spectrum visualizer. |
| `cliamp-go/ui/vis_butterfly.go` | Mirrored butterfly / Rorschach visualizer. |
| `cliamp-go/ui/vis_columns.go` | Thin-column visualizer. |
| `cliamp-go/ui/vis_firework.go` | Firework burst visualizer. |
| `cliamp-go/ui/vis_flame.go` | Flame-like visualizer. |
| `cliamp-go/ui/vis_glitch.go` | Glitch/corruption-themed visualizer. |
| `cliamp-go/ui/vis_heartbeat.go` | ECG/heartbeat visualizer. |
| `cliamp-go/ui/vis_lightning.go` | Lightning-bolt visualizer. |
| `cliamp-go/ui/vis_logo.go` | CLIAMP logo visualizer. |
| `cliamp-go/ui/vis_matrix.go` | Matrix rain visualizer. |
| `cliamp-go/ui/vis_pulse.go` | Pulse/circle visualizer. |
| `cliamp-go/ui/vis_rain.go` | Rain/falling-droplet visualizer. |
| `cliamp-go/ui/vis_retro.go` | Retro synthwave/grid visualizer. |
| `cliamp-go/ui/vis_sakura.go` | Sakura/falling-petal visualizer. |
| `cliamp-go/ui/vis_scatter.go` | Scatter/sparkle visualizer. |
| `cliamp-go/ui/vis_scope.go` | Oscilloscope / XY scope visualizer. |
| `cliamp-go/ui/vis_terrain.go` | Terrain/mountain visualizer. |
| `cliamp-go/ui/vis_wave.go` | Waveform visualizer. |

### UI tests

| File | What it does |
| --- | --- |
| `cliamp-go/ui/jump_test.go` | Jump mode tests. |
| `cliamp-go/ui/model_pause_test.go` | Pause behavior tests. |
| `cliamp-go/ui/model_seek_test.go` | Seek behavior tests. |
| `cliamp-go/ui/model_tick_test.go` | Tick/update loop tests. |

## Themes, lyrics, MPRIS, upgrade, and helpers

| File | What it does |
| --- | --- |
| `cliamp-go/theme/theme.go` | Loads embedded and user themes, parses theme TOML, and merges built-in/user theme sets. |
| `cliamp-go/lyrics/lyrics.go` | Lyrics fetcher using LRCLIB first and NetEase fallback. Parses synced LRC lines and supports query cleanup for noisy YouTube/SoundCloud titles. |
| `cliamp-go/mpris/mpris.go` | Linux MPRIS2 D-Bus service implementation, exported methods/signals/properties, metadata mapping, volume conversion, and event-loop message bridge. |
| `cliamp-go/mpris/mpris_stub.go` | Non-Linux no-op stub for MPRIS package APIs. |
| `cliamp-go/upgrade/upgrade.go` | Self-upgrade command that downloads the latest GitHub release binary and atomically replaces the current executable. |
| `cliamp-go/internal/appdir/appdir.go` | Resolves `~/.config/cliamp`. |
| `cliamp-go/internal/browser/` | Browser-launch helpers used by OAuth sign-in flows. |
| `cliamp-go/internal/fileutil/` | Shared file utilities such as copy helpers. |
| `cliamp-go/internal/httpclient/` | Tuned HTTP client setup used by the player/streaming stack. |
| `cliamp-go/internal/resume/` | Resume-state persistence helpers. |
| `cliamp-go/internal/tomlutil/` | Small TOML parsing/unquoting helpers used by config-like files. |

## What matters most for `amp808`

### Features worth carrying forward

1. `cliamp-go/mpris/mpris.go` style Linux integration
2. `cliamp-go/lyrics/lyrics.go` style lyrics capability
3. `cliamp-go/ui/keys.go` + `cliamp-go/ui/model.go` runtime URL input and manager flows
4. `cliamp-go/ui/visualizer.go` + selected `cliamp-go/ui/vis_*.go` modes
5. `cliamp-go/external/local/provider.go` style playlist manager support
6. `cliamp-go/external/navidrome/client.go` browse depth
7. `cliamp-go/config/flags.go` and `cliamp-go/config/config.go` ideas for selectively expanding Rust config/CLI

### Features to treat cautiously

1. `cliamp-go/external/spotify/*`
2. `cliamp-go/external/ytmusic/*`
3. very broad visualizer proliferation
4. self-upgrade in-app command

These are all valuable, but they add real maintenance and auth complexity.

## Proposed Rust follow-up work

### Short-term

- add a parity section to `ROADMAP.md`
- ADR for provider strategy
- ADR for visualizer expansion philosophy
- ADR for MPRIS scope
- ADR for lyrics source/sync scope

### Implementation order

1. MPRIS
2. Lyrics
3. Runtime URL input
4. Persisted visualizer mode
5. Curated visualizer expansion
6. Playlist manager
7. Richer Navidrome browser
8. Plex
9. Optional Spotify later

## Bottom line

The Go app is best understood as a **feature-rich terminal media hub**.

`amp808` should not try to become identical to it.

The best parity strategy is to preserve the old app’s strongest ideas where they reinforce `amp808`’s identity:

- system integration
- lyrics
- curated visual richness
- playlist and provider ergonomics
- stronger Navidrome workflows

and to avoid prematurely inheriting every provider, auth flow, and novelty mode just because the original accumulated them.
