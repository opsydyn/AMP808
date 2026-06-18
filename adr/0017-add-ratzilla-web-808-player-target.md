---
status: proposed
date: 2026-06-15
decision-makers: alan
---

# Add a Ratzilla Web 808 player target

## Context and Problem Statement

AMP808 is currently a native terminal music player. Existing accepted ADRs deliberately bind the
native app to:

- `cpal` for low-latency native audio output (ADR-0002)
- Tokio plus crossterm for native TUI I/O and event polling (ADR-0004)
- `ratatui-image` terminal protocols for album art (ADR-0008)
- UI-only 808 animation and visualizer effects (ADR-0010)
- synthetic visualizers only where real samples are unavailable, such as `Music.app` mode
  (ADR-0014)

We want an AMP808 web target that can be hosted on GitHub Pages and feels like the 808 view,
similar in deployment shape to the existing Tech Radar Ratzilla web app. The target architecture
must support real browser audio, not settle for a static demo as the final web surface, but it must
not weaken or replace the native terminal architecture. The first committed slice may establish the
Ratzilla shell, shared core, and static-hosting boundary before playback wiring lands.

The question is: how should AMP808 add a GitHub Pages-hosted Ratzilla web player while respecting
the native audio/TUI decisions and browser security constraints?

## Decision Drivers

- Keep the native player architecture intact.
- Ship the web surface from static assets only, suitable for GitHub Pages.
- Support real browser playback in the first playback phase after the shell/core foundation.
- Preserve the 808 visual identity and use real analyser data when audio is playable.
- Make browser CORS limits explicit instead of silently showing fake visualizer data for external
  URLs that cannot be analysed.
- Keep the first implementation narrow enough to develop test-first.

## Considered Options

- Add a separate Ratzilla web target with `HTMLAudioElement` playback and Web Audio analysis.
- Try to compile the existing native AMP808 binary directly to WebAssembly.
- Build a custom Web Audio playback engine without `HTMLAudioElement`.
- Render a demo-only Ratzilla UI with synthetic playback state.

## Decision Outcome

Chosen option: "Add a separate Ratzilla web target with `HTMLAudioElement` playback and Web Audio
analysis", because it gives the web playback phase real browser playback while keeping native-only
dependencies out of the wasm build.

The web target architecture will:

1. Add a new `apps/web` Ratzilla app.
2. Use Ratzilla `WebGl2Backend` as the primary renderer.
3. Use `HTMLAudioElement` as the browser playback owner for play, pause, seek, duration, buffering,
   selected local files, and hosted URLs.
4. Connect the audio element to a Web Audio `AudioContext` through `createMediaElementSource()`.
5. Use an `AnalyserNode` for real visualizer data in the 808 UI.
6. Support the first playback phase from:
   - browser-selected local audio files, using object URLs
   - external hosted audio URLs, when browser media loading and CORS allow Web Audio analysis
7. Show a clear error for external URLs that cannot satisfy the browser's media/CORS requirements
   for AMP808 web playback and analysis.
8. Avoid synthetic/fallback visualizers for external hosted URLs that fail CORS analysis. Synthetic
   visualizers remain acceptable only for explicitly non-audio-backed states such as idle/demo
   screens or native `Music.app` mode governed by ADR-0014.

### Consequences

- Good, because local file playback works without a backend or upload flow.
- Good, because CORS-enabled hosted URLs can play and drive real visualizer data.
- Good, because `HTMLAudioElement` keeps browser-native seeking, duration, buffering, and media
  errors instead of reimplementing playback scheduling in Rust.
- Good, because `WebGl2Backend` is the right primary renderer for a frequently changing 808
  terminal grid.
- Bad, because arbitrary external URLs will not always work from GitHub Pages.
- Bad, because the static web target cannot proxy audio or fix CORS without adding a backend, which
  is out of scope.
- Neutral, because the codebase needs a web-safe shared core boundary before UI rendering can be
  reused cleanly.

## Non-goals

- No direct wasm port of the native `src/main.rs` binary.
- No `cpal`, `crossterm`, `ratatui-image`, `ffmpeg`, `yt-dlp`, `ctrlc`, local filesystem scanning,
  Music.app automation, or native config file access in the web target.
