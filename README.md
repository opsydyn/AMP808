# CLIAMP-RS

A Winamp 2.x-inspired terminal music player, written in Rust with Ratatui. Port of [cliamp](https://github.com/bjarneo/cliamp) (Go/Bubbletea).

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
- YouTube/SoundCloud/Bandcamp via yt-dlp (two-phase: fast enumerate, lazy download)
- Podcast RSS feeds and M3U/M3U8 playlists
- Gapless playback with preloaded next track
- 10-band parametric EQ with 10 built-in presets
- FFT spectrum visualizer (bars and bricks modes)
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

# Run with an HTTP stream
cargo run -- "https://example.com/stream.mp3"
```

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
├── playlist/
│   └── mod.rs            # Track, Playlist, shuffle, repeat, queue, search
├── resolve/
│   ├── mod.rs            # URL routing, feed/M3U resolution, file collection
│   └── ytdl.rs           # Two-phase yt-dlp: flat-playlist + lazy download
├── player/
│   ├── mod.rs            # Player state machine, cpal output stream
│   ├── source.rs         # AudioSource trait
│   ├── decode.rs         # Symphonia decoder, PcmSource, resampling
│   ├── ffmpeg.rs         # FFmpeg subprocess fallback
│   ├── eq.rs             # 10-band biquad parametric EQ
│   ├── volume.rs         # dB volume + mono downmix
│   ├── gapless.rs        # GaplessSource (zero-gap transitions)
│   └── tap.rs            # Ring buffer for FFT visualizer
└── ui/
    ├── mod.rs            # App struct, tokio select! event loop
    ├── view.rs           # Ratatui rendering (Winamp 2.x layout)
    ├── keys.rs           # Key event handlers
    ├── styles.rs         # Color/style constants (ANSI palette)
    ├── visualizer.rs     # FFT spectrum analyzer (rustfft)
    └── eq_presets.rs     # 10 built-in EQ presets
```

### Architecture Decision Records

Key architectural decisions are documented in [adr/](adr/):

- [ADR-0002: cpal direct for audio output](adr/0002-use-cpal-direct-for-audio-output.md)
- [ADR-0003: Symphonia + FFmpeg fallback](adr/0003-symphonia-decode-with-ffmpeg-fallback.md)
- [ADR-0004: Tokio async with cpal audio thread](adr/0004-tokio-async-with-cpal-audio-thread.md)
- [ADR-0005: Two-phase yt-dlp resolution](adr/0005-two-phase-ytdlp-resolution.md)

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
| `Tab` | Toggle focus (playlist/EQ) |
| `Ctrl+K` | Keymap overlay |
| `q` | Quit |

## EQ Presets

Flat, Rock, Pop, Jazz, Classical, Bass Boost, Treble Boost, Vocal, Electronic, Acoustic

## License

MIT
