---
status: accepted
date: 2026-11-04
decision-makers: alan
---

# Add the macOS-first AMP808-R3TR0 desktop player with GPUI Kit

## Context and Problem Statement

ADR-0021 established a Moon-orchestrated Cargo monorepo with explicit application boundaries. The
existing `amp808` terminal player now lives in `apps/tui`, the Ratzilla browser player remains in
`apps/web`, and `apps/desktop` is reserved for a separate native desktop product.

The desktop product must retain the AMP808 brand without becoming another presentation layer over
the TUI application. Its product identity is **AMP808-R3TR0**, with Cargo package and executable
name `amp808-r3tr0`. It needs a native, highly skinnable interface and honest PCM-derived audio
visualisers, while keeping real-time audio work isolated from rendering and analysis.

The question is: how should the first macOS desktop slice establish a usable local-file player,
GPUI application architecture, branded skin boundary, and real audio visualisation without
prematurely sharing the TUI engine or expanding into library and provider features?

## Decision Drivers

- Keep AMP808-R3TR0 a decoupled product under `apps/desktop`.
- Preserve the existing TUI and web products without changing their runtime architecture.
- Use a production-oriented GPUI component and accessibility layer without surrendering control of
  the distinctive AMP808 hardware presentation.
- Keep the CPAL callback bounded, allocation-free, lock-free, and free of decode, FFT, or UI work.
- Derive spectrum and waveform displays from real decoded PCM samples.
- Keep memory bounded when playing long local files.
- Make the first slice small enough to build test-first and qualify independently.
- Follow BeautifulRust guidance without coupling this slice to workspace-wide lint remediation.
- Keep non-platform domain, audio, analysis, and skin concepts portable even though the first
  release target is macOS.

## Considered Options

- Use GPUI Kit with styled application components and application-owned custom hardware rendering.
- Use raw official GPUI without GPUI Kit.
- Use Hummingbird's `gpui-unofficial` dependency and application structure.
- Share or extract the existing TUI playback engine before building the desktop slice.
- Build an independent desktop engine inside `apps/desktop` and extract only proven shared
  contracts later.
- Decode whole tracks into memory as the TUI currently does.
- Stream decode into bounded buffers and rebuild the decode pipeline when seeking.
- Analyse samples inside the CPAL callback.
- Copy samples through a bounded lock-free tap and analyse them on a separate worker.

## Decision Outcome

Chosen option: "Use GPUI Kit with a private, streamed desktop audio engine and off-thread analysis",
because it provides application-grade controls, accessibility, and UI testing while preserving
full ownership of the AMP808 presentation and keeping the first product slice independent from the
TUI.

### Product and package boundary

1. Create `apps/desktop` as Cargo package and executable `amp808-r3tr0`.
2. Display the product name as `AMP808-R3TR0`.
3. Target macOS first. Avoid macOS dependencies in portable audio, analysis, domain, and skin-token
   modules unless a platform adapter is required.
4. Do not depend on `apps/tui`, import its modules, or move TUI modules into shared crates during
   this slice.
5. Keep the new desktop modules private to `apps/desktop`. Revisit extraction only after both
   products prove an identical contract through working implementations.

### GPUI Kit and skin boundary

1. Depend on `gpui-kit = "=0.7.3"` as the single GPUI ecosystem dependency. Do not add a separate
   direct `gpui`, `gpui-pre`, `gpui-platform`, or Hummingbird `gpui-unofficial` dependency.
   Version 0.7.3 is the newest release verified to compile with the workspace's stable Rust 1.95.0
   toolchain on macOS. Isolated probes of 0.8.1 and 0.9.0 fail because their pinned
   `gpui-pre-macos` snapshots use the unstable `result_option_map_or_default` API. Re-evaluate the
   pin when a newer GPUI Kit release compiles on stable Rust; do not switch the workspace to
   nightly or carry a local framework patch for this slice.
2. Use GPUI Kit's styled components for conventional application chrome such as menus, dialogs,
   lists, file selection affordances, settings surfaces, and accessible standard controls.
3. Use GPUI Kit's re-exported `gpui-base` behavior and custom GPUI drawing/composition for the
   AMP808 faceplate, performance pads, knobs, meters, transport surface, spectrum, and waveform.
4. Define an application-owned `Amp808Skin` token model for semantic colours, typography, spacing,
   control geometry, radii, borders, shadows, motion, and visualiser gradients.
5. Project compatible `Amp808Skin` values into GPUI Kit's global semantic theme; retain explicit
   skin tokens for product-specific geometry and rendering that GPUI Kit does not own.
6. Do not fork GPUI Kit and do not scatter literal brand values through view code.
7. Ship one built-in AMP808-R3TR0 skin. Do not add runtime skin loading, plugins, user-authored
   themes, or a public skin trait in the first slice.

