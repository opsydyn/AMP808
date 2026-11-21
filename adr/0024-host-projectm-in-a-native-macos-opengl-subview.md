---
status: accepted
date: 2026-11-05
decision-makers: alan
consulted: projectM macOS frontend source, GPUI Kit pinned source, ADR-0023 spike evidence
informed: AMP808 contributors
supersedes: ADR-0023 when accepted
---

# Host projectM in a native macOS OpenGL subview

## Context and Problem Statement

ADR-0023 selected a dedicated CGL renderer worker which would read projectM's output back to RGBA
and upload that frame into GPUI. The first test-first native spike compiled projectM 4.1.2, created
core-profile CGL contexts, and then disproved the assumed headless drawable path:

- `CGLCreatePBuffer` returned `kCGLBadDrawable`;
- `CGLSetOffScreen` returned `kCGLBadOffScreen`; and
- projectM's final render pass binds draw framebuffer `0`, so a real default drawable is required.

This triggers ADR-0023's explicit stop condition. Continuing the CGL readback design would require
either a hidden window, a helper process, or a local projectM fork that adds a custom output
framebuffer. Each adds more lifecycle or maintenance complexity than the original decision allowed.

The official projectM macOS Music plug-in demonstrates the supported native model: create an
`NSOpenGLView`, add it to the host view hierarchy, make its context current, and render projectM in
the view. The pinned GPUI implementation used by GPUI Kit implements `HasWindowHandle` for
`gpui::Window`; on macOS that public handle contains the live GPUI `NSView`. AMP808 can therefore
mount a native child view without a GPUI fork or private renderer access.

How should AMP808-R3TR0 host that view while preserving GPUI controls, keyboard accessibility, and
the bounded audio contract?

## Decision Drivers

- Render genuine `.milk` presets through unmodified projectM.
- Use a supported macOS OpenGL drawable rather than relying on rejected headless CGL APIs.
- Keep the CPAL callback allocation-free, non-blocking, and unaware of projectM or AppKit.
- Keep GPUI responsible for the visible application window, controls, focus, and accessibility.
- Avoid a second visible window, helper process, projectM fork, and per-frame GPU-to-CPU copy.
- Use only public GPUI/raw-window-handle contracts to locate the host `NSView`.
- Make creation, resize, hide, failure, and teardown deterministic and testable.

## Considered Options

- Mount a non-interactive `NSOpenGLView` child inside the GPUI window.
- Create a hidden OpenGL window and read pixels back into GPUI.
- Run projectM in a helper process and transfer frames over shared memory.
- Patch projectM to render into an application-supplied framebuffer or texture.
- Abandon `.milk` compatibility for a native Metal/WGSL visualiser.

## Proposed Decision Outcome

Replace ADR-0023's CGL worker/readback boundary with a macOS-only native view adapter behind the
existing `milkdrop-projectm` feature.

The adapter obtains GPUI's public AppKit raw window handle on the main thread, creates a
non-interactive `NSOpenGLView` child with a 3.2 core profile, and mounts it only over the MilkDrop
visualiser bounds. It owns the OpenGL context and projectM handle for their full lifetime. GPUI's
existing animation-frame loop synchronizes the native view frame, supplies completed PCM windows
from the bounded analysis handoff, and requests a view redraw. ProjectM rendering occurs with that
view's context current. Hiding or leaving MilkDrop mode hides the native child view without
affecting Spectrum or Waveform.

The native view must not accept first responder status, mouse input, or accessibility focus. GPUI's
existing visualiser control remains the accessible control and owns the keyboard `V` cycle. The
adapter exposes only safe lifecycle and data methods; Objective-C, OpenGL, raw handles, and
projectM FFI remain inside the macOS module.

The PCM handoff remains bounded and lossy for visual data. The analysis worker is still the only
producer, and projectM consumes available samples during a UI animation frame. Playback never
waits for the native view. When rendering cannot initialize or a preset fails, the view stays
detached or hidden and GPUI displays an explicit recoverable error.