- No GitHub Pages audio proxy or server-side URL resolver.
- No arbitrary external URL guarantee. Hosted URLs are supported only when the browser can load and
  analyse them under CORS.
- No web implementation of the full native playlist resolver in the first web phases.
- No album art protocol parity in the first web phases.
- No custom Web Audio sample scheduler in the first web phases.
- No Canvas or DOM rendering fallback in the first web phases. If WebGL2 is unavailable, show an
  unsupported browser/runtime error.

## Implementation Plan

- **Affected paths**:
  - `Cargo.toml`: convert the repository to a workspace while preserving the existing native
    `amp808` package.
  - `crates/amp808-core/`: add a small web-safe shared crate for pure state and rendering inputs.
  - `apps/web/`: add the Ratzilla WebAssembly app, `index.html`, `Trunk.toml`, and web-specific
    source files.
  - `src/ui/visualizer.rs`: extract reusable visualizer data shaping into `amp808-core` where it is
    pure and web-safe.
  - `src/ui/view_808.rs`: extract or mirror small pure helpers for 808 render state where practical;
    keep native-only widgets and terminal image paths in the native app.
  - `src/playlist/mod.rs`: extract only web-safe track metadata and playlist state that can be
    shared without pulling native resolver/provider dependencies into wasm.
  - `README.md`: document the web target, GitHub Pages constraints, local file support, and external
    URL CORS requirements.
  - `adr/README.md`: add this ADR.
- **Dependencies**:
  - Add `ratzilla = "0.3.1"` to `apps/web`.
  - Add `wasm-bindgen = "0.2"`, `wasm-bindgen-futures = "0.4"`, `js-sys = "0.3"`, and
    `web-sys = "0.3"` to `apps/web`.
  - Enable only the needed `web-sys` features, including `Window`, `Document`, `HtmlAudioElement`,
    `HtmlMediaElement`, `HtmlInputElement`, `Url`, `Blob`, `File`, `FileList`, `AudioContext`,
    `MediaElementAudioSourceNode`, `AnalyserNode`, `AudioNode`, `AudioDestinationNode`,
    `DomException`, `Event`, and `console`.
  - Do not add native audio, subprocess, terminal, or filesystem crates to `apps/web` or
    `amp808-core`.
- **Patterns to follow**:
  - Keep web playback state as a small browser-facing adapter around `HTMLAudioElement`.
  - Treat the audio element as the source of truth for transport state, current time, duration, and
    media errors.
  - Connect the audio element to one Web Audio graph per active media element:
    `HTMLAudioElement -> MediaElementAudioSourceNode -> AnalyserNode -> AudioDestinationNode`.
  - Sample analyser output into a compact, testable `amp808-core` structure before rendering.
  - Keep Ratzilla rendering deterministic from a render snapshot: playback state, playlist state,
    analyser bands, focus, and dimensions.
  - Treat `WebGl2Backend` initialization failure as an explicit unsupported-browser/runtime error,
    not as a silent blank screen.
  - Reuse native 808 visual vocabulary where it is web-safe, but prefer extraction of pure helpers
    over importing native `App` or `PlaybackBackend`.
  - Use the existing Tech Radar web project only as a deployment and Ratzilla shape reference, not
    as an architecture to copy wholesale.
  - Use `/tdd` for implementation. Start with failing tests for pure state conversion, playlist
    selection, CORS error classification, and visualizer band mapping before writing the web app
    glue.
- **Patterns to avoid**:
  - Do not make the existing native `App` compile to wasm by feature-gating large parts of it.
  - Do not let `apps/web` depend on the native `amp808` binary crate.
  - Do not show synthetic visualizer data when an external URL fails CORS analysis.
  - Do not add a backend service or proxy as part of the static web target.
  - Do not regress native `cargo run`, native playback, or existing ADR-governed behavior.
- **Configuration**:
  - Add a Trunk configuration under `apps/web/`.
  - Configure the app for static hosting under the GitHub Pages public URL chosen for AMP808.
  - Keep any sample/demo tracks as same-origin static assets only if they are small and legally
    distributable.
