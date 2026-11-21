---
status: accepted
date: 2026-11-06
decision-makers: alan
consulted: GPUI Fast composition source, ADR-0022, ADR-0024, Rune renderer plan
informed: AMP808 contributors
---

# Add a native cosmic Metal underlay beneath GPUI

## Context and Problem Statement

AMP808-R3TR0 currently paints an opaque GPUI hardware skin and hosts projectM in a native
`NSOpenGLView` limited to the visualiser panel. ADR-0024 deliberately keeps that view above GPUI's
base surface and non-interactive. Expanding it to the whole window would cover the player controls
instead of becoming their background.

The product direction is now a full-window, intergalactic psychedelic energy field with dark
translucent hardware panels above it. The scene should react to real decoded audio without moving
analysis or rendering work onto the CPAL callback. GPUI must continue to own controls, focus,
keyboard shortcuts, accessibility, dialogs, and error presentation.

GPUI Kit 0.7.3 provides an opt-in `gpui-fast` backend with public window composition APIs. The
dependency is already present transitively, but AMP808 currently uses Kit's default upstream GPUI
backend; this decision explicitly enables the published Kit feature. GPUI Fast places native
surfaces between a GPUI base surface and a transparent GPUI overlay surface. This supplies the
missing composition boundary without a GPUI fork, direct GPUI dependency, or framework migration.

How should AMP808-R3TR0 add a whole-player animated cosmic background while preserving its tested
audio, interaction, accessibility, and MilkDrop boundaries?

## Decision Drivers

- Keep `gpui-kit = "=0.7.3"` as the only application-facing GPUI dependency and select its
  published `gpui-fast` feature.
- Preserve GPUI ownership of every interactive and accessible element.
- Reuse GPUI Fast's public composition API rather than private renderer state or a framework fork.
- Keep shader work and audio analysis out of the CPAL callback.
- Drive audio reaction only from bounded, immutable analysis snapshots produced from real PCM.
- Render at display cadence without queueing unbounded work.
- Respect macOS Reduce Motion and provide a static or substantially restrained scene.
- Fail decoratively: playback and the complete opaque fallback skin must remain usable if Metal
  initialization, shader compilation, drawable acquisition, or presentation fails.
- Retain projectM as the authentic `.milk` visualiser selected inside the visualiser panel.
- Keep the first implementation macOS-first and avoid pretending it proves another platform.

## Considered Options

- Add a native Metal composition surface below a deferred GPUI overlay.
- Expand the existing projectM `NSOpenGLView` to the whole window.
- Decode a looped video or animated image into GPUI.
- Replace GPUI with Rune or another renderer that owns a compositor.

## Decision Outcome

Chosen option: "Add a native Metal composition surface below a deferred GPUI overlay", because it
uses the pinned framework's public composition contract, gives the background a direct GPU path,
keeps all interaction in GPUI, avoids deprecated OpenGL for new rendering work, and needs no
per-frame GPU-to-CPU copy.

The root GPUI scene will retain an opaque AMP808 fallback colour on the base surface. A non-
interactive native composition surface will cover the content bounds above that base. The complete
player interface will be painted as a deferred GPUI draw on the framework's transparent overlay
surface. Existing panels will use explicit translucent skin tokens so the cosmic surface is visible
through them while text and controls retain adequate contrast.

The native adapter will own a Metal device, command queue, render pipeline, buffer state, and
`CAMetalLayer` attached inside GPUI's managed native-surface container. A small safe host API will
accept current bounds, elapsed presentation time, the motion policy, and a value-only audio signal.
Objective-C and Metal objects remain inside the macOS adapter.

The cosmic audio signal will derive bass, mid, high, RMS, and peak energy from the latest immutable
analysis snapshot. Available snapshots are clamped to finite normalized values. Idle, unavailable,
paused, and stale analysis produce zero audio energy, while ambient shader time may continue under
standard motion. Reduced Motion freezes shader time and disables audio displacement while retaining
a static cosmic field.

Renderer failure will be stored as a bounded status and will stop further native presentation
attempts for that host. The GPUI base remains opaque, controls remain available, and playback is
unaffected. The decorative renderer must never become a playback error or an audio-thread
dependency.

### Consequences

- Good, because the composition boundary is public and already implemented by the pinned GPUI Fast
  macOS platform.
- Good, because GPUI continues to own all interaction, focus, accessibility, popovers, and errors.
- Good, because the shader receives compact value data instead of owning playback or analysis
  state.
