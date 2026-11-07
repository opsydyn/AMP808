---
status: accepted
date: 2026-11-07
decision-makers: alan
consulted: ADR-0005, ADR-0022, ADR-0031, yt-dlp command-line documentation
informed: AMP808 contributors
---

# Run yt-dlp behind a cancellable desktop transfer adapter

## Context and Problem Statement

ADR-0031 established a provider-agnostic remote acquisition state model and a deliberately fake
GPUI preview. The next desktop slice needs a real boundary around the user-installed `yt-dlp`
binary. The integration must be able to produce a complete local media file, report progress, and
stop active work without copying the Ratatui application or exposing subprocess details to GPUI.

The Ratatui implementation downloads into tracked temporary directories and optionally copies the
current result into the user's music folder. AMP808-R3TR0 needs a smaller adapter first: transfer
one URL directly into an explicit destination supplied by its caller, while leaving destination
selection and playback hand-off for later slices.

## Decision

Add a private `yt_dlp` module to `apps/desktop` using only the Rust standard library.

- `YtDlpTransfer::start` accepts one source URL, one existing destination directory, and an
  executable path that defaults to `yt-dlp` in production.
- Trim the source and reject anything that is not an explicit `http://` or `https://` URL before
  process startup, preventing option-shaped input from becoming a `yt-dlp` argument.
- Invoke the executable with `std::process::Command`; never pass arguments through a shell.
- Pass `--ignore-config` so user or system configuration cannot silently change the integration's
  destination, playlist, output, or authentication behavior.
- Pass `--no-playlist`, the existing best-audio format preference, an explicit `--paths` value, a
  bounded filename template, `--newline`, a machine-readable `--progress-template`, and
  `--print after_move:...` for the actual final path.
- A dedicated worker owns the subprocess. Typed events cross a standard-library channel:
  `Progress`, `Completed`, and `Failed`. GPUI and the remote job model do not import `Child`, pipes,
  exit statuses, or stdout protocol strings.
- A cloneable cancellation handle sets an atomic request. The worker polls `Child::try_wait`, kills
  an active child after cancellation, waits for termination, and emits `Cancelled`.
- Validate that the destination exists and is a directory before spawning. Validate the reported
  final path as an existing regular file contained within the canonical destination before
  reporting success.
- Treat complete-file output as a user-operated `yt-dlp` integration. This adapter does not grant,
  infer, or validate media rights; public release terms remain required by ADR-0031.

The first adapter slice is exercised only with fake executables. It performs no real provider
request during automated qualification and is not wired to the visible GPUI action yet.

## Consequences

- Good, because process ownership, progress parsing, output validation, and cancellation are
  testable without a provider or network.
- Good, because direct argv construction and ignored external configuration reduce shell and
  destination-surprise risks.
- Good, because complete local output is a first-class result instead of an undocumented temporary
  playback artifact.
- Bad, because a dedicated thread and polling loop add lifecycle code that must be shut down
  correctly.
- Bad, because progress-template fields are an external CLI protocol and may require maintenance as
  `yt-dlp` evolves.
- Bad, because the adapter initially exists below the UI and therefore does not yet give users a
  working URL-entry flow.

## Boundary Review

- **yt-dlp adapter** owns argv, process lifetime, stdout/stderr parsing, cancellation, and output
  containment validation.
- **Remote acquisition model** owns provider-independent lifecycle state and invariants.
- **Desktop UI** will later own URL entry, destination selection, user intent, and presentation.
- **Playback** will later consume a validated local result and continues to own decoding.
- **Local library** does not automatically import or persist results in this slice.

Allowed future dependency direction is UI coordinator -> adapter events -> remote job, followed by
an explicit validated local-path hand-off to playback. The adapter must not import GPUI, playback,
SQLite, library types, or FFmpeg. Verdict: **safe with noted risk**; the later UI coordinator must
preserve cancellation and must not treat a subprocess path as trusted before adapter validation.

## Non-goals

