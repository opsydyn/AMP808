---
status: accepted
date: 2026-03-01
decision-makers:
---

# Album art display via ratatui-image

## Context and Problem Statement

cliamp plays local audio files that often contain embedded cover art (ID3v2 APIC in MP3, METADATA_BLOCK_PICTURE in FLAC/OGG). Displaying album art in the terminal would enhance the player experience. Modern terminals support image protocols (Sixel, Kitty, iTerm2) and there are fallback approaches (halfblocks, Unicode braille) for terminals that don't.

## Decision

Use `ratatui-image` for terminal image rendering and Symphonia's existing metadata API for cover art extraction.

Key design choices:

1. **Symphonia metadata extraction**: The Symphonia probe already reads metadata (visuals are available via `MetadataRevision::visuals()`). We extract the `FrontCover` visual (falling back to the first visual) as a `CoverArt { data: Vec<u8>, media_type: String }` during `decode_symphonia()`. Two metadata locations are checked: `ProbeResult.metadata` (pre-probe, e.g., ID3v2) and `format.metadata()` (post-probe, container metadata).

2. **`ratatui-image` with protocol auto-detection**: `Picker::from_query_stdio()` queries the terminal at startup and selects the best available protocol (Sixel > Kitty > iTerm2 > halfblocks). The `Protocol` enum holds the rendered image state. `Image::new(&Protocol)` is a standard Ratatui widget.

3. **Art threaded through `Arc<RwLock<Option<CoverArt>>>`**: The decode thread writes cover art to a shared lock. The UI polls it once per track in `on_tick()`, decodes the image bytes via `image::load_from_memory()`, and creates a `Protocol` instance sized to the render area.

4. **Spectrum-side placement anchored to status column**: When art is available and display is enabled, the main view spectrum region splits horizontally: `[Min(40), Length(24)]`. Art stays in the right-side visualizer column and is vertically anchored under the play-status row for cleaner alignment. In 808 mode, art remains in the existing spectrum split behavior.

5. **Toggle with `c` key**: Users can hide/show cover art. No art is fetched for HTTP streams or FFmpeg-decoded files — only Symphonia-probed local files provide embedded visuals.

6. **Feature gating via Cargo features**: `ratatui-image` is included with `default-features = false, features = ["image-defaults", "crossterm"]` to avoid requiring the `chafa` C library. This means only protocol-native rendering (Sixel/Kitty/iTerm2) and halfblock fallback are available.

## Consequences

* Good, because album art displays automatically for local files with embedded cover art — no user configuration needed
* Good, because protocol auto-detection means it works across terminal emulators without user intervention
* Good, because the `c` toggle and graceful fallback (full-width spectrum) mean the feature is non-intrusive
* Bad, because HTTP streams and FFmpeg-decoded files don't provide cover art — only Symphonia-probed local files
* Bad, because terminals without image protocol support fall back to halfblocks which are low fidelity
* Neutral, because cover art is decoded in the UI thread (once per track) — acceptable since `image::load_from_memory` is fast for typical cover art sizes (~100-500KB)

## Implementation Plan

* **Affected paths**: `src/player/decode.rs` (cover extraction), `src/player/mod.rs` (cover state lifecycle), `src/ui/mod.rs` (protocol init + per-track polling), `src/ui/view.rs` and `src/ui/view_808.rs` (placement/render), `src/ui/keys.rs` (`c` toggle)
* **Dependencies**: `ratatui-image` (protocol-backed widget), `image` (decode bytes to image), existing Symphonia metadata APIs
* **Patterns to follow**: Extract `FrontCover` first then fallback visual; carry art via `Arc<RwLock<Option<CoverArt>>>`; draw art only when toggle is enabled and protocol is available; gracefully fall back to full-width visualizer when art is missing.
* **Patterns to avoid**: Do not fetch cover art from network during playback; do not block audio callback for image decode/render work; do not hard-fail UI when protocol negotiation is unavailable.

### Verification

* [x] Local files with embedded visuals populate `CoverArt` from Symphonia metadata probe path
* [x] UI creates a `ratatui-image` protocol instance and renders only when available
* [x] `c` key toggles art visibility without affecting playback
* [x] Main view keeps art in spectrum-side column aligned under playback status text
* [ ] Manual validation: confirm protocol behavior on at least one native image terminal and one fallback terminal

## Alternatives Considered

* **Fetch cover art from web APIs (MusicBrainz, Discogs)**: Would provide art for streams and files without embedded art, but adds network dependency, API keys, and complexity. Can be layered on later.
* **`viuer` crate**: Simpler API but doesn't integrate with Ratatui's widget system — would require manual cursor positioning and wouldn't compose with the layout.
* **`chafa`-based rendering**: Higher quality ASCII/braille art, but requires the chafa C library to be installed, adding a system dependency.
* **Extract art via FFmpeg**: Could pipe `ffmpeg -i file -an -vcodec copy -f image2 -` to extract art from any format, but adds subprocess overhead and Symphonia already handles the common cases natively.

## More Information

* ADR-0003 defines Symphonia/FFmpeg decode routing, which limits where embedded visuals are available
* `ratatui-image` docs: protocol selection via terminal query (Sixel/Kitty/iTerm2/halfblocks)
