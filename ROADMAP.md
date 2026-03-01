# CLIAMP-RS Roadmap

## Done

### Core Audio Engine
- [x] MP3, WAV, FLAC, OGG native decode (Symphonia)
- [x] M4A/AAC/ALAC/WMA/Opus via FFmpeg subprocess fallback
- [x] Gapless playback with preloaded next track
- [x] 10-band parametric biquad EQ with 10 presets
- [x] Volume control (dB, [-30, +6])
- [x] Mono downmix (L+R)
- [x] FFT tap / ring buffer for spectrum visualizer
- [x] Linear resampling to 44100 Hz

### Playlist
- [x] Sequential, shuffle (Fisher-Yates), repeat (off/all/one)
- [x] Queue (play-next) with `[Qn]` badges
- [x] Peek-next for gapless preload
- [x] "Artist - Title" filename parsing
- [x] URL-to-title extraction

### Input Sources
- [x] Local files and directories (recursive walk, sorted)
- [x] Podcast RSS/XML feed parsing
- [x] M3U/M3U8 playlist parsing
- [x] yt-dlp Phase 1: flat-playlist enumerate (fast)
- [x] yt-dlp Phase 2: lazy per-track download (on select)
- [x] yt-dlp temp directory cleanup (RAII + ctrlc signal handler)
- [x] Save downloaded tracks to `~/Music/cliamp/` (`S` key)

### TUI
- [x] Winamp 2.x-inspired layout (centered, 80-col)
- [x] Track info with marquee scroll for long titles
- [x] Time/duration display + playback status indicator
- [x] Seek bar with progress thumb
- [x] Volume bar with dB readout
- [x] 10-band EQ row with cursor and preset display
- [x] FFT spectrum visualizer (Bars + Bricks modes)
- [x] Playlist view with active track, cursor, queue badges
- [x] Search mode (`/`) with live filtering
- [x] Keymap overlay (`Ctrl+K`)
- [x] Error and save message display with TTL

### Themes
- [x] 17 built-in color themes (catppuccin, gruvbox, nord, tokyo-night, etc.)
- [x] Theme picker overlay (`t` key) with live preview
- [x] Custom user themes from `~/.config/cliamp/themes/*.toml`
- [x] Theme name persisted in config

### Roland TR-808 Mode
- [x] Toggleable alternate layout (`8` key)
- [x] 808 color palette (red/orange/amber/yellow/black/grey)
- [x] Canvas-based rotary knobs for volume + 10 EQ bands (Braille markers)
- [x] LED-style spectrum with 808 color gradient
- [x] TR-808 header branding
- [x] Mode persisted in config

### Config & Persistence
- [x] `~/.config/cliamp/config.toml` with serde
- [x] All state saved on exit (volume, repeat, shuffle, mono, EQ, theme, 808 mode)
- [x] Validation and clamping on load

### Quality
- [x] 100 unit tests across all modules
- [x] Zero warnings, zero clippy issues
- [x] 4 Architecture Decision Records (ADRs)

---

## Todo

### High Priority — Go Feature Parity
- [ ] **HTTP audio streaming** — Player can't open `http://` URLs. Need HTTP GET + streaming decode for direct URL playback (radio stations, hosted MP3s)
- [ ] **ICY metadata** — SHOUTcast/Icecast stream title extraction. Show live station/track titles during radio playback. Depends on HTTP streaming
- [ ] **Stream-aware UI** — "Streaming" indicator, static seek bar for non-seekable sources, context-aware help bar
- [ ] **Theme picker cancel-restore** — Esc should revert to original theme instead of keeping the previewed one
- [ ] **Chained OGG** — Re-initialize vorbis decoder on logical bitstream boundary for Icecast radio continuation

### Medium Priority — Major Features
- [ ] **Navidrome/Subsonic provider** — Full client (MD5 token auth, `getPlaylists`, `getPlaylist`, `stream`), provider browser UI with `b`/Esc back navigation, `NAVIDROME_URL`/`NAVIDROME_USER`/`NAVIDROME_PASS` env vars
- [ ] **MPRIS2 D-Bus** (Linux) — Media key control, desktop widget integration, metadata push (`org.mpris.MediaPlayer2.Player`), volume sync, ICY title in metadata

### Low Priority — Distribution & Polish
- [ ] **`install.sh`** — Curl-pipe installer from GitHub releases
- [ ] **CI release workflow** — GitHub Actions for cross-platform binaries (linux/darwin amd64/arm64, windows amd64)
- [ ] **`config.toml.example`** — Sample config file with all fields documented
- [ ] **tachyonfx effects** — Post-render visual effects for 808 mode (blocked on ratatui version compatibility)

### Ideas / Future
- [ ] Mouse support for seek bar and playlist
- [ ] Album art display (sixel/kitty protocol)
- [ ] Last.fm scrobbling
- [ ] Discord rich presence
- [ ] Lyrics display (synced/unsynced)