- No visible URL field, folder picker, Terms and Conditions screen, or GPUI process coordinator.
- No real SoundCloud, YouTube, Bandcamp, or other provider network qualification.
- No playlist enumeration, cookies, credentials, browser-session extraction, arbitrary user args,
  proxy settings, or provider-specific policy.
- No FFmpeg post-processing, forced audio conversion, metadata embedding, artwork retrieval, or
  format normalization.
- No automatic playback, queue insertion, library import, persistent history, or retry policy.
- No shared crate or dependency from the desktop application to the Ratatui application.

## Implementation Plan

- **Affected paths**:
  - `apps/desktop/src/main.rs`: register the private adapter module.
  - `apps/desktop/src/yt_dlp.rs`: request, validated result, typed events, error type, command
    construction, output parsing, worker, and cancellation.
  - `adr/README.md`: index this decision.
- **Dependencies**: add none; use `std::process`, `std::thread`, `std::sync`, and existing
  `tempfile` only in tests.
- **Patterns to follow**: keep the concrete adapter private like `remote.rs`; use fake executable
  integration tests; use argv rather than shell command strings; keep errors structured.
- **Patterns to avoid**: do not copy the TUI tracker, add Tokio to the desktop, block GPUI, trust a
  printed path outside the destination, or invoke a real provider in tests.

### Verification

- [x] The generated argv ignores external config, disables playlists, selects best audio, pins the
  destination and bounded output template, and requests progress plus the final moved path.
- [x] A fake executable can report determinate progress and complete with a validated local file.
- [x] Missing executable, non-zero exit, malformed progress, missing output, and escaped output
  paths produce structured events without panics.
- [x] Cancellation terminates a long-running fake process and emits `Cancelled` once.
- [x] The adapter imports no GPUI, playback, library, SQLite, Tokio, serde, or FFmpeg types.
- [x] Desktop base and projectM tests, strict test-target Clippy, formatting, and diff checks pass.

## More Information

Qualification on 2026-11-07 used local fake executables only. The base desktop suite passed 135
tests; the projectM-enabled suite passed 137 tests; strict all-feature test-target Clippy,
formatting, and whitespace checks passed. No real provider request was made, and `yt-dlp` was not
installed on the qualification host, so live SoundCloud or other provider behavior remains an
explicit later acceptance gate.

### 2026-11-07 UI coordinator follow-up

The accepted boundary now has a visible GPUI coordinator. `USE YT-DLP` opens an accessible
HTTP(S)-source form, validates the source before process startup, and asks the user for exactly one
destination directory. A private `remote_transfer` coordinator translates typed adapter events
into ADR-0031's provider-neutral lifecycle; GPUI polls the adapter channel without blocking the UI
thread, and cancellation remains pending until the adapter confirms process termination. Success
exposes only the adapter-validated local path. It does not imply playback, queue insertion, library
import, FFmpeg conversion, or a successful live-provider qualification.

The follow-up qualification passed 142 base desktop tests and 144 projectM-enabled tests. Tests
cover the accessible form, directory picker, invalid option-shaped input, determinate progress,
validated completion, confirmed cancellation, failure, and retry. No live provider request was
made; the host still has no verified `yt-dlp` installation.

## Options Considered

### Copy the Ratatui resolver and temporary-file tracker

This is proven prior art and includes playback-oriented cleanup, but it would copy Tokio, playlist,
temporary-session, and application-message assumptions into a decoupled desktop product.

### Add Tokio and reuse asynchronous process handling

Tokio offers convenient process futures, but the desktop currently has no Tokio runtime. Adding a
runtime for one subprocess would broaden dependency and shutdown ownership before it is needed.

### Run a blocking command on the GPUI background executor

This is a shorter implementation, but it cannot provide responsive cancellation and may occupy a
framework executor thread for the duration of a long transfer.

### Use a dedicated standard-library worker

Chosen because it isolates blocking process I/O, adds no dependency, supports cancellation, and
keeps the adapter independent of GPUI and playback.
