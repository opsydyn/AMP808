---
status: accepted
date: 2026-11-06
decision-makers: alan
consulted: ADR-0008, ADR-0026, AMP808-R3TR0 desktop architecture
informed: AMP808 contributors
---

# Cache embedded artwork for local-library thumbnails

## Context and Problem Statement

ADR-0026 deliberately excluded artwork from the first persistent local-catalogue slice. The
desktop player can now render embedded artwork for the active track, but library results still use
text-only rows. Re-probing every file during every render or search would block interaction and
repeat media I/O, while storing image blobs in SQLite would mix presentation assets into the
catalogue's metadata ownership.

How should AMP808-R3TR0 show responsive local-library thumbnails without changing the catalogue
schema, blocking GPUI or CPAL, or introducing network artwork lookup?

## Decision

Add a bounded, session-scoped artwork cache owned by the desktop UI adapter.

- A cache key combines canonical track path, indexed file size, and indexed modified time. A
  changed file therefore receives a new key without requiring explicit cache invalidation.
- Cache entries distinguish loading, embedded artwork, and no usable artwork. Negative results are
  retained so files without artwork are not repeatedly probed.
- The UI schedules a bounded batch from the current library result set on GPUI's background
  executor. Media probing and image-byte extraction never run in render or on the audio callback.
- The ready cache stores GPUI in-memory images created from the existing bounded `CoverArt` bytes.
  Rows render them cropped into a 48-pixel square with an accessible image label. Missing and
  unsupported artwork render a deterministic decorative placeholder.
- The cache has a fixed entry limit and evicts the oldest non-loading entry when required. It is
  discarded on application exit.

The existing SQLite schema remains unchanged. Embedded files remain the source of truth, and the
catalogue exposes only the existing size/mtime fingerprint needed by this UI adapter.

## Consequences

- Good, because repeat searches and rerenders reuse positive and negative results.
- Good, because catalogue, playback, and presentation ownership remain separate.
- Good, because changed files naturally miss the previous key after a successful rescan.
- Bad, because artwork is re-probed after each application launch.
- Bad, because original encoded image bytes are decoded for a small display rather than being
  resized and persisted ahead of time.
- Neutral, because only a bounded prefix of the current flat result list is loaded in this slice;
  list virtualization can later request additional visible rows.

## Boundary Review

- **Local catalogue** owns path identity and the persisted file fingerprint, not artwork bytes.
- **Media probe** owns bounded embedded-art extraction.
- **Desktop UI** owns request state, GPUI images, eviction, accessibility, and fallbacks.
- **Playback** remains unaware of the catalogue and thumbnail cache.

Allowed dependencies are UI -> catalogue value type and UI -> media probe. The catalogue does not
import GPUI or image types. Verdict: **safe with noted risk**; decoded images consume bounded
session memory, controlled by the fixed entry limit.

## Non-goals

- No SQLite artwork column, schema migration, disk cache, or application-support image files.
- No network lookup, MusicBrainz, Discogs, Spotify, Apple Music, or provider artwork.
- No new image-decoding or resizing dependency.
- No filesystem watcher, eager whole-library scan, album grouping, or list virtualization.
- No change to now-playing artwork, playback, queue, playlist, or search contracts.

## Implementation Plan

- `apps/desktop/src/library.rs`: include the existing size and modified-time columns in
  `LibraryTrack` query results and expose a crate-private immutable fingerprint.
- `apps/desktop/src/media.rs`: expose a metadata-only helper that returns bounded embedded art.
- `apps/desktop/src/ui.rs`: add the bounded cache, background batch scheduling, accessible 48-pixel
  thumbnail renderer, and deterministic fallback.
- Tests proceed as vertical TDD slices: cache request/negative/stale behavior first, then GPUI row
  rendering and accessibility, then native macOS validation with art and no-art local tracks.

### Verification

- [x] A current track key is requested once while loading and not requested again after art or a
      no-art result is stored.
- [x] A changed size or modified time creates a distinct key and is requested again.
- [x] Cache growth remains within the fixed entry limit.
- [x] Artwork probing occurs on a background executor, not in GPUI render or CPAL callbacks.
- [x] A ready image renders as a cropped 48-pixel library thumbnail with an accessible label.
- [x] A missing or unsupported image renders a stable placeholder and keeps the play action usable.
- [x] Existing desktop tests, projectM feature tests, focused strict Clippy, and formatting pass.
- [x] Native macOS acceptance observes one art thumbnail and one no-art fallback.

## Options Considered

### Persist thumbnails beside the SQLite database

This would improve cold-launch behavior, but requires a cache directory contract, atomic writes,
format/version migration, byte-budget cleanup, and recovery rules. Those costs are not justified
until measured libraries show session reprobe latency is material.

### Store artwork blobs in SQLite

This simplifies lookup but couples presentation payloads to metadata transactions, inflates backup
and query storage, and contradicts the catalogue boundary.

### Probe artwork directly while rendering each row

This has the smallest code diff but blocks GPUI and repeats file I/O on every render. It is rejected.

## More Information

- ADR-0008 defines embedded-art selection and the no-network policy.
- ADR-0026 owns the persistent local catalogue and explicitly deferred artwork to a follow-up.
- Revisit persistent resized thumbnails only after measuring cold-launch and large-library behavior.

Verification on 2026-11-06: 109 locked base tests and 111 projectM-enabled tests passed, together
with strict all-feature desktop-test Clippy and formatting. Native macOS acceptance rendered the
embedded `Cosmic Cover Trial` image at 48 pixels, exposed `Album artwork for Cosmic Cover Trial` to
the accessibility tree, and kept musical-note fallbacks plus play/queue actions for no-art tracks.
The exact temporary track copy was removed afterward and its already-registered root rescanned back
from seven to six catalogue tracks; the original acceptance MP3 was retained.