- Good, because projectM remains available without being made responsible for application chrome.
- Bad, because the desktop now owns a second native GPU adapter alongside the OpenGL projectM
  adapter.
- Bad, because Metal Shader Language and `CAMetalLayer` make this implementation macOS-specific.
- Bad, because moving the whole application UI to the GPUI overlay surface depends on GPUI Fast's
  composition semantics and requires focused regression tests during GPUI Kit upgrades.
- Neutral, because the ambient field may animate without audio; only the added displacement and
  intensity are represented as audio-reactive.

## Non-goals

- No Rune adoption, GPUI fork, GPUI Kit fork, or private renderer access.
- No replacement, removal, full-window expansion, or preset-browser work for projectM.
- No Windows, Linux, web, wgpu, or cross-platform shader backend in this slice.
- No video decoder, animated-image decoder, downloaded asset, or remote shader content.
- No user-authored shaders, shader plug-in API, runtime theme format, or second product skin.
- No blur-behind-window, Apple Liquid Glass, or sampling content outside the AMP808 window.
- No change to the CPAL callback, decode worker, analysis-worker ownership, or bounded PCM handoff.

## Implementation Plan

- **Affected paths**:
  - `apps/desktop/src/cosmic.rs`: value-only signal, motion policy, renderer status, safe host, and
    testable lifecycle.
  - `apps/desktop/src/cosmic/macos.rs`: GPUI composition attachment, AppKit view/layer ownership,
    Metal pipeline, shader uniforms, presentation, resize, failure, and teardown.
  - `apps/desktop/src/cosmic/cosmic.metal`: compiled-at-runtime full-screen cosmic energy shader.
  - `apps/desktop/src/ui.rs`: base/overlay composition, full-window background host, translucent
    panels, animation-frame scheduling, and accessibility regressions.
  - `apps/desktop/src/skin.rs`: semantic translucent surface alpha values rather than scattered
    literals.
  - `apps/desktop/src/main.rs`: register the cosmic module and retain the existing window contract.
  - `apps/desktop/Cargo.toml` and `Cargo.lock`: enable GPUI Kit's `gpui-fast` feature, add
    target-specific `metal = "0.33"`, and make the already-used macOS Objective-C dependencies
    available to the default desktop build without adding another GPUI facade.
  - `moon.yml`: add explicit macOS cosmic build/test/lint tasks only if the existing desktop tasks
    do not exercise the default feature path.
- **Patterns to follow**:
  - Follow `milkdrop.rs` and `milkdrop/macos.rs` for a safe cloneable host around main-thread native
    state, bounded failure text, and platform-specific unsafe isolation.
  - Use `Window::enable_window_composition`, `WindowComposition::create_native_surface`, and the
    returned public `PlatformSurfaceAttachment`; do not recover GPUI private window fields.
  - Use one behaviour-first TDD slice at a time: signal mapping, motion policy, failure/fallback,
    UI overlay/accessibility, then native presentation.
  - Use `// SAFETY:` comments at every Objective-C or Metal FFI boundary.
  - Keep every per-frame allocation out of the shader presentation path after initialization.
- **Patterns to avoid**:
  - Do not read playback state from the native adapter or send UI work from the audio callback.
  - Do not use `raw-window-handle` for the cosmic surface; GPUI owns its composition attachment.
  - Do not make the native surface interactive, focusable, or an accessibility element.
  - Do not make renderer availability a prerequisite for opening the player or playing audio.
  - Do not hide native failure by fabricating an active status.
- **Configuration**:
  - No user configuration, environment variables, persistent settings, or network assets.
  - The built-in shader is compiled into the executable with `include_str!`.
  - The feature is the default macOS presentation; non-macOS builds retain the opaque GPUI skin.
- **Migration steps**:
  1. Add a failing signal-mapping test for available and unavailable analysis.
  2. Implement the value-only signal and explicit standard/reduced motion policies.
  3. Add a failing lifecycle test proving renderer failure leaves an opaque, usable fallback.
  4. Implement the safe host and macOS adapter behind it.
  5. Add a failing headless GPUI test proving the overlay retains controls, focus, labels, and
     keyboard actions.
  6. Move the player interface to the deferred overlay and introduce semantic translucent panels.
  7. Add the full-window native surface canvas and feed it immutable signal values each frame.
  8. Run desktop and workspace automated gates.
  9. Perform separate native visual, resize, Reduced Motion, interaction, accessibility-tree,
     audio-reactivity, performance, underflow, failure/fallback, and teardown acceptance.

