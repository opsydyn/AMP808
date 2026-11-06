---
status: superseded
date: 2026-11-05
decision-makers: alan
consulted: projectM upstream documentation and official Rust bindings
informed: AMP808 contributors
superseded-by: ADR-0024
---

# Adopt projectM for authentic MilkDrop visualisation

## Context and Problem Statement

ADR-0022 established AMP808-R3TR0 as a decoupled macOS-first GPUI Kit application with a bounded
CPAL callback, off-thread PCM analysis, and honest spectrum and waveform views. Those views prove
the audio and rendering seams, but they do not provide the product's defining immersive MilkDrop
experience.

The original product handoff identified MilkDrop-class visualisation and projectM interoperability,
but deferred them behind a graphics-boundary experiment. Product direction on 2026-11-05 promotes
genuine MilkDrop preset compatibility to a release requirement and defers packaging work until that
capability is proven.

projectM is an LGPL-licensed, MilkDrop-compatible library. It accepts PCM samples, evaluates `.milk`
presets, and renders with OpenGL. GPUI Kit's renderer is Metal-backed on macOS and does not expose a
supported application API for arbitrary projectM OpenGL rendering. How should AMP808-R3TR0 add
authentic `.milk` compatibility without coupling OpenGL, projectM, or frame timing to the audio
callback or GPUI internals?

Related decisions remain active:

- ADR-0002 and ADR-0022 require bounded, non-blocking audio callbacks.
- ADR-0021 keeps desktop, TUI, and web products decoupled until reuse is proven.
- ADR-0022 keeps GPUI Kit as the desktop UI facade and requires honest visualiser availability.

## Decision Drivers

- Load and render real MilkDrop `.milk` presets rather than approximate their appearance.
- Reuse the real PCM path already proven by AMP808-R3TR0.
- Keep all projectM, OpenGL, preset evaluation, pixel readback, and GPUI image construction off the
  CPAL callback.
- Preserve GPUI Kit ownership of the application window, input, focus, accessibility, and controls.
- Avoid a GPUI fork, direct access to GPUI's private renderer, or a second visible application
  window.
- Make projectM failures visible and recoverable while retaining spectrum and waveform fallback
  modes.
- Use only presets with explicit AMP808 ownership or independently verified redistribution terms.
- Establish measurable rendering evidence before optimizing OpenGL-to-Metal transfer.

## Considered Options

- Run projectM in an isolated OpenGL context and copy its latest RGBA frame into a GPUI image.
- Embed a native OpenGL view or IOSurface directly inside the GPUI window.
- Launch a separate projectM companion window or process.
- Build a new Rust/WGSL visualiser inspired by MilkDrop without `.milk` compatibility.
- Reimplement the MilkDrop preset parser, evaluator, and renderer in Rust.

## Decision Outcome

Chosen option: "Run projectM in an isolated OpenGL context and copy its latest RGBA frame into a
GPUI image", because it is the least-entangled path that provides genuine `.milk` compatibility
without forking GPUI, sharing renderer internals, or changing the audio callback.

The first implementation uses a dedicated renderer worker with a macOS CGL OpenGL context. The
worker owns projectM and all OpenGL resources, consumes bounded mono PCM blocks forwarded by the
existing analysis worker, and publishes only its newest immutable RGBA frame. The GPUI thread turns
new frame revisions into `RenderImage` values and presents them in the existing visualiser panel.
Old PCM blocks and old render frames may be dropped; audible samples may not be dropped to preserve
visual output.

The GPU-to-CPU readback is an intentionally measured first transport, not the permanent high-end
rendering promise. A later ADR may replace it with IOSurface or another zero-copy bridge only after
the compatibility path works and profiling shows readback is the limiting cost.

Use the official low-level `projectm-sys` bindings behind an application-owned safe adapter rather
than exposing raw FFI or the higher-level wrapper throughout the application. The adapter owns the
projectM handle, validates null pointers and dimensions, converts Rust strings to nul-terminated
`CString` values, prevents unwinding across FFI, and destroys projectM on the renderer thread.

### Consequences

- Good, because AMP808-R3TR0 gains authentic `.milk` compatibility instead of a branded imitation.
- Good, because projectM and OpenGL remain replaceable behind a data-only PCM-in/frame-out boundary.
- Good, because GPUI retains ownership of all visible application interaction and accessibility.
- Good, because initialization or preset failures can fall back to existing visualisers without
  affecting playback.
- Bad, because projectM adds a C++/CMake/native-library toolchain and LGPL distribution obligations.
- Bad, because OpenGL is deprecated on macOS and the initial frame readback consumes CPU and memory
  bandwidth.
- Bad, because two graphics APIs run in one process and require explicit lifecycle, thread, and
  resource ownership.
- Neutral, because spectrum and waveform remain supported visualiser modes and useful fallbacks.

## Non-goals

- No projectM or MilkDrop work in the TUI or web applications.
- No GPUI or GPUI Kit fork and no direct dependency on GPUI's private Metal/wgpu renderer.
- No second visible SDL, OpenGL, or companion-process window.
- No community preset pack, texture pack, preset marketplace, executable plugin, or network fetch.
- No preset browser, user preset import, transitions, favourites, or automatic preset rotation in
  the first vertical slice.
