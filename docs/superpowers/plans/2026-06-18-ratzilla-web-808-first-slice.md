# Ratzilla Web 808 First Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the first buildable AMP808 Ratzilla web slice with a web-safe core crate, explicit hosted-audio CORS policy behavior, analyser band mapping, and a minimal `WebGl2Backend` shell.

**Architecture:** Keep the native `amp808` package intact and add a workspace around it. Put browser-independent playback/source policy and analyser band shaping in `crates/amp808-core`; put all browser/Ratzilla/Web Audio glue in `apps/web`. The web app is wasm-only and built with Trunk, while root `cargo test` continues to cover native plus shared core behavior without compiling browser glue.

**Tech Stack:** Rust 2024 for the existing native package, Rust 2021 for small new crates unless upgraded later, Ratatui/Ratzilla `0.3.1`, Trunk, `wasm-bindgen`, `web-sys`, `HTMLAudioElement`, and Web Audio `AnalyserNode`.

---

## File Structure

- Modify `Cargo.toml` to add workspace metadata while preserving the existing root package.
- Create `crates/amp808-core/Cargo.toml` for web-safe pure Rust code.
- Create `crates/amp808-core/src/lib.rs` to expose core modules.
- Create `crates/amp808-core/src/web_audio.rs` for source policy, CORS error messaging, and analyser band mapping.
- Create `apps/web/Cargo.toml` for wasm/Ratzilla dependencies.
- Create `apps/web/Trunk.toml` for static web build configuration.
- Create `apps/web/index.html` as the Trunk entrypoint.
- Create `apps/web/src/main.rs` with a minimal Ratzilla `WebGl2Backend` shell that renders AMP808 WEB 808 and initial source status.
- Modify `README.md` to document the first web slice commands and CORS behavior after the shell builds.

## Task 1: Workspace And Core CORS Policy

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/amp808-core/Cargo.toml`
- Create: `crates/amp808-core/src/lib.rs`
- Create: `crates/amp808-core/src/web_audio.rs`

- [ ] **Step 1: Write the failing core CORS policy test**

Add this test-first file:

```rust
// crates/amp808-core/src/web_audio.rs

#[cfg(test)]
mod tests {
    use super::{HostedAudioIssue, WebAudioSource};

    #[test]
    fn hosted_url_cors_error_names_amp808_web_playback() {
        let message = HostedAudioIssue::CorsRequired.user_message();

        assert_eq!(
            message,
            "This hosted audio URL must allow CORS for AMP808 web playback."
        );
    }

    #[test]
    fn local_file_source_is_not_hosted() {
        let source = WebAudioSource::local_file("amen-break.wav");

        assert!(!source.is_hosted_url());
        assert_eq!(source.label(), "amen-break.wav");
    }

    #[test]
    fn hosted_url_source_is_hosted_and_uses_url_as_label() {
        let source = WebAudioSource::hosted_url("https://example.com/audio.mp3");

        assert!(source.is_hosted_url());
        assert_eq!(source.label(), "https://example.com/audio.mp3");
    }
}
```

- [ ] **Step 2: Add minimal crate files so the failing test can compile far enough**

```toml
# crates/amp808-core/Cargo.toml
[package]
name = "amp808-core"
version = "0.1.0"
edition = "2021"
license = "MIT"