### Verification

- [x] Available analysis maps finite, clamped bass, mid, high, RMS, and peak values into the cosmic
      signal; unavailable, idle, paused, and stale analysis map to zero energy.
- [x] Reduced Motion freezes decorative time and removes audio displacement while preserving a
      static cosmic background and essential playback indicators.
- [x] The production adapter uses only GPUI Kit's public window-composition API and target-specific
      AppKit/Metal APIs; it contains no GPUI private access or framework patch.
- [x] The complete desktop suite passes through GPUI Kit's `gpui-fast` backend with no direct
      `gpui-fast`, `gpui-pre`, or official GPUI dependency in `apps/desktop/Cargo.toml`.
- [x] Metal initialization, shader compilation, drawable, and presentation failures leave the
      opaque AMP808 fallback and all playback controls usable.
- [x] The native surface is non-interactive, absent from the accessibility tree, and never becomes
      first responder.
- [x] The GPUI overlay retains the existing control identifiers, focus traversal, keyboard
      shortcuts, accessible names, progress values, alerts, and MilkDrop mode control.
- [x] Existing projectM MilkDrop mode still renders within its panel above the cosmic background
      and switching modes leaves no incorrectly stacked native surface.
- [ ] Resizing and a real display scale-factor transition keep both native surfaces aligned.
- [x] Native acceptance observes psychedelic cosmic animation, translucent readable panels, and
      visible response to real track energy without fabricated response while paused or idle.
- [x] Native qualification sustains nominal 60 Hz presentation (at least 58 measured FPS) across a
      ten-minute interaction run with zero audio underflow frames and responsive transport/keyboard
      input.
- [x] `cargo test --locked -p amp808-r3tr0` passes.
- [x] `cargo test --locked -p amp808-r3tr0 --features milkdrop-projectm` passes on macOS.
- [x] Strict desktop test-target Clippy and `cargo fmt --all --check` pass; broader pre-existing
      lint failures, if any, are reported separately.

Verification evidence on 2026-11-06: the locked base, projectM, test-hook, and workspace suites
passed; strict test-target Clippy passed with and without projectM; formatting and diff checks
passed. Native macOS observation covered idle and live PCM rendering, pause/resume via Space,
MilkDrop switching via `V`, readable translucent controls, and a 1120x752 to 1320x820 live resize.
The separate all-target strict Clippy audit retains 16 pre-existing production dead-code findings.
A 651-second interactive telemetry run rendered 38,865 frames at 59.671 average FPS with zero
audio underflow frames while covering live playback, pause/resume, mode switching, and resize. The
58 FPS measured threshold accounts for a 60 Hz display's wall-clock scheduling overhead rather
than claiming an unattainable average above its refresh cadence. A real display scale-factor
transition remains open until observed rather than inferred from resize.

## Pros and Cons of the Options

### Native Metal composition surface

- Good, because it is a direct GPU path using a public existing composition boundary.
- Good, because it supports procedural, resolution-independent, audio-reactive animation.
- Good, because the GPUI overlay remains independently interactive and accessible.
- Bad, because it adds macOS-specific shader and lifecycle code.

### Full-window projectM surface

- Good, because projectM and the bounded PCM handoff already exist.
- Good, because `.milk` presets can create cosmic imagery.
- Bad, because it expands deprecated OpenGL into new product chrome.
- Bad, because it entangles the selectable visualiser with the persistent application background.

### Looped video or animated image

- Good, because art direction can be authored without a shader implementation.
- Bad, because decode and texture upload add recurring CPU/GPU transfer work.
- Bad, because audio reaction would be absent or manufactured as an unrelated overlay.

### Rune migration

- Good, because Rune is explicitly designing first-class composition and render-thread motion.
- Bad, because it is pre-release and migration would replace the entire tested UI framework to
  obtain one visual capability already exposed by GPUI Fast composition.

## More Information

- ADR-0022: [Add the macOS-first AMP808-R3TR0 desktop player with GPUI Kit](0022-add-amp808-r3tr0-macos-desktop-player.md)
- ADR-0024: [Host projectM in a native macOS OpenGL subview](0024-host-projectm-in-a-native-macos-opengl-subview.md)
- GPUI Fast composition source: `gpui-fast/src/fast/composition.rs`
- Rune plan: <https://github.com/anomalyco/rune/blob/dev/PLAN.md>

Revisit this decision when GPUI Kit changes composition backends, before a Windows/Linux desktop
release, or if measured native composition performance fails the acceptance threshold.