- No full-screen visualiser, packaging, signing, notarization, or updater work in this slice.
- No claim that GPU-to-CPU readback is the final performance architecture.

## Implementation Plan

- **Affected paths**:
  - `apps/desktop/Cargo.toml`: add a macOS-only `milkdrop-projectm` feature and exact native adapter
    dependencies; keep projectM dependencies out of TUI and web packages.
  - `apps/desktop/src/milkdrop.rs`: define data-only renderer state, bounded PCM/frame contracts,
    failure states, and worker lifecycle.
  - `apps/desktop/src/milkdrop/macos.rs`: own CGL, OpenGL, projectM FFI, preset loading, rendering,
    readback, and resource cleanup.
  - `apps/desktop/src/analysis.rs`: forward completed PCM windows to an optional bounded MilkDrop
    producer from the analysis thread, not the audio callback.
  - `apps/desktop/src/playback.rs`: create and stop the renderer worker with the playback session and
    expose the latest frame/status in immutable session snapshots.
  - `apps/desktop/src/ui.rs`: add `MilkDrop` to the `V` cycle, present the latest frame, keep the
    existing accessible mode control, and show explicit unavailable/error text.
  - `apps/desktop/assets/presets/amp808-first-light.milk`: provide one original AMP808-owned preset
    used by the first slice and tests.
  - `apps/desktop/tests/` or package-local test modules: cover state, bounded transfer, lifecycle,
    fallback, preset errors, and rendered UI behavior.
  - `moon.yml`: add an explicit native MilkDrop build/test task after the feature compiles reliably;
    do not change existing default desktop task semantics during the spike.
- **Dependencies**:
  - `projectm-sys = "1.2.2"` with playlist disabled and static linking for the spike, behind
    `milkdrop-projectm`. Revisit shared-library distribution before packaging.
  - `objc2-open-gl = "0.3.2"` for macOS CGL ownership, behind `milkdrop-projectm`.
  - `glow = "0.17"` for the small OpenGL framebuffer/readback surface, behind
    `milkdrop-projectm`.
  - `image = "0.25"` only if GPUI Kit's public image construction requires the matching public
    image types.
  - CMake is a local/CI build prerequisite for `projectm-sys`; do not depend on Homebrew's older
    system projectM formula.
- **Patterns to follow**:
  - Follow `AnalysisTap`, `AnalysisWorker`, and `LatestAnalysis` for bounded transfer, latest-value
    publication, explicit availability, and owned worker shutdown.
  - Use red-green-refactor one behavioral slice at a time: state/fallback, bounded PCM transfer,
    fake renderer worker, native projectM frame, then GPUI presentation.
  - Keep unsafe code inside the macOS adapter; document every unsafe block with its invariant and
    expose only safe Rust types to the rest of the application.
  - Keep the renderer and projectM handle on one dedicated thread for their entire lifetime.
  - Stop rendering while PCM is paused, stale, or unavailable; retain the last frame with an honest
    state label rather than fabricating audio-reactive motion.
  - Check preset provenance before adding every distributed preset or texture.
- **Patterns to avoid**:
  - Do not feed projectM from GPUI spectrum bands; it must receive real PCM.
  - Do not add a second producer/consumer workload to the CPAL callback.
  - Do not use unbounded channels or queue every rendered frame.
  - Do not pass GPUI, OpenGL, CGL, or projectM types through the audio/analysis public contracts.
  - Do not call projectM from more than one thread or destroy its handle under a different current
    OpenGL context.
  - Do not use community presets whose redistribution terms are assumptions rather than evidence.
- **Configuration**:
  - Initial render size is `640x360`, target rate is 60 FPS, and degraded mode is 30 FPS.
  - The renderer publishes at most one unconsumed frame; newer frames replace older ones.
  - `milkdrop-projectm` is an explicit spike feature until native verification passes, then a
    follow-up change may make it part of the default macOS product build.
- **Migration steps**:
  1. Add failing public-behavior tests for mode cycling and explicit unavailable/failure state.
  2. Implement the data-only MilkDrop state and bounded latest-frame contract without native FFI.
  3. Add a fake renderer worker test proving PCM input, frame replacement, pause, and shutdown.
  4. Add the macOS CGL/projectM adapter behind `milkdrop-projectm` and render the AMP808 preset from
     deterministic PCM.
  5. Present the resulting RGBA frame inside the existing GPUI visualiser panel.
  6. Run automated desktop and workspace gates with the native feature disabled and enabled.
  7. Perform a separate native macOS pass with audible playback and measure render FPS, frame drops,
     audio underruns, resize behavior, pause/resume, and recovery from an invalid preset.
  8. Decide whether readback meets the product bar or whether a separate IOSurface ADR is required.

### Verification

- [ ] A failing test is observed before each behavioral slice is implemented.
- [ ] `V` cycles `Spectrum -> Waveform -> MilkDrop -> Spectrum` and the accessible action label names
      the next mode.
