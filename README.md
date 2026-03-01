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
- Gapless playback with preloaded next track
- 10-band parametric EQ with 10 built-in presets
- FFT spectrum visualizer (bars and bricks modes)
- 17 built-in themes + custom user themes
- Roland TR-808 alternate UI mode
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
```

### Navidrome / Subsonic

Set these environment variables to connect to a [Navidrome](https://www.navidrome.org/) (or any Subsonic-compatible) server:

| Variable | Description |
|----------|-------------|
| `NAVIDROME_URL` | Server base URL (e.g. `https://music.example.com`) |
| `NAVIDROME_USER` | Username |
| `NAVIDROME_PASS` | Password |

When all three are set, cliamp starts in provider mode showing your server's playlists. Use `↑↓` to navigate, `Enter` to load a playlist, and `Esc` to return to the playlist browser. You can also pass local files alongside Navidrome — they'll be in the playlist while the provider browser is available via `Tab`.

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
│   └── navidrome.rs      # Navidrome/Subsonic client (MD5 token auth)
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

## EQ Presets

Flat, Rock, Pop, Jazz, Classical, Bass Boost, Treble Boost, Vocal, Electronic, Acoustic

## License

MIT
