---
status: accepted
date: 2026-02-28
decision-makers: alan
---

# Use Symphonia for native decoding with FFmpeg subprocess fallback

## Context and Problem Statement

CLIAMP-RS must decode multiple audio formats: MP3, FLAC, WAV, OGG (native priority) and M4A, AAC, OPUS, WMA, ALAC (codec-licensed formats). The Go version uses gopxl/beep decoders for native formats and shells out to FFmpeg for the rest.

We need a decode strategy that:
1. Handles common formats without external dependencies
2. Falls back gracefully for codec-licensed formats
3. Produces in-memory seekable PCM for the gapless playback engine
4. Resamples to 44100 Hz at decode time

## Decision

Use `symphonia 0.5` (all features) for native decoding of MP3, FLAC, WAV, and OGG. Fall back to FFmpeg subprocess (`ffmpeg -i <path> -f f32le -ar 44100 -ac 2 pipe:1`) for M4A, AAC, OPUS, WMA, and ALAC.

**Decode pipeline**:
1. `format_ext()` determines the file extension (handles URLs with query params)
2. Extensions in `FFMPEG_EXTS` (`m4a`, `aac`, `m4b`, `alac`, `wma`, `opus`) go directly to FFmpeg
3. All other extensions try Symphonia first, with FFmpeg as a fallback on error
4. Both paths produce `PcmSource` — a `Vec<[f32; 2]>` with full seek support
5. Resampling (linear interpolation) is applied if source sample rate != 44100 Hz

**Non-goals**: Streaming decode (we decode entire files into memory), hardware-accelerated decode, multichannel (>2ch) support.

## Consequences

* Good, because MP3/FLAC/WAV/OGG decode natively without FFmpeg installed
* Good, because FFmpeg fallback covers every format FFmpeg supports
* Good, because `PcmSource` (in-memory) gives instant seek and known duration
* Bad, because decoding entire files into memory uses significant RAM for long files (~40MB for a 10-min FLAC)
* Bad, because FFmpeg subprocess has startup latency (~100ms) and requires FFmpeg installed
* Bad, because linear interpolation resampling is lower quality than polyphase — acceptable for a terminal player

## Implementation Plan

* **Affected paths**: `src/player/decode.rs` (Symphonia + routing), `src/player/ffmpeg.rs` (subprocess), `src/player/source.rs` (AudioSource trait)
* **Dependencies**: `symphonia = { version = "0.5", features = ["all"] }`
* **Patterns to follow**: All decoders return `Box<dyn AudioSource>`. The `AudioSource` trait provides `read()`, `len_frames()`, `position()`, `seek()`, `seekable()`, `sample_rate()`.
* **Patterns to avoid**: Do not stream-decode (the gapless engine needs seekable sources). Do not use symphonia's resampler (we do our own for simplicity).

### Verification

- [x] `decode_file("test.mp3", 44100)` returns a seekable PcmSource via Symphonia
- [x] `decode_file("test.m4a", 44100)` routes to FFmpeg and returns PcmSource
- [x] FFmpeg unavailable produces a clear error message
- [x] Resampling from 48000 Hz to 44100 Hz produces correct output length
- [x] PcmSource seek/position/len_frames are correct (unit tests passing)

## Alternatives Considered

* **Symphonia only**: Skip FFmpeg entirely. Rejected because Symphonia doesn't decode AAC/ALAC/OPUS natively (licensing).
* **FFmpeg only**: Decode everything via subprocess. Rejected because it adds a hard dependency on FFmpeg for common formats (MP3/FLAC) that Symphonia handles natively.
* **GStreamer bindings**: Full multimedia framework. Rejected as too heavy — we only need audio decode, not a pipeline framework.

## More Information

- ADR-0002 covers the audio output (cpal direct)
- The Go version uses the same dual strategy: native beep decoders + FFmpeg fallback
- FFmpeg command: `ffmpeg -i <path> -f f32le -ar 44100 -ac 2 -loglevel error pipe:1`