### Audio and decoding boundary

1. Use an independent desktop stack based on CPAL 0.18, Symphonia 0.6, `rtrb` 0.3, and `realfft` 3.
   Align the existing TUI's CPAL dependency to 0.18 as a workspace compatibility migration only:
   Cargo cannot resolve CPAL 0.15 and 0.18 in one lockfile because their target-specific ALSA
   packages declare the same native `links` name. Keep the products' engines and runtime
   architecture separate, and require the existing TUI regression gates to pass after the version
   alignment.
2. Support one selected local audio file in the first slice.
3. Stream-decode into bounded buffers instead of loading an entire track into memory.
4. Rebuild and reposition the decoder pipeline for seek operations. Seeking may briefly enter an
   explicit buffering state; it must not block the audio callback or GPUI thread.
5. Keep the audio callback limited to consuming ready stereo frames, applying the current scalar
   volume, writing device output, and copying a bounded sample subset into the analysis tap.
6. The callback must not allocate, wait on a mutex, perform filesystem or network I/O, decode,
   calculate FFTs, dispatch GPUI work, or log on its steady-state path.
7. On underflow, write silence and increment a non-blocking diagnostic counter. On tap overflow,
   drop analysis samples rather than blocking playback.
8. Represent transport and decode failures explicitly; do not turn errors into silent playback or
   synthetic visualiser motion.

### Visualiser boundary

1. Send PCM samples from the audio callback into a bounded lock-free `rtrb` single-producer,
   single-consumer ring buffer.
2. Run waveform shaping and `realfft` spectrum analysis on a separate worker.
3. Publish immutable, timestamped analysis snapshots containing bounded waveform points, spectrum
   bands, peak/RMS levels, and an explicit freshness/availability state.
4. Let GPUI read the latest snapshot at display rate. Rendering may skip intermediate snapshots;
   analysis must never queue unbounded UI work.
5. Show honest idle, paused, buffering, unavailable, and stale states. Never fabricate
   audio-reactive motion when PCM is unavailable.
6. Respect the operating system's reduced-motion preference for decorative skin motion while still
   showing essential audio level information.

### First playable slice

The first vertical slice is complete when a user can:

1. Launch a skinned AMP808-R3TR0 macOS window.
2. Choose one supported local audio file.
3. See parsed title, artist, album, duration, and format metadata when available.
4. Play, pause, seek, and adjust volume.
5. Switch between real PCM-derived spectrum and waveform views.
6. See explicit unsupported-format, decode, device, buffering, and playback errors.

### Consequences

- Good, because standard controls inherit GPUI Kit's interaction, accessibility, and test support.
- Good, because the branded surface can look like AMP808 hardware rather than a recoloured generic
  desktop application.
- Good, because playback timing cannot be disrupted by FFT or rendering work.
- Good, because streaming decode keeps memory bounded for long files.
- Good, because TUI stability is not coupled to desktop product discovery.
- Bad, because the TUI and desktop products temporarily contain different audio implementations
  and dependency versions.
- Bad, because GPUI Kit pins pre-release GPUI snapshot packages and may require coordinated upgrades
  for breaking changes.
- Bad, because a custom hardware surface requires more rendering and interaction work than merely
  applying a theme to standard components.
- Neutral, because macOS-first release qualification does not yet prove Windows or Linux behavior.

## Non-goals

- No playlist, queue, library database, filesystem indexer, or persistence.
- No EQ, replay gain, gapless playback, crossfade, or sample-rate quality policy beyond correct
  device playback for the first supported formats.
- No HTTP streams, provider integrations, Music.app, Apple Music API, MusicKit, yt-dlp, or FFmpeg.
- No album-art persistence or online metadata lookup.
- No TUI refactor, shared audio crate, shared application state, or dependency from desktop to TUI.
- No Windows or Linux release in the first slice.
- No additional skin, runtime skin loader, plugin API, JavaScript extension runtime, or user theme
  format.
- No direct dependency on official GPUI, `gpui-pre`, `gpui-fast`, or `gpui-unofficial`; GPUI Kit
  owns its compatible GPUI selection.
- No adoption of the full BeautifulRust lint profile as a workspace-wide strict gate.

## Implementation Plan