- **Migration steps**:
  1. Create the workspace and `amp808-core` crate without changing native runtime behavior.
  2. Move or duplicate the smallest pure helpers into `amp808-core`, covered by tests.
  3. Add `apps/web` with a minimal Ratzilla shell using `WebGl2Backend`.
  4. Add browser audio adapter for local files and external URLs.
  5. Add Web Audio analyser capture and map it to 808 visualizer bands.
  6. Add CORS/media error handling for external URLs.
  7. Wire GitHub Pages static build documentation.

### Verification

These criteria cover the full web playback decision, not only the first shell/core foundation slice.

- [ ] `cargo test` passes for the native package and `amp808-core`.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes for native code that remains
      in the root package.
- [ ] `cargo fmt --all --check` passes.
- [ ] `trunk build --release` from `apps/web` succeeds for `wasm32-unknown-unknown`.
- [ ] The native app still builds and runs with `cargo run -- <args>`.
- [ ] Browser-selected local audio plays in the web app.
- [ ] Browser-selected local audio drives non-zero analyser/808 visualizer data while playing.
- [ ] A CORS-enabled external hosted audio URL plays in the web app and drives non-zero analyser/808
      visualizer data.
- [ ] A non-CORS external hosted audio URL shows an error that says the URL must allow CORS for
      AMP808 web playback.
- [ ] The web build output can be served as static files with no server-side code.
- [ ] No `cpal`, `crossterm`, `ratatui-image`, `ffmpeg`, `yt-dlp`, `ctrlc`, or native filesystem
      dependencies are pulled into the `apps/web` wasm dependency graph.

## More Information

- 2026-06-18: First implementation slice establishes the workspace boundary, `amp808-core`
  CORS/source policy and analyser band mapping, and a minimal Ratzilla `WebGl2Backend` shell in
  `apps/web`. It intentionally does not create an `HTMLAudioElement`, file picker, hosted URL input,
  or playback wiring yet.

## Pros and Cons of the Options

### Separate Ratzilla web target with `HTMLAudioElement` playback and Web Audio analysis

This option adds a dedicated browser app and extracts only web-safe shared state and rendering
helpers.

- Good, because it fits static GitHub Pages hosting.
- Good, because it supports real local-file and hosted-URL playback.
- Good, because Web Audio analyser output can drive real visuals.
- Good, because native AMP808 decisions remain intact.
- Bad, because it requires a shared-core extraction before reuse is clean.
- Bad, because browser CORS rules limit arbitrary hosted URLs.

### Direct wasm port of the existing native binary

This option tries to compile the current native `src/main.rs` and UI stack to wasm.

- Good, because it sounds like maximum reuse.
- Bad, because native dependencies such as `cpal`, `crossterm`, subprocesses, local filesystem
  access, and terminal image protocols do not map to GitHub Pages/browser execution.
- Bad, because it would require broad feature gates through the native app and risks weakening
  existing ADR boundaries.

### Custom Web Audio playback engine

This option decodes and schedules audio through Web Audio directly instead of using an audio
element.

- Good, because it could eventually provide deeper DSP control.
- Bad, because it is too large for the first browser playback phase.
- Bad, because it would reimplement browser media loading, seeking, buffering, and error behavior.
- Bad, because it does not remove the CORS limits for external hosted audio.

### Demo-only Ratzilla UI with synthetic playback state

This option renders the 808 UI in the browser without real audio.

- Good, because it would be the fastest visual prototype.
- Bad, because it does not satisfy the web target requirement for real browser playback.
- Bad, because synthetic visuals would hide the important browser audio/CORS constraints until
  later.

## More Information

- ADR-0002: Use cpal directly for audio output
- ADR-0004: Use tokio for async I/O with dedicated cpal audio thread
- ADR-0008: Album art display via ratatui-image
- ADR-0010: Add tachyonfx animated 808 chrome and visualizer parity
- ADR-0014: Pause Apple Music API rollout and use synthetic visualizers for Music.app parity
- Ratzilla backend documentation: <https://docs.rs/ratzilla/latest/ratzilla/backend/index.html>
- MDN `HTMLMediaElement.crossOrigin`: <https://developer.mozilla.org/en-US/docs/Web/API/HTMLMediaElement/crossOrigin>
- MDN `AudioContext.createMediaElementSource()`: <https://developer.mozilla.org/en-US/docs/Web/API/AudioContext/createMediaElementSource>
- GitHub Pages overview: <https://docs.github.com/en/pages/getting-started-with-github-pages/what-is-github-pages>