[dependencies]
```

```rust
// crates/amp808-core/src/lib.rs
pub mod web_audio;
```

- [ ] **Step 3: Add the workspace members**

Append this to the root `Cargo.toml` without changing existing package metadata or dependencies:

```toml
[workspace]
members = [".", "crates/amp808-core"]
default-members = [".", "crates/amp808-core"]
resolver = "2"
```

- [ ] **Step 4: Run the test to verify RED**

Run:

```bash
cargo test -p amp808-core web_audio::tests::hosted_url_cors_error_names_amp808_web_playback
```

Expected: FAIL because `HostedAudioIssue` and `WebAudioSource` do not exist.

- [ ] **Step 5: Implement the minimal core policy**

Replace `crates/amp808-core/src/web_audio.rs` with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedAudioIssue {
    CorsRequired,
}

impl HostedAudioIssue {
    pub fn user_message(self) -> &'static str {
        match self {
            Self::CorsRequired => {
                "This hosted audio URL must allow CORS for AMP808 web playback."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAudioSource {
    LocalFile { name: String },
    HostedUrl { url: String },
}

impl WebAudioSource {
    pub fn local_file(name: impl Into<String>) -> Self {
        Self::LocalFile { name: name.into() }
    }

    pub fn hosted_url(url: impl Into<String>) -> Self {
        Self::HostedUrl { url: url.into() }
    }

    pub fn is_hosted_url(&self) -> bool {
        matches!(self, Self::HostedUrl { .. })
    }

    pub fn label(&self) -> &str {
        match self {
            Self::LocalFile { name } => name,
            Self::HostedUrl { url } => url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostedAudioIssue, WebAudioSource};

    #[test]
    fn hosted_url_cors_error_names_amp808_web_playback() {
        let message = HostedAudioIssue::CorsRequired.user_message();

        assert_eq!(
            message,
            "This hosted audio URL must allow CORS for AMP808 web playback."
        );
    }

    #[test]
    fn local_file_source_is_not_hosted() {
        let source = WebAudioSource::local_file("amen-break.wav");

        assert!(!source.is_hosted_url());
        assert_eq!(source.label(), "amen-break.wav");
    }

    #[test]
    fn hosted_url_source_is_hosted_and_uses_url_as_label() {
        let source = WebAudioSource::hosted_url("https://example.com/audio.mp3");

        assert!(source.is_hosted_url());
        assert_eq!(source.label(), "https://example.com/audio.mp3");
    }
}
```

- [ ] **Step 6: Run test to verify GREEN**

Run:

```bash
cargo test -p amp808-core web_audio::tests
```

Expected: PASS.

## Task 2: Core Analyser Band Mapping

**Files:**
- Modify: `crates/amp808-core/src/web_audio.rs`

- [ ] **Step 1: Write the failing analyser mapping test**

Append this test inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn analyser_bins_are_averaged_into_normalized_bands() {
    let bins = [0, 64, 128, 255];

    let bands = super::analyser_bins_to_bands(&bins, 2);

    assert_eq!(bands.len(), 2);
    assert!((bands[0] - 0.1254902).abs() < 0.0001);
    assert!((bands[1] - 0.7509804).abs() < 0.0001);
}
```

- [ ] **Step 2: Run test to verify RED**

Run:

```bash
cargo test -p amp808-core web_audio::tests::analyser_bins_are_averaged_into_normalized_bands
```

Expected: FAIL because `analyser_bins_to_bands` does not exist.

- [ ] **Step 3: Implement minimal analyser mapping**

Add this function above the test module:

```rust
pub fn analyser_bins_to_bands(bins: &[u8], band_count: usize) -> Vec<f32> {
    if band_count == 0 {
        return Vec::new();
    }

    if bins.is_empty() {
        return vec![0.0; band_count];
    }

    let mut bands = Vec::with_capacity(band_count);
    for band in 0..band_count {
        let start = band * bins.len() / band_count;
        let end = ((band + 1) * bins.len() / band_count).max(start + 1);
        let end = end.min(bins.len());
        let slice = &bins[start..end];
        let sum: u32 = slice.iter().map(|value| u32::from(*value)).sum();
        let average = sum as f32 / slice.len() as f32;
        bands.push(average / 255.0);
    }
    bands
}
```

- [ ] **Step 4: Run test to verify GREEN**

Run:

```bash
cargo test -p amp808-core web_audio::tests
```

Expected: PASS.

## Task 3: Minimal Ratzilla Web Shell

**Files:**
- Modify: `Cargo.toml`
- Create: `apps/web/Cargo.toml`
- Create: `apps/web/Trunk.toml`
- Create: `apps/web/index.html`
- Create: `apps/web/src/main.rs`

- [ ] **Step 1: Add the web app to the workspace**

Modify the root workspace block:

```toml
[workspace]
members = [".", "crates/amp808-core", "apps/web"]
default-members = [".", "crates/amp808-core"]
resolver = "2"
```

- [ ] **Step 2: Add the web package manifest**

```toml
# apps/web/Cargo.toml
[package]
name = "amp808_web"
version = "0.1.0"
edition = "2021"
license = "MIT"
publish = false