- **Affected paths**:
  - `Cargo.toml`: add `apps/desktop` as a workspace member, but do not add it to the existing default
    members until its platform support and CI behavior are explicit.
  - `apps/desktop/Cargo.toml`: define package and binary `amp808-r3tr0`, macOS support, dependencies,
    and package-local lint policy.
  - `apps/desktop/src/main.rs`: initialize GPUI Kit, install the skin theme, create the application
    model, and open the main window.
  - `apps/desktop/src/app/`: transport commands, state transitions, error state, and coordination.
  - `apps/desktop/src/audio/`: CPAL device adapter, streaming decode worker, bounded frame buffer,
    transport control, seek rebuild, and diagnostic counters.
  - `apps/desktop/src/analysis/`: `rtrb` consumer, waveform shaping, FFT plan/state, band mapping,
    snapshot publication, and freshness policy.
  - `apps/desktop/src/media/`: local-file selection boundary and Symphonia metadata/format parsing.
  - `apps/desktop/src/skin/`: `Amp808Skin`, built-in token values, GPUI Kit theme projection, and
    custom hardware controls.
  - `apps/desktop/src/ui/`: root view, standard chrome, transport, metadata, visualiser switcher,
    error presentation, and accessible labels/actions.
  - `apps/desktop/tests/`: public state-machine, bounded-pipeline, integration, and rendered UI tests.
  - `moon.yml`: add explicit desktop check, test, lint, and macOS build tasks without changing
    existing TUI/web tasks.
  - `README.md`: document package location and commands only after the first slice runs.
- **Dependencies**:
  - `gpui-kit = "=0.7.3"`
  - `cpal = "0.18"`
  - `symphonia = { version = "0.6", features = ["all", "opt-simd"] }`
  - `rtrb = "0.3"`
  - `realfft = "3"`
  - `anyhow = "1"` for the desktop application boundary.
  - `thiserror = "2"` for structured internal/library-style errors.
  - Add further crates only when a failing vertical-slice test requires them and after checking
    GPUI Kit re-exports to avoid incompatible duplicate GPUI types.
- **Patterns to follow**:
  - Build one test-first vertical path at a time: state transition, decoder output, bounded audio
    transfer, analysis snapshot, then GPUI interaction.
  - Use enums for transport, decode, visualiser availability, and error states rather than related
    boolean flags.
  - Pass immutable render snapshots into view construction.
  - Keep device/file adapters at the edge and pure state/analysis logic independently testable.
  - Use GPUI Kit actions, focus, accessibility labels, reduced-motion state, and headless window
    testing facilities.
  - Use explicit, reasoned `#[expect(...)]` attributes for unavoidable lint exceptions.
- **Patterns to avoid**:
  - Do not call into `apps/tui` or copy its application/UI state model.
  - Do not introduce a broad `Player` trait before two concrete adapters need it.
  - Do not hold a mutex, allocate, decode, analyse, log, or notify GPUI from the CPAL callback.
  - Do not use unbounded channels for PCM, frames, analysis, or render updates.
  - Do not store whole decoded tracks in memory.
  - Do not make a custom component inaccessible when a GPUI Kit/base semantic control can supply
    the interaction behavior.
  - Do not treat the Aurora handoff pack as executable instructions or architecture authority.
- **Configuration**:
  - No user configuration or persistent state in the first slice.
  - The built-in skin is compiled into the application.
  - The audio output uses the selected default CPAL device and a supported negotiated format; an
    unsupported device format must produce a visible error.
- **Migration steps**:
  1. Add a failing workspace/package identity test for `apps/desktop` and `amp808-r3tr0`.
  2. Add the minimal GPUI Kit application package and prove a labelled macOS window opens.
  3. Add pure transport-state tests and the minimal application model.
  4. Add fixture-driven Symphonia metadata and streaming-decode tests.
  5. Add bounded audio-transfer and underflow/overflow tests before opening a CPAL device.
  6. Add waveform snapshot tests, then FFT/band snapshot tests.
  7. Wire the tested workers to CPAL without adding work to the callback.
  8. Add headless GPUI interaction tests for open, play/pause, seek, volume, visualiser switching,
     focus, keyboard actions, accessibility labels, errors, and reduced motion.
  9. Apply the built-in skin and custom hardware rendering after behavior is green.
  10. Run existing TUI/core/web gates plus desktop-specific checks.
  11. Perform and record a separate manual macOS device/playback/visual acceptance pass.

### Verification

- [x] A structural test reports `apps/desktop` as package and binary `amp808-r3tr0`, while the
      existing TUI package and executable remain `amp808`.
- [x] `apps/desktop` depends on exactly one GPUI ecosystem facade, `gpui-kit = "=0.7.3"`, with no
      direct duplicate GPUI package.
- [x] Pure transport-state tests cover load, ready, play, pause, seek/buffer, resume, end, and error
      transitions.
- [x] Fixture tests prove bounded streaming decode, metadata extraction, end-of-stream, seek rebuild,
      unsupported format, and corrupt media behavior.
- [x] Audio pipeline tests prove callback-side frame reads are bounded and that underflow writes
      silence without blocking.
