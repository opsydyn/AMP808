---
status: accepted
date: 2026-02-28
decision-makers: alan
---

# Two-phase yt-dlp resolution for YouTube/SoundCloud/Bandcamp

## Context and Problem Statement

CLIAMP supports playback from YouTube, SoundCloud, Bandcamp, and other sites via yt-dlp. These URLs can be single tracks or playlists. Downloading audio for an entire playlist upfront is slow and wasteful — users may skip tracks.

The Go version solves this with a two-phase approach: fast enumerate first, lazy download on play. We need to port this strategy.

## Decision

Implement two-phase yt-dlp resolution, matching the Go version's approach:

**Phase 1 — Fast enumerate** (`resolve_ytdl_playlist`):

```
yt-dlp --flat-playlist -j <page_url>
```

- Runs async via `tokio::process::Command`
- Parses stdout line-by-line as `YtdlFlatEntry` JSON (serde)
- Maps to `Track { path: webpage_url, stream: true }` — the path is still a page URL, not audio
- Fallback chain for fields: `webpage_url` -> `url` -> skip; `title` -> humanize(basename) -> url; `uploader` -> `playlist_uploader`

**Phase 2 — Lazy download** (`resolve_ytdl_track`):

```
yt-dlp -f "bestaudio[protocol=https]/bestaudio[protocol=http]/bestaudio" \
       --no-playlist --print-json \
       -o "<tmpdir>/%(id)s.%(ext)s" <page_url>
```

- Triggered on-demand when user selects a track for playback
- Creates a temp directory (`/tmp/cliamp-ytdl-*`) registered in `YtdlTempTracker`
- Parses stdout as `YtdlFullEntry` for metadata; falls back to `find_first_file(tmpdir)` for the audio file path
- Returns `Track { path: local_file, stream: false }` — now seekable local audio
- UI shows "Buffering..." during download

**Temp cleanup**: `YtdlTempTracker` (Arc<Mutex<Vec<PathBuf>>>) tracks all temp dirs. Cleanup runs on: normal exit, ctrlc signal handler, and Drop impl. We do NOT use `tempfile::TempDir` because files must persist for the duration of playback.

**Non-goals**: Caching resolved tracks across sessions, parallel downloads, progress reporting.

## Consequences

- Good, because playlist enumeration is near-instant (~1s for flat-playlist)
- Good, because audio is only downloaded when actually played — saves bandwidth and disk
- Good, because downloaded files are seekable local PCM (full position/duration support)
- Bad, because there's a download delay (2-10s) when first playing a yt-dlp track
- Bad, because temp files accumulate during playback (cleaned on exit)
- Bad, because yt-dlp must be installed separately (`brew install yt-dlp`)

## Implementation Plan

- **Affected paths**: `src/resolve/ytdl.rs` (both phases + temp tracker), `src/resolve/mod.rs` (URL routing), `src/ui/mod.rs` (buffering state + AppMsg handling)
- **Dependencies**: `serde`, `serde_json`, `tokio` (process feature)
- **Patterns to follow**: Phase 2 runs in `tokio::spawn`, sends `AppMsg::YtdlResolved` or `AppMsg::YtdlError` via channel. UI sets `buffering = true` during download. Preload is skipped for yt-dlp tracks (can't preload without downloading).
- **Patterns to avoid**: Do not download audio during Phase 1 (flat-playlist only). Do not preload yt-dlp tracks. Do not use `tempfile::TempDir` (auto-cleanup would delete files during playback).

### Verification

- [x] `YtdlFlatEntry` and `YtdlFullEntry` JSON parsing tests pass
- [x] `YtdlTempTracker` register/cleanup/Drop tests pass
- [x] `humanize_basename()` and `find_first_file()` tests pass
- [ ] `yt-dlp --flat-playlist -j <youtube_playlist>` returns Track list
- [ ] Selecting a yt-dlp track triggers Phase 2 download and plays the local file
- [ ] Temp dirs are cleaned on exit and ctrlc

## Alternatives Considered

- **Stream directly via URL**: Use yt-dlp to extract direct audio URL, stream without downloading. Rejected because direct URLs are often short-lived and not seekable.
- **Download all tracks upfront**: Download entire playlist at load time. Rejected because it's slow and wastes bandwidth for skipped tracks.
- **Use yt-dlp as a library (pyo3)**: Embed Python yt-dlp via PyO3. Rejected because it adds Python as a runtime dependency and complicates the build.

## More Information

- Go implementation: `resolve/resolve.go:187-352`
- yt-dlp format selection: `bestaudio[protocol=https]` prefers HTTPS to avoid mixed-content issues
- Detection: `playlist::is_ytdl()` checks for youtube.com, youtu.be, soundcloud.com, bandcamp.com, and yt-dlp-supported domains