Native failure qualification uses an opt-in `milkdrop-test-hooks` Cargo feature that is disabled
from default and packaged builds. Only when that feature is compiled, the adapter reads
`AMP808_MILKDROP_INJECT_FAILURE`; accepted values are `initialization` and `preset`. The configured
failure is consumed once by the existing `MilkdropViewHost`, returns through the normal
`MilkdropError` path, and leaves a subsequent track load free to initialize normally. Unknown
values also fail honestly instead of silently weakening the qualification. The hook must not
panic, replace the embedded preset, add a dependency, or change the CPAL/analysis boundaries.

### Consequences

- Good, because this follows projectM's demonstrated macOS integration model.
- Good, because it avoids both framebuffer readback and a second Metal upload.
- Good, because it uses GPUI's public `raw-window-handle` implementation rather than private GPUI
  state or renderer APIs.
- Good, because the native view can be created, resized, hidden, and destroyed on AppKit's main
  thread.
- Bad, because a native child view must be kept geometrically aligned with GPUI layout and window
  scaling.
- Bad, because AppKit/OpenGL/projectM work now shares the UI thread and must be measured for frame
  time and control responsiveness.
- Bad, because OpenGL remains deprecated on macOS and native-view interop is macOS-specific.
- Bad, because acceptance builds gain a deliberately non-production environment contract that
  must remain compile-time gated and omitted from packaging.
- Neutral, because the bounded PCM contract survives but the RGBA frame and renderer-worker types
  from the spike become unnecessary.

## Non-goals

- No hidden or second visible window and no helper process.
- No projectM fork or patch to expose an output framebuffer.
- No GPUI fork, GPUI private API, or direct access to GPUI's Metal renderer.
- No interactive controls or accessibility nodes inside the OpenGL child view.
- No community preset pack, preset browser, transitions, full-screen mode, or packaging work.
- No claim of acceptable performance until native measurements are recorded.

## Implementation Plan

1. Remove the invalid drawable-less CGL renderer and its RGBA worker/frame contracts.
2. Retain the tested bounded PCM producer/consumer and analysis-worker forwarding.
3. Add a failing macOS adapter test for native-view lifecycle and preset initialization using the
   original AMP808 preset.
4. Implement the safe `NSOpenGLView` adapter using AppKit and projectM FFI on the main thread.
5. Add a GPUI element seam that reports visualiser bounds and synchronizes the native child frame
   without exposing AppKit types to the rest of the UI.
6. Wire MilkDrop mode to attach/show/render the view and other modes to hide it; preserve honest
   unavailable and failed states.
7. Verify keyboard focus, hit testing, accessibility tree, resize, pause/resume, mode switching,
   file replacement, and teardown.
8. Run automated gates and then a separate native macOS acceptance/performance pass.

### Verification

- [x] The adapter uses `gpui::Window`'s public AppKit `raw-window-handle`; no GPUI source patch or
      private field access is present.
- [ ] The native OpenGL view is a child of the GPUI view only while the owning GPUI window is live.
- [x] The child view cannot become first responder, receive pointer input, or appear as a separate
      accessibility element.
- [x] `V` cycles `Spectrum -> Waveform -> MilkDrop -> Spectrum` with focus retained by GPUI.
- [x] The original AMP808 `.milk` preset initializes and produces visible audio-reactive output.
- [x] PCM reaches projectM only through the bounded analysis handoff; no projectM, OpenGL, AppKit,
      allocation, or blocking work is added to the CPAL callback.
- [ ] Switching away, pausing, loading another file, and closing the window do not leave a visible
      child view or live projectM/OpenGL resources.
- [x] Initialization and preset failures leave playback operational and show an honest error.
- [ ] Resize and scale-factor changes keep the native view aligned with the GPUI visualiser panel.
- [x] Native acceptance sustains at least 30 FPS at `640x360`, introduces no audible underruns over
      ten minutes, and keeps transport/keyboard interaction responsive.
- [x] `cargo test --locked -p amp808-r3tr0` passes.
- [x] `cargo test --locked -p amp808-r3tr0 --features milkdrop-projectm` passes on macOS.
- [ ] `cargo clippy --locked -p amp808-r3tr0 --all-targets --features milkdrop-projectm -- -D warnings`
      and `cargo fmt --all --check` pass.

