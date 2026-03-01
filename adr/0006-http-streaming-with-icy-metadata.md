---
status: accepted
date: 2026-03-01
decision-makers:
---

# HTTP streaming with ICY metadata extraction

## Context and Problem Statement

The Go version of cliamp supports playing audio from HTTP URLs (internet radio, podcast streams, Navidrome tracks). The Rust port needs equivalent streaming playback, including transparent extraction of SHOUTcast/Icecast ICY metadata (live stream titles).

Key constraints:
- Symphonia's `MediaSource` trait requires `Read + Seek + Send + Sync`
- HTTP streams are not seekable
- ICY metadata is interleaved in the audio byte stream (every N bytes)
- The audio decode runs on a blocking thread, but the TUI runs on a tokio async runtime
- cpal's `Stream` is `!Send + !Sync`, so the `Player` struct cannot be moved across threads

## Decision

Layer three components to handle HTTP streaming:

1. **`HttpMediaSource`** — implements Symphonia's `MediaSource` trait. Uses `reqwest::blocking::Client` (not async) with `Icy-MetaData: 1` request header. Returns `is_seekable() = false`, `byte_len() = None`. When the server responds with `Icy-Metaint`, wraps the response body in `IcyReader`.

2. **`IcyReader<R: Read>`** — transparent `Read` wrapper that strips interleaved ICY metadata blocks. Clamps each read to not cross a metadata boundary. At each boundary, reads the 1-byte length prefix (x16), parses `StreamTitle='...'` from the null-padded block, and fires a callback. The callback writes to a shared `Arc<RwLock<String>>` that the UI polls.

3. **`StreamingSource`** — implements `AudioSource` for on-demand packet decoding from a Symphonia `FormatReader`. Unlike `PcmSource` (which loads entire files into memory), this decodes packets as they arrive. Returns `len_frames() = None` and `seek() = Err`. Handles chained OGG bitstreams by catching Symphonia's `Error::ResetRequired` and re-creating the decoder.

For bridging blocking HTTP with async TUI: `Player::play_async()` clones individual `Arc`-wrapped fields (not the whole Player, since cpal::Stream is !Send) and spawns a `tokio::task::spawn_blocking` that performs the HTTP connect + Symphonia probe + decode setup. Completion is signaled via `AppMsg::StreamPlayed`.

## Consequences

* Good, because the same `AudioSource` trait works for both local files (seekable, known length) and HTTP streams (non-seekable, unknown length)
* Good, because ICY metadata extraction is transparent to the decode pipeline — `IcyReader` wraps the raw response body before Symphonia ever sees it
* Good, because chained OGG (Icecast radio song changes) works by handling `ResetRequired` in `StreamingSource`
* Bad, because blocking HTTP in `spawn_blocking` ties up a thread pool thread for the stream's lifetime — acceptable for a CLI player with one active stream
* Neutral, because `reqwest::blocking` adds the `json` feature for Navidrome API, increasing binary size slightly

## Implementation Plan

* **Affected paths**: `src/player/http.rs`, `src/player/icy.rs`, `src/player/stream.rs`, `src/player/decode.rs`, `src/player/mod.rs`, `src/ui/mod.rs`
* **Dependencies**: `reqwest` (blocking client), `symphonia` (probe/decode), `tokio` (`spawn_blocking` bridge)
* **Patterns to follow**: Keep HTTP decode path synchronous (`Read`-based) and non-seekable; parse ICY metadata in `IcyReader`; update stream title via shared `Arc<RwLock<String>>`; surface stream setup completion through `AppMsg::StreamPlayed`.
* **Patterns to avoid**: Do not buffer full HTTP streams in memory; do not make `Player` cross-thread `Send`; do not introduce async reads into Symphonia's blocking decode path.

### Verification

* [x] `HttpMediaSource` reports non-seekable contract (`is_seekable() = false`, `byte_len() = None`) with tests
* [x] `IcyReader` strips metadata blocks and extracts `StreamTitle` with unit tests
* [x] `StreamingSource` keeps stream semantics (`len_frames() = None`, `seek()` errors) and handles packet decode loop
* [x] Async-to-blocking bridge sends `AppMsg::StreamPlayed` from `Player::play_async()`
* [ ] Manual validation: play a live ICY stream and observe live title updates in the UI

## Alternatives Considered

* **Async HTTP with `reqwest` + `tokio::io`**: Would avoid blocking a thread, but Symphonia's `FormatReader` requires synchronous `Read`, making this impractical without a bridge layer that adds complexity for no user-visible benefit.
* **Decode entire HTTP response into memory**: Simpler but defeats the purpose of streaming — radio stations are infinite streams.
* **Separate process for HTTP (like ffmpeg)**: ffmpeg already handles HTTP for its formats (m4a, aac, opus). Using it for all HTTP would simplify code but lose Symphonia's native decode quality for mp3/flac/ogg and prevent ICY metadata extraction.

## More Information

* ADR-0003 defines decode routing and FFmpeg fallback strategy
* ADR-0004 defines async architecture and `tokio::spawn`/`spawn_blocking` boundaries
