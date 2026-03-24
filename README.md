# CLIAMP-RS

A Winamp 2.x-inspired terminal music player, written in Rust with Ratatui. Port of [cliamp](https://github.com/bjarneo/cliamp) (Go/Bubbletea).

<https://en.wikipedia.org/wiki/Winamp>

```
C L I A M P
♫ Artist - Song Title
01:23 / 04:56                    ▶ Playing

█████▆▃▁  ▂▅███▇▅▃▁  ▁▃▅▇█     (spectrum)
━━━━━━━━━━━━━●━━━━━━━━━━━━━━━━

VOL ████████████░░░░░░░░  +0.0dB
EQ  70 180 320 600 1k 3k 6k 12k 14k 16k [Rock]

── Playlist ── [Shuffle] [Repeat: All] ──
▶ 1. First Track
  2. Second Track
  3. Third Track
```

## Features

- Local file playback: MP3, FLAC, WAV, OGG (native via Symphonia)
- Codec-licensed formats: M4A, AAC, OPUS, WMA, ALAC (via FFmpeg fallback)
- HTTP audio streaming with ICY metadata (SHOUTcast/Icecast stream titles)
- YouTube/SoundCloud/Bandcamp via yt-dlp (two-phase: fast enumerate, lazy download)
- Podcast RSS feeds and M3U/M3U8 playlists
- Navidrome/Subsonic server integration (browse and play remote playlists)
- macOS `Music.app` remote-control backend (`--backend music-app`)
- Gapless playback with preloaded next track
- 10-band parametric EQ with 10 built-in presets
- FFT/time-domain visualizer (bars, bricks, oscilloscope modes)
- Album art display from embedded cover art (sixel/kitty/iTerm2/halfblocks, auto-detected)
- 17 built-in themes + custom user themes
- Roland TR-808 alternate UI mode with animated tachyonfx chrome
- Playlist: shuffle (Fisher-Yates), repeat (off/all/one), queue
- Volume control (dB) with mono downmix
- Search within playlist
- Save downloaded yt-dlp tracks to `~/Music/cliamp/`
- Config persistence (`~/.config/cliamp/config.toml`)

## Requirements

- Rust 2024 edition (1.85+)
- macOS, Linux, or Windows (via cpal)
- **Optional**: [FFmpeg](https://ffmpeg.org/) for M4A/AAC/OPUS/WMA playback
- **Optional**: [yt-dlp](https://github.com/yt-dlp/yt-dlp) for YouTube/SoundCloud/Bandcamp

```bash
# macOS
brew install ffmpeg yt-dlp

# Ubuntu/Debian
sudo apt install ffmpeg
pip install yt-dlp
```

## Build & Run

```bash
# Build
cargo build

# Build (release, optimized)
cargo build --release

# Run with local files
cargo run -- track.mp3 song.flac ~/Music/

# Run with a YouTube playlist
cargo run -- "https://www.youtube.com/playlist?list=..."

# Run with a podcast feed
cargo run -- "https://example.com/podcast/feed.xml"

# Run with an HTTP stream / internet radio
cargo run -- "https://example.com/stream.mp3"
cargo run -- "http://ice1.somafm.com/groovesalad-256-mp3"

# Run with Navidrome (browse server playlists)
NAVIDROME_URL=https://music.example.com NAVIDROME_USER=alice NAVIDROME_PASS=secret cargo run

# Run in macOS Music.app remote-control mode
cargo run -- --backend music-app

# Release binary examples
./target/release/cliamp "Alive.mp3"
./target/release/cliamp --backend music-app
```

### Navidrome / Subsonic

Set these environment variables to connect to a [Navidrome](https://www.navidrome.org/) (or any Subsonic-compatible) server:

| Variable | Description |
|----------|-------------|
| `NAVIDROME_URL` | Server base URL (e.g. `https://music.example.com`) |
| `NAVIDROME_USER` | Username |
| `NAVIDROME_PASS` | Password |

When all three are set, cliamp starts in provider mode showing your server's playlists. Use `↑↓` to navigate, `Enter` to load a playlist, and `Esc` to return to the playlist browser. You can also pass local files alongside Navidrome — they'll be in the playlist while the provider browser is available via `Tab`.

### Music.app Backend

On macOS, cliamp can control the system `Music.app` instead of its local audio engine:

```bash
./target/release/cliamp --backend music-app
```

Phase 1 scope:

- Reads current `Music.app` title, artist, play state, position, duration, and volume
- Controls `play/pause`, `next`, `previous`, `stop`, and volume
- Exposes synthetic visualizers in both standard and 808 views
- Does not accept local file or URL arguments in this mode
- Does not expose local playlist loading, EQ, or album art controls in this mode

The first run may prompt for macOS Automation permission to control `Music.app`.

### Manual Test: Music.app Phase 1

1. Open `Music.app`
2. Start a track in `Music.app`
3. Run:

```bash
./target/release/cliamp --backend music-app
```

1. Verify in the TUI:
   - current title/artist appears in the player
   - `Space` toggles play/pause
   - `>` and `<` move to next/previous track
   - `s` stops playback
   - `+` and `-` change volume
   - `v` cycles synthetic visualizer variants
   - `8` switches between standard and 808 layouts

Invalid usage:

```bash
./target/release/cliamp --backend music-app "Alive.mp3"
```

That fails by design because `music-app` mode is a remote-control backend, not a local file player.

## Development

```bash
# Run tests
cargo test

# Run clippy lints
cargo clippy

# Format code
cargo fmt

# Build and run in one step
cargo run -- <args>
```

### Project Structure

```
src/
├── main.rs               # CLI entry point, tokio runtime, signal handling
├── config.rs             # ~/.config/cliamp/config.toml (serde + toml)
├── external/
│   ├── mod.rs            # External service providers
│   ├── music_app.rs      # macOS Music.app AppleScript controller
│   ├── apple_music_api.rs# Apple Music metadata client (internal, phase 1)
│   └── navidrome.rs      # Navidrome/Subsonic client (MD5 token auth)
├── playback_backend.rs   # Local player vs Music.app backend wrapper
├── playlist/
│   └── mod.rs            # Track, Playlist, Provider trait, shuffle, repeat, queue
├── resolve/
│   ├── mod.rs            # URL routing, feed/M3U resolution, file collection
│   └── ytdl.rs           # Two-phase yt-dlp: flat-playlist + lazy download
├── player/
│   ├── mod.rs            # Player state machine, cpal output stream
│   ├── source.rs         # AudioSource trait
│   ├── decode.rs         # Symphonia decoder, PcmSource, resampling, HTTP routing
│   ├── ffmpeg.rs         # FFmpeg subprocess fallback
│   ├── http.rs           # HttpMediaSource (Symphonia MediaSource over HTTP)
│   ├── icy.rs            # IcyReader (SHOUTcast/Icecast metadata extraction)
│   ├── stream.rs         # StreamingSource (on-demand packet decode)
│   ├── eq.rs             # 10-band biquad parametric EQ
│   ├── volume.rs         # dB volume + mono downmix
│   ├── gapless.rs        # GaplessSource (zero-gap transitions)
│   └── tap.rs            # Ring buffer for FFT visualizer
└── ui/
    ├── mod.rs            # App struct, tokio select! event loop, provider state
    ├── view.rs           # Ratatui rendering (Winamp 2.x layout)
    ├── view_808.rs       # Roland TR-808 alternate layout
    ├── keys.rs           # Key event handlers (Playlist/EQ/Provider focus)
    ├── styles.rs         # Color/style constants (ANSI palette)
    ├── theme.rs          # Theme loading (17 built-in + custom TOML)
    ├── visualizer.rs     # FFT spectrum analyzer (rustfft)
    └── eq_presets.rs     # 10 built-in EQ presets
```

### Architecture Decision Records

Key architectural decisions are documented in [adr/](adr/):

- [ADR-0002: cpal direct for audio output](adr/0002-use-cpal-direct-for-audio-output.md)
- [ADR-0003: Symphonia + FFmpeg fallback](adr/0003-symphonia-decode-with-ffmpeg-fallback.md)
- [ADR-0004: Tokio async with cpal audio thread](adr/0004-tokio-async-with-cpal-audio-thread.md)
- [ADR-0005: Two-phase yt-dlp resolution](adr/0005-two-phase-ytdlp-resolution.md)
- [ADR-0006: HTTP streaming with ICY metadata](adr/0006-http-streaming-with-icy-metadata.md)
- [ADR-0007: Provider trait for external music services](adr/0007-provider-trait-for-external-music-services.md)
- [ADR-0008: Album art display via ratatui-image](adr/0008-album-art-display-via-ratatui-image.md)

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Space` | Play / Pause |
| `s` | Stop |
| `> .` | Next track |
| `< ,` | Previous track |
| `← →` | Seek ±5s |
| `+ -` | Volume up/down |
| `m` | Toggle mono |
| `e` | Cycle EQ preset |
| `v` | Cycle visualizer |
| `c` | Toggle album art |
| `↑ ↓` | Playlist scroll / EQ adjust |
| `h l` | EQ cursor left/right |
| `Enter` | Play selected track |
| `a` | Toggle queue (play next) |
| `S` | Save track to ~/Music |
| `r` | Cycle repeat |
| `z` | Toggle shuffle |
| `/` | Search playlist |
| `t` | Choose theme |
| `8` | Toggle 808 mode |
| `Tab` | Cycle focus (playlist/EQ/provider) |
| `Esc` | Back to provider (when in playlist with provider) |
| `Ctrl+K` | Keymap overlay |
| `q` | Quit |

Music.app backend uses a reduced control surface: `Space`, `s`, `>`, `<`, `+`, `-`, `t`, `v`, `8`, `:`, and `q`.

## EQ Presets

Flat, Rock, Pop, Jazz, Classical, Bass Boost, Treble Boost, Vocal, Electronic, Acoustic

## License

MIT
