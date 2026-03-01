---
status: proposed
date: 2026-03-01
decision-makers: alan
---

# Add oscilloscope visualizer mode (scope-tui inspired)

## Context and Problem Statement

CLIAMP-RS currently renders a 10-band FFT spectrum (`Bars` and `Bricks`). This is useful but visually coarse and does not expose the waveform shape over time. We want an oscilloscope-style visualization in the main view that follows the interaction patterns and idioms used by `scope-tui` (mode-based visuals, trigger-centric scope controls, optional overlays), while preserving CLIAMP's current architecture constraints.

Key constraints:
- Audio callback must remain non-blocking and allocation-light (ADR-0002)
- UI runs at 50ms ticks on the tokio main loop (ADR-0004)
- Visualizer currently uses samples captured via `Tap` ring buffer
- Main layout has a fixed 5-row visualizer area in normal mode
- 808 mode should remain unchanged in the first phase

## Decision

Add a third visualizer mode, `Scope`, implemented as a time-domain oscilloscope renderer in the existing visualizer module. Use a phased rollout:

1. **Phase 1 (MVP trace)**: Add `Scope` mode with basic waveform rendering from recent tap samples (no trigger controls yet).
2. **Phase 2 (trigger controls)**: Add trigger edge/threshold/debounce and stable trigger alignment.
3. **Phase 3 (scope overlays/controls)**: Add scope toggles inspired by `scope-tui` idioms (scatter/line, reference line, peaks, range controls) via a dedicated scope-control key layer to avoid collisions with existing playback keys.

Design choices:
- Reuse `Tap` as the data source for scope rendering (no new audio thread path)
- Keep scope rendering in UI layer only (no DSP path mutation)
- Keep `v` as visualizer mode cycle (`Bars -> Bricks -> Scope`)
- Maintain 808 renderer behavior unchanged for phase 1

**Non-goals (this ADR)**:
- Vectorscope/Lissajous mode
- Replacing the current FFT spectrum modes
- Adding new graphics backends or external rendering processes

## Consequences

* Good, because the UI gains a richer, time-domain visualization without changing the audio backend
* Good, because phased delivery lets us ship useful behavior early and iterate with low risk
* Good, because scope-specific controls can mirror `scope-tui` idioms while preserving existing keybindings
* Bad, because scope rendering in a 5-row area limits fidelity compared to full-screen oscilloscope apps
* Bad, because trigger logic and control surface increase visualizer complexity and test surface

## Implementation Plan

* **Affected paths**: `src/ui/visualizer.rs`, `src/ui/view.rs`, `src/ui/keys.rs`, `src/ui/mod.rs`, `src/config.rs`, `src/main.rs` (and optionally `src/player/tap.rs` for richer sample access)
* **Dependencies**: No new external dependency required for phase 1
* **Patterns to follow**:
  - Keep audio callback untouched and non-blocking
  - Keep visualizer math/policies testable as pure functions where possible
  - Use TDD red-green-refactor for each behavior slice
* **Patterns to avoid**:
  - Do not move rendering work into the audio thread
  - Do not overload existing playback bindings with scope-only actions
  - Do not regress existing Bars/Bricks behavior

### Verification

* [x] `VisMode` cycles through `Bars -> Bricks -> Scope -> Bars`
* [x] Scope renderer emits correct output dimensions for a given width/height
* [x] Scope renderer paints a non-empty trace when non-silent samples are present
* [x] Trigger edge detection supports both rising and falling crossings
* [x] Debounce confirmation filters noisy trigger chatter
* [x] Scope render aligns trace start to detected trigger when enabled
* [ ] Trigger alignment stays stable for periodic signals (rising/falling edge cases)
* [ ] Scope control layer applies overlays/range controls without keybinding regressions
* [ ] Config round-trip persists visualizer mode and scope settings
* [ ] Manual validation confirms no perceptible UI stutter at 50ms ticks during playback

## Alternatives Considered

* **Keep FFT-only visualizer**: Lowest complexity, but does not meet the desired oscilloscope experience
* **Use an external scope process**: Could mimic dedicated scope tools, but adds IPC/process complexity and weak TUI integration
* **Replace all visualizers with scope**: Simplifies mode matrix, but removes existing spectrum styles users already use

## More Information

* `scope-tui` reference: <https://github.com/alemidev/scope-tui>
* ADR-0002 (audio callback constraints)
* ADR-0004 (UI/event loop async model)