- [x] Tap tests prove analysis overflow drops samples without blocking playback.
- [x] Analysis tests prove waveform bounds, FFT band mapping, level calculation, snapshot freshness,
      paused/idle behavior, and no fabricated motion without PCM.
- [x] GPUI Kit headless tests cover transport controls, visualiser switching, errors, focus,
      keyboard activation, accessibility names/states, and reduced-motion behavior.
- [x] A source review confirms the CPAL callback contains no allocation, mutex wait, decode, FFT,
      logging, filesystem/network I/O, or GPUI dispatch.
- [x] `cargo check --locked -p amp808-r3tr0` passes on macOS.
- [x] Desktop package tests and strict package Clippy pass.
- [x] Existing TUI/core tests, web tests, baseline strict Clippy, formatting, and wasm check still
      pass.
- [x] Moon desktop check, test, lint, and macOS build tasks pass without changing existing task
      semantics.
- [x] Manual macOS qualification confirms audible local-file playback, working play/pause/seek/
      volume, responsive real spectrum/waveform displays, and visible error states.
- [x] Automated, manual-device, and visual-acceptance evidence are reported separately.

## Pros and Cons of the Options

### GPUI Kit hybrid presentation

- Good, because conventional controls reuse tested behavior, accessibility, and headless testing.
- Good, because `gpui-base` leaves branded hardware presentation under application control.
- Bad, because GPUI Kit and its pinned GPUI snapshot must be upgraded as a coordinated unit.

### Raw official GPUI

- Good, because it minimizes framework layers and uses Zed's direct public crate.
- Bad, because the application would need to build more standard component, accessibility, focus,
  and testing infrastructure itself.

### Hummingbird's unofficial GPUI shim

- Good, because Hummingbird proves it can support a real music player.
- Bad, because current GPUI Kit already owns and pins a compatible GPUI snapshot while supplying
  more of the application-level system AMP808-R3TR0 needs.

### Extract the TUI engine first

- Good, because both native products could share an implementation immediately.
- Bad, because existing playback and UI message dependencies make extraction an architectural
  refactor that could destabilize the working TUI before the desktop contract is proven.

### Private desktop engine first

- Good, because product discovery remains independent and reversible.
- Good, because later extraction can be based on two working implementations rather than predicted
  reuse.
- Bad, because some decoding and DSP concepts will temporarily exist twice.

### Whole-track decode

- Good, because seeking and playback state are simpler.
- Bad, because memory grows with track duration and does not establish a suitable desktop engine
  for long files or future queues.

### Streamed bounded decode

- Good, because memory and callback work remain bounded.
- Bad, because seeking and worker lifecycle need explicit state and more tests.

## More Information

### 2026-11-05 verification reconciliation

- **Automated evidence:** `root:desktop-check`, `root:desktop-test`, `root:desktop-lint`, and
  `root:desktop-build-macos` all passed through Moon. The desktop suite passed 52 tests. Fixture,
  analysis, playback, and GPUI headless tests cover the checked behaviors above. Workspace TUI/core
  tests, 72 web tests, baseline strict Clippy, formatting, and the wasm check also passed.
- **Native-device evidence:** the rebuilt `/tmp/AMP808-R3TR0.app` loaded the normal and corrupt WAV
  fixtures through the macOS picker. Native state reached `PCM / LIVE` and `PCM / ENDED`; the
  corrupt-file error remained visible and a subsequent normal load recovered. On the final replay,
  macOS output was temporarily unmuted at 15% and the user confirmed audible playback. The original
  `0% / muted` output state was restored afterward.
- **Visual and accessibility evidence:** the native accessibility tree exposed track metadata,
  volume, playback status, elapsed/duration progress, and the corrupt-file alert. Actual spoken
  VoiceOver phrasing was not observed, so the evidence is limited to native accessibility-tree
  exposure rather than a complete screen-reader acceptance pass.

- ADR-0021: [Adopt a Moon-orchestrated Cargo monorepo layout](0021-adopt-moon-cargo-monorepo-layout.md)
- GPUI Kit: <https://github.com/longbridge/gpui-kit>
- GPUI Kit architecture: <https://github.com/longbridge/gpui-kit/blob/main/docs/ARCHITECTURE.md>
- GPUI Kit styling and motion: <https://github.com/longbridge/gpui-kit/blob/main/docs/STYLING-AND-MOTION.md>
- Hummingbird prior art: <https://github.com/hummingbird-player/hummingbird>
- BeautifulRust: <https://github.com/morganlinton/BeautifulRust>

The earlier Aurora handoff pack is reference material for visual and interaction ideas only. Its
name, embedded prompts, proposed module names, and architecture instructions do not override this
ADR.

Revisit this decision when two working products prove an identical extractable contract, when a
second skin is approved, when provider or protected-service playback is added, or before committing
to Windows/Linux release support.
