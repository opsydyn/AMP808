# Architecture Decision Records (ADR)

An Architecture Decision Record (ADR) captures an important architecture decision along with its context and consequences.

## Conventions

- Directory: `adr`
- Naming:
  - Prefer numbered files when starting fresh: `0001-choose-database.md`
  - If the repo already uses slug-only names, keep that: `choose-database.md`
- Status values: `proposed`, `accepted`, `rejected`, `deprecated`, `superseded`

## Workflow

- Create a new ADR as `proposed`.
- Discuss and iterate.
- When the team commits: mark it `accepted` (or `rejected`).
- If replaced later: create a new ADR and mark the old one `superseded` with a link.

## ADRs

- [ADR-0001: Adopt architecture decision records](0001-adopt-architecture-decision-records.md) (accepted, 2026-02-28)
- [ADR-0002: Use cpal directly for audio output](0002-use-cpal-direct-for-audio-output.md) (accepted, 2026-02-28)
- [ADR-0003: Symphonia decode with FFmpeg fallback](0003-symphonia-decode-with-ffmpeg-fallback.md) (accepted, 2026-02-28)
- [ADR-0004: Tokio async with dedicated cpal audio thread](0004-tokio-async-with-cpal-audio-thread.md) (accepted, 2026-02-28)
- [ADR-0005: Two-phase yt-dlp resolution](0005-two-phase-ytdlp-resolution.md) (accepted, 2026-02-28)
- [ADR-0006: HTTP streaming with ICY metadata](0006-http-streaming-with-icy-metadata.md) (accepted, 2026-03-01)
- [ADR-0007: Provider trait for external music services](0007-provider-trait-for-external-music-services.md) (accepted, 2026-03-01)
- [ADR-0008: Album art display via ratatui-image](0008-album-art-display-via-ratatui-image.md) (accepted, 2026-03-01)
- [ADR-0009: Oscilloscope visualizer mode](0009-oscilloscope-visualizer-mode.md) (proposed, 2026-03-01)
- [ADR-0010: Tachyonfx animated 808 chrome and visualizer parity](0010-tachyonfx-animated-808-chrome-and-visualizer-parity.md) (accepted, 2026-03-02)
- [ADR-0011: Add inline browser-backed runtime playlist loading](0011-in-app-command-mode-for-runtime-playlist-loading.md) (proposed, 2026-03-02)
- [ADR-0012: Add a macOS Music.app playback backend](0012-add-macos-music-app-playback-backend.md) (accepted, 2026-03-07)
- [ADR-0013: Use the Apple Music API as a metadata-only integration](0013-use-apple-music-api-as-a-metadata-only-integration.md) (superseded, 2026-03-07)
- [ADR-0014: Pause Apple Music API rollout and use synthetic visualizers for Music.app parity](0014-pause-apple-music-api-rollout-and-use-synthetic-visualizers.md) (accepted, 2026-03-07)