## Rejected Alternatives

### Hidden OpenGL window with frame readback

This supplies framebuffer `0`, but it retains the copy cost and adds an invisible window lifecycle
solely to work around the original assumption.

### Helper process

This provides strong fault and event-loop isolation, but IPC, shared-memory frames, process
supervision, signing, and packaging are disproportionate for the first in-window slice.

### Local projectM framebuffer patch

This could preserve worker ownership and avoid a native subview, but AMP808 would own a permanent
C++ fork of the compatibility engine before proving product behavior.

### Metal/WGSL replacement

This is attractive for a future native renderer, but it does not meet the approved requirement to
load genuine `.milk` presets.

## More Information

### 2026-11-06 native acceptance evidence

- Build identity: checkout `03780a16664139b693fe00d8a53890dc32804109` with the current
  uncommitted ADR-0024 implementation, launched using
  `cargo run --locked -p amp808-r3tr0 --features milkdrop-projectm`.
- Normal fixtures: `/private/tmp/amp808-manual-acceptance.wav` and a 60-second PCM repeat at
  `/private/tmp/amp808-milkdrop-acceptance-60s.wav`; corrupt fixture:
  `/private/tmp/amp808-manual-corrupt.wav`.
- Passed: the original preset rendered inside the GPUI visualiser panel and changed across live
  frames while the playhead advanced from 00:12 to 00:27.
- Passed: pausing held both the playhead at 00:54 and the same MilkDrop frame across separate
  observations; resuming and seeking remained responsive.
- Passed: leaving MilkDrop immediately revealed Spectrum with no native surface left visible;
  loading another normal file while MilkDrop was selected replaced playback without a crash.
- Passed: resizing the window kept the OpenGL child aligned to the visualiser panel. A separate
  display scale-factor transition was not exercised.
- Passed: native Tab traversal moved from Open File through transport controls to the visualiser
  control without entering the OpenGL child; Enter switched to Spectrum and retained focus on the
  GPUI visualiser control. VoiceOver tree inspection and pointer hit-testing remain unobserved.
- Passed: the corrupt fixture showed
  `/private/tmp/amp808-manual-corrupt.wav: unsupported feature: core (probe): no suitable format reader found`,
  cleared the active session safely, and a subsequent normal-file load recovered playback.
- Partial: closing the native window removed it without a crash or visible child surface. The bare
  development process remained in its event loop until the launcher was interrupted, so resource
  release is supported by the adapter's ownership/drop path but was not independently measured.
- Open: display scale-factor transition and VoiceOver traversal were not run.

### 2026-11-06 ten-minute native performance qualification

- Build identity: checkout `03780a16664139b693fe00d8a53890dc32804109` with the current
  uncommitted ADR-0024 implementation. The release executable and temporary application-bundle
  copy both had SHA-256
  `68edad0b8e207ee0e255edf10944b3d3807ab3863afa132d5c35e120d4576f2f`.
- Fixture: `/private/tmp/amp808-milkdrop-acceptance-620s.wav`, played through the release build with
  `AMP808_MILKDROP_METRICS=1`. The native visualiser panel exceeded the `640x360` baseline.
- Passed: the uninterrupted qualifying interval reported 36,068 presented MilkDrop frames at
  601.134 seconds, an average of 60.000 FPS, with `underflow_frames=0`. Telemetry remained
  continuous through 611.151 seconds at 60.000 FPS and zero underflows.
- Passed: the native end state reported `PCM / ENDED`, `MilkDrop paused`, and 10:20 of 10:20. The
  earlier acceptance pass separately established responsive pause, resume, seek, and keyboard
  visualiser controls.
- Excluded: attempts interrupted by an output-device transition, macOS locking, or a stalled
  presentation loop were discarded and did not contribute to the qualifying interval.
- Still open: display scale-factor transition and VoiceOver traversal.

### 2026-11-06 pointer and accessibility follow-up

