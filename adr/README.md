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