# amp808

Rust port of [cliamp](https://github.com/bjarneo/cliamp) — a Winamp 2.x-inspired terminal music player.

## Development Workflow

### Required Skills

- **ADR** (`/adr`): Use before implementing any architectural decision. Create ADRs for audio backend choice, async model, yt-dlp integration strategy, DSP pipeline design, etc.
- **TDD** (`/tdd`): All modules must be built test-first using red-green-refactor. Write failing tests before implementation.

### Architecture Decision Records

ADRs live in `adr/` at the project root. Consult existing ADRs before making changes.
Run `/adr` to create or update records.

### Testing

- `cargo test` must pass before committing
- Use `/tdd` for feature development
- Unit tests for pure logic (playlist, config, EQ math, yt-dlp JSON parsing)
- Integration tests for subprocess mocking (yt-dlp, ffmpeg)

### Build & Run

- `cargo build` / `cargo run -- <args>`
- `cargo clippy` for lints
- `cargo fmt` for formatting

### Key Conventions

- Error handling: `anyhow` for application errors, `thiserror` for library errors
- Async: tokio for I/O tasks only; audio thread runs on cpal (not tokio)
- Config: `~/.config/amp808/config.toml` (serde + toml)
- Temp files: `/tmp/amp808-ytdl-*` cleaned on exit via RAII + signal handler

### Go Reference Codebase

The original Go source lives at `/Users/alan/Projects/cliamp-main/` for reference during porting.