- [ ] Native-feature-disabled and initialization-failure states display `MILKDROP UNAVAILABLE`
      without hiding spectrum/waveform or interrupting playback.
- [ ] PCM reaches the MilkDrop worker through a bounded handoff owned by the analysis path; source
      review confirms no new projectM/OpenGL/allocation/blocking work in the CPAL callback.
- [ ] One original `.milk` preset loads from application data and renders a non-uniform frame from a
      deterministic PCM fixture.
- [ ] Pausing playback stops PCM ingestion and projectM frame advancement while retaining an honest
      paused state.
- [ ] Invalid preset data produces a visible recoverable error and selecting Spectrum or Waveform
      still works.
- [ ] At `640x360`, native acceptance records at least 30 rendered frames per second with no increase
      in audible underflow count during a ten-minute playback sample; 60 FPS remains the target.
- [ ] `cargo test --locked -p amp808-r3tr0` passes without the native feature.
- [ ] `cargo test --locked -p amp808-r3tr0 --features milkdrop-projectm` passes on the supported
      macOS toolchain.
- [ ] `cargo clippy --locked -p amp808-r3tr0 --all-targets --features milkdrop-projectm -- -D warnings`
      and `cargo fmt --all --check` pass.
- [ ] Existing TUI/core/web verification remains unchanged and green.
- [ ] Dependency and source review records projectM/library/preset licences; no ambiguous community
      preset is included.

## Pros and Cons of the Options

### Isolated projectM context with GPUI image copy

- Good, because it proves real `.milk` compatibility without renderer-private APIs.
- Good, because PCM-in/RGBA-out is a small, testable boundary.
- Bad, because `glReadPixels` and image upload may cap resolution or frame rate.
- Bad, because CGL/OpenGL are deprecated macOS APIs.

### Native OpenGL view or IOSurface inside GPUI

- Good, because a direct or shared surface could avoid per-frame CPU readback.
- Good, because it could support higher resolutions after compatibility is proven.
- Bad, because GPUI does not currently expose a stable child-surface or arbitrary-shader contract.
- Bad, because Cocoa/Metal/OpenGL lifecycle coupling would enlarge the unsafe and framework-specific
  boundary before the product behavior is proven.

### Separate projectM window or process

- Good, because projectM's existing SDL frontends already prove that rendering model.
- Bad, because it breaks the single-window AMP808 experience and complicates focus, lifecycle,
  accessibility, and state synchronization.

### Rust/WGSL MilkDrop-inspired renderer

- Good, because it aligns with Metal/wgpu and avoids OpenGL/C++ integration.
- Bad, because it does not satisfy the approved requirement to load genuine `.milk` presets.

### Reimplement MilkDrop in Rust

- Good, because AMP808 would eventually control the entire runtime and rendering stack.
- Bad, because parsing, expression evaluation, shader translation, texture semantics, and preset
  compatibility form a separate multi-year product risk before feature parity.

## More Information

- ADR-0022: [Add the macOS-first AMP808-R3TR0 desktop player with GPUI Kit](0022-add-amp808-r3tr0-macos-desktop-player.md)
- projectM: <https://github.com/projectM-visualizer/projectm>
- Official Rust bindings: <https://github.com/projectM-visualizer/projectm-rs>
- projectM C API: <https://github.com/projectM-visualizer/projectm/blob/master/src/api/include/projectM-4/core.h>
- `objc2-open-gl`: <https://docs.rs/objc2-open-gl/0.3.2/objc2_open_gl/>

Boundary review verdict: **safe with noted risk**. Audio owns audible frames; analysis owns PCM
feature extraction and optional visual forwarding; the MilkDrop worker owns projectM/OpenGL and
render frames; GPUI owns presentation. The notable risks are deprecated OpenGL, native build
tooling, LGPL distribution obligations, and measurable frame-copy cost.

Simplicity check: authentic `.milk` compatibility rules out the bespoke WGSL option; reusing the
existing analysis boundary avoids callback coupling; RGBA latest-frame publication avoids GPUI
renderer internals. Preset browsing, community packs, full-screen mode, transitions, zero-copy
interop, and packaging remain deliberately omitted from the first vertical slice.

Review this decision after native verification or by 2026-11-12, whichever comes first. Stop the
readback path if it cannot sustain 30 FPS at `640x360`, increases audible underflows, requires a GPUI
fork, or cannot fail safely. In that case, preserve the proven PCM/projectM adapter and write a new
ADR for a native surface or IOSurface bridge.

The 2026-11-05 native spike triggered this stop condition before frame-rate measurement: projectM
requires a real default OpenGL drawable, while macOS rejected both attempted drawable-less CGL
paths. ADR-0024 proposed the supported native `NSOpenGLView` route. ADR-0023 remained accepted
until that proposal was explicitly accepted.

Superseded by
[ADR-0024: Host projectM in a native macOS OpenGL subview](0024-host-projectm-in-a-native-macos-opengl-subview.md)
after Alan accepted the native-view route on 2026-11-05.