- Build identity remained checkout `03780a16664139b693fe00d8a53890dc32804109` and release binary
  SHA-256 `68edad0b8e207ee0e255edf10944b3d3807ab3863afa132d5c35e120d4576f2f`.
- Passed: with MilkDrop live, a direct pointer click in the native OpenGL surface did not capture
  input or first-responder state. The next `V` shortcut reached GPUI and switched immediately to
  Spectrum. The accessibility tree continued to contain only the GPUI visualiser status/control,
  with no separate OpenGL child element.
- Incomplete: macOS System Settings accepted the temporary VoiceOver toggle, but the VoiceOver
  launch agent immediately returned to `state = not running`; no VoiceOver cursor or focus ring
  appeared. The setting was restored to `off`, so spoken traversal remains unqualified.
- Blocked by environment: `system_profiler SPDisplaysDataType` exposed the Apple M4 Pro GPU but no
  second attached display or alternative backing scale. A real scale-factor transition could not
  be exercised in this session.
- Not run: the current product has no projectM initialization or preset-failure injection hook.
  Product code and the embedded preset were left unchanged rather than manufacturing acceptance
  evidence from a modified binary.

### 2026-11-06 deterministic projectM failure qualification

- Added the non-default `milkdrop-test-hooks` Cargo feature. Only that feature reads
  `AMP808_MILKDROP_INJECT_FAILURE`; `initialization`, `preset`, and invalid values are consumed once
  and returned through the existing `MilkdropError` path. Default and packaged builds do not read
  the hook.
- TDD red phases failed for the intended missing behavior: unresolved injection types, missing
  one-shot host ownership, and missing stage-specific errors. The final hook-feature suite passed
  65 tests; the normal `milkdrop-projectm` suite passed 62 tests.
- Strict test-target Clippy passed with both `milkdrop-test-hooks` and `milkdrop-projectm`, and
  `cargo fmt --all --check` passed. The broader all-target Clippy command remains blocked by 16
  pre-existing desktop dead-code findings, so its combined verification checkbox remains open.
- Native acceptance used the isolated hook-enabled release binary at
  `/private/tmp/AMP808-R3TR0-Failure.app`, SHA-256
  `21123b02137cf380b3efa3f31afaa38486e124179918a9e764b3c1b1a6a9c957`. The previously qualified
  release bundle remained unchanged at SHA-256
  `68edad0b8e207ee0e255edf10944b3d3807ab3863afa132d5c35e120d4576f2f`.
- Passed initialization injection: the accessible state reported
  `MilkDrop error: MilkDrop backend failed: injected projectM initialization failure` while
  playback remained `PCM / LIVE`; reloading the same track recovered to `MilkDrop active` in the
  same process.
- Passed preset injection with the equivalent stage-specific error, live playback, and same-process
  reload recovery to `MilkDrop active`.
- Remaining native acceptance gaps are a real display scale-factor transition and working
  VoiceOver spoken traversal in a session where the VoiceOver service stays running.

- ADR-0023: [Adopt projectM for authentic MilkDrop visualisation](0023-adopt-projectm-for-authentic-milkdrop-visualisation.md)
- projectM macOS plug-in: <https://github.com/projectM-visualizer/frontend-music-plug-in>
- projectM core: <https://github.com/projectM-visualizer/projectm>
- GPUI window handles: <https://github.com/zed-industries/zed/blob/main/crates/gpui/src/window.rs>
- GPUI macOS handle implementation: <https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/src/window.rs>

Boundary review verdict: **safe if accepted and kept main-thread confined**. Audio owns audible
frames; analysis owns bounded visual PCM; the native adapter owns the child view, OpenGL context,
and projectM; GPUI owns application interaction and accessibility. No framework object crosses the
audio boundary.

Simplicity check: the direct child view deletes the invalid off-screen drawable, RGBA readback,
frame publication, and second texture upload. It adds one necessary native seam: mapping GPUI
layout bounds to an AppKit child view.

Review this proposal before any native-view implementation. If public raw handles cannot safely
support child-view lifecycle, hit testing, or geometry synchronization, reject this ADR and assess
the helper-process boundary separately.

Accepted by Alan on 2026-11-05.
