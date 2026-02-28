---
status: accepted
date: 2026-02-28
decision-makers: alan
---

# Use cpal directly for audio output

## Context and Problem Statement

CLIAMP-RS needs a real-time audio output backend for stereo PCM playback at 44100 Hz. The Go version uses gopxl/beep which wraps oto (a cpal-equivalent). In Rust, the main options are cpal (low-level cross-platform audio), rodio (high-level wrapper around cpal), or kira (game audio engine).

We need direct control over the audio callback to implement a custom DSP pipeline: GaplessSource -> 10-band EQ -> Volume/Mono -> Tap (visualizer capture) -> interleaved output. The pipeline runs in the audio callback thread, separate from the tokio async runtime.

## Decision

Use `cpal 0.15` directly, without rodio or kira wrappers.

- Open a single `f32` stereo output stream at 44100 Hz
- The audio callback owns the DSP chain: reads from GaplessSource, applies EQ filters, volume, and writes to the tap ring buffer
- Shared state (volume dB, EQ band gains, mono flag) is behind `Arc<Mutex<PlayerState>>` — the lock is held only for fast copies of numeric values
- GaplessSource uses `try_lock` in the callback — fills silence on contention rather than blocking the audio thread
- The cpal stream is created once in `Player::new()` and kept alive for the session; only the source is swapped on track change

**Non-goals**: ASIO support, multi-device output, sample rate negotiation (we hardcode 44100 Hz and resample at decode time).

## Consequences

* Good, because full control over the audio callback enables zero-gap transitions and custom DSP
* Good, because no wrapper overhead — we process interleaved `[f32; 2]` frames directly
* Good, because cpal is the most widely used Rust audio crate with macOS/Linux/Windows support
* Bad, because more boilerplate than rodio (we write the callback, manage the stream lifetime)
* Bad, because no built-in resampling — we implement linear interpolation ourselves

## Implementation Plan

* **Affected paths**: `src/player/mod.rs`, `src/player/gapless.rs`, `src/player/eq.rs`, `src/player/volume.rs`, `src/player/tap.rs`
* **Dependencies**: `cpal = "0.15"`
* **Patterns to follow**: Audio callback must never block (use `try_lock`, pre-allocated buffers). All DSP operates on `&mut [[f32; 2]]` stereo frames.
* **Patterns to avoid**: Do not allocate in the audio callback. Do not hold locks across the callback boundary. Do not use tokio in the audio thread.

### Verification

- [x] `cargo build` compiles with cpal 0.15
- [x] Audio callback processes GaplessSource -> EQ -> Volume -> Tap pipeline
- [x] GaplessSource uses `try_lock` and fills silence on contention
- [x] Player state (volume, EQ, mono) is read via short-lived lock
- [ ] Local MP3/FLAC playback produces audible output on macOS

## Alternatives Considered

* **rodio**: Higher-level API with built-in decoders and mixer. Rejected because it doesn't expose the raw audio callback — we need custom DSP chain control and gapless source swapping.
* **kira**: Game audio engine with tweening and spatial audio. Rejected as overkill — we don't need spatial audio, and kira's abstraction model doesn't map to our pipeline.

## More Information

- ADR-0003 covers the decode strategy (Symphonia + FFmpeg fallback)
- The Go version's equivalent: `gopxl/beep` wrapping `oto` with a similar streamer pipeline