[dependencies]
amp808-core = { path = "../../crates/amp808-core" }
ratzilla = "0.3.1"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = [
    "Window",
    "Document",
    "HtmlAudioElement",
    "HtmlMediaElement",
    "HtmlInputElement",
    "Url",
    "Blob",
    "File",
    "FileList",
    "AudioContext",
    "MediaElementAudioSourceNode",
    "AnalyserNode",
    "AudioNode",
    "AudioDestinationNode",
    "DomException",
    "Event",
    "console",
] }
```

- [ ] **Step 3: Add Trunk and HTML entrypoint files**

```toml
# apps/web/Trunk.toml
[build]
target = "index.html"
release = true
public_url = "/AMP808/"
```

```html
<!-- apps/web/index.html -->
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>AMP808 Web</title>
  </head>
  <body>
    <main id="app"></main>
  </body>
</html>
```

- [ ] **Step 4: Add the minimal Ratzilla shell**

```rust
// apps/web/src/main.rs
use std::io;

use amp808_core::web_audio::{HostedAudioIssue, WebAudioSource};
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use ratzilla::{WebGl2Backend, WebRenderer};

fn main() -> io::Result<()> {
    let backend = WebGl2Backend::new()?;
    let terminal = Terminal::new(backend)?;
    let source = WebAudioSource::hosted_url("https://example.com/audio.mp3");
    let cors_message = HostedAudioIssue::CorsRequired.user_message();

    terminal.draw_web(move |frame| {
        let area = frame.area();
        let block = Block::default()
            .title("AMP808 WEB 808")
            .title_style(
                Style::default()
                    .fg(Color::Rgb(0xff, 0x7a, 0x45))
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(0xf6, 0xa6, 0x23)));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(inner);

        let title = Paragraph::new(Text::from(Line::from(vec![
            Span::styled("Source: ", Style::default().fg(Color::Gray)),
            Span::styled(source.label(), Style::default().fg(Color::White)),
        ])))
        .alignment(Alignment::Center);
        frame.render_widget(title, rows[0]);

        let cors = Paragraph::new(Text::from(Line::from(Span::styled(
            cors_message,
            Style::default().fg(Color::Red),
        ))))
        .alignment(Alignment::Center);
        frame.render_widget(cors, rows[1]);

        let body = Paragraph::new(Text::from(Line::from(Span::styled(
            "WebGL2 Ratzilla shell ready for browser audio wiring",
            Style::default().fg(Color::Rgb(0xc9, 0xc9, 0xc9)),
        ))))
        .alignment(Alignment::Center);
        frame.render_widget(body, rows[2]);
    });

    Ok(())
}
```

- [ ] **Step 5: Verify the web package checks for wasm**

Run:

```bash
rustup target add wasm32-unknown-unknown
cargo check -p amp808_web --target wasm32-unknown-unknown
```

Expected: PASS.

## Task 4: README First-Slice Documentation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a short web section**

Add this section near the build/run documentation:

```markdown
## AMP808 Web 808

The first web slice lives in `apps/web` and is designed for static hosting on GitHub Pages.
It uses Ratzilla with `WebGl2Backend`; browser playback is owned by `HTMLAudioElement`, with
Web Audio analysis planned for the 808 visualizer path.

Local file playback is the reliable phase-one path. External hosted URLs are in scope only when
the host allows CORS for browser media and Web Audio analysis. If a hosted URL does not allow
CORS, AMP808 Web shows: "This hosted audio URL must allow CORS for AMP808 web playback."

```bash
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
cd apps/web
trunk serve
```
```

- [ ] **Step 2: Run docs/core/native checks**

Run:

```bash
cargo test
cargo check -p amp808_web --target wasm32-unknown-unknown
```

Expected: PASS.

## Final Verification

- [ ] `cargo test` passes.
- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo check -p amp808_web --target wasm32-unknown-unknown` passes.
- [ ] `git status --short` shows only intended files.
