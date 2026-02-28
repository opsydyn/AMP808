---
status: accepted
date: 2026-02-28
decision-makers: alan
---

# Use tokio for async I/O with dedicated cpal audio thread

## Context and Problem Statement

CLIAMP-RS has two fundamentally different concurrency needs:

1. **Audio playback**: Real-time, low-latency callback driven by cpal (~5ms buffer). Must never block. Runs on a dedicated OS thread managed by cpal.
2. **I/O tasks**: yt-dlp subprocess resolution, HTTP feed/M3U fetching, crossterm terminal events. These are I/O-bound and benefit from async/await.

The Go version uses goroutines for everything. In Rust, we need to choose between sync threads, async runtime, or a hybrid.

## Decision

Use **tokio** (full features) for all I/O-bound work. The cpal audio thread runs independently — it is NOT managed by tokio.

**Architecture**:
- `main()` is `#[tokio::main]` — the tokio runtime owns the event loop
- The TUI event loop uses `tokio::select!` to multiplex:
  - `tokio::time::sleep(50ms)` tick intervals
  - `mpsc::UnboundedReceiver<AppMsg>` for async task results
  - `crossterm::event::poll()` for terminal key events (sync, polled during tick)
- Async tasks (`tokio::spawn`) handle: yt-dlp subprocess, feed resolution, M3U fetching
- The cpal output stream runs on its own OS thread (cpal manages this)
- Communication between async tasks and the App is via `mpsc::unbounded_channel<AppMsg>`

**Non-goals**: Running the audio callback on tokio. Using async for file I/O (decode is sync and fast enough).

## Consequences

* Good, because yt-dlp resolution and HTTP fetching don't block the UI
* Good, because `tokio::select!` cleanly multiplexes events, messages, and ticks
* Good, because the audio thread has no async overhead — pure real-time processing
* Bad, because crossterm events are sync and must be polled (not truly async in `select!`)
* Neutral, because `tokio = { features = ["full"] }` adds compile time but we need process, time, sync, and net features

## Implementation Plan

* **Affected paths**: `src/main.rs` (`#[tokio::main]`), `src/ui/mod.rs` (event loop with `tokio::select!`), `src/resolve/ytdl.rs` (`tokio::process::Command`), `src/resolve/mod.rs` (async feed/M3U resolution)
* **Dependencies**: `tokio = { version = "1", features = ["full"] }`
* **Patterns to follow**: All I/O tasks use `tokio::spawn` and send results via `AppMsg` channel. The `App` struct is NOT `Send` — it lives on the main thread and processes messages synchronously.
* **Patterns to avoid**: Do not use `tokio::spawn_blocking` for decode (it's fast enough sync). Do not pass `&Player` across await points (it's not `Send`). Do not use async in the cpal callback.

### Verification

- [x] `#[tokio::main]` compiles and runs
- [x] yt-dlp resolution runs in `tokio::spawn` and sends `AppMsg::YtdlResolved`
- [x] Feed resolution runs async and sends `AppMsg::FeedsLoaded`
- [x] TUI event loop processes key events, async messages, and ticks via `tokio::select!`
- [ ] UI remains responsive during yt-dlp download (buffering indicator shown)

## Alternatives Considered

* **Pure sync with std::thread**: Spawn OS threads for I/O tasks. Rejected because managing thread pools, channels, and timeouts manually is error-prone — tokio handles this.
* **async-std**: Alternative async runtime. Rejected because tokio has broader ecosystem support (reqwest, tokio::process) and is the de facto standard.
* **smol**: Lightweight async runtime. Rejected because we need `tokio::process::Command` for yt-dlp subprocess and reqwest for HTTP — both are tokio-native.

## More Information

- ADR-0002 covers cpal audio thread architecture
- The Go version uses goroutines + `tea.Cmd` (Bubbletea's async command pattern) — our `AppMsg` channel is the equivalent
