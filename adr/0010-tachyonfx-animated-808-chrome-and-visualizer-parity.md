---
status: accepted
date: 2026-03-02
decision-makers: alan
---

# Add tachyonfx animated 808 chrome and visualizer parity

## Context and Problem Statement

The main view supports visualizer mode cycling (`v`) including oscilloscope mode, while
the TR-808 view has been fixed to LED-style spectrum columns. This creates a UX gap:
users can toggle mode globally but do not get equivalent behavior in 808 mode. We also
want a more expressive 808 presentation with animated borders inspired by Exabind-style
panel motion, using the already-installed `tachyonfx` dependency.

Constraints:
- Keep audio callback non-blocking/allocation-light (ADR-0002)
- Keep visual effects in UI render path only (ADR-0004)
- Preserve 808 identity (LED column visual should remain available)

## Decision

1. **808 visualizer mode parity via existing `v` cycle**
   - Keep global cycle (`Bars -> Bricks -> Scope`)
   - In 808 view, map modes as:
     - `Bars` => horizontal bars (main-view style)
     - `Bricks` => existing 808 LED columns
     - `Scope` => oscilloscope

2. **Expose visualizer state in 808 UI controls and status**
   - Add `v` (`VIS`) to 808 bottom control pads
   - Show `VIS:<mode>` in 808 status row for immediate feedback

3. **Add tachyonfx-driven animated 808 chrome**
   - Introduce UI-only effect state in `App`
   - Render animated outer/header/focus borders in 808 mode only
   - Drive effect tick from frame-to-frame elapsed time, clamped for stability

## Consequences

* Good, because 808 and main view now share visualizer mode semantics
* Good, because users can verify active visual mode directly in 808 status/help
* Good, because animated chrome adds motion without touching playback/audio threads
* Bad, because 808 rendering complexity increases (additional mode mapping + FX state)
* Neutral, because new border overlays slightly reduce available edge text area

## Implementation Plan

* **Affected paths**:
  - `src/ui/view_808.rs` (mode mapping, rendering branches, controls, status, FX application)
  - `src/ui/mod.rs` (tachyonfx effect state fields)
* **Patterns to follow**:
  - Keep visualization + effects in UI layer
  - Keep behavior testable through small pure mapping/control helpers
  - Keep `Bricks` behavior visually consistent with prior 808 LED presentation
* **Patterns to avoid**:
  - No tachyonfx work in player/audio code
  - No keybinding divergence between main and 808 for visualizer mode control

### Verification

* [x] 808 controls include `v` visualizer toggle
* [x] 808 render mode mapping is explicit (`Bars/Bricks/Scope`)
* [x] 808 status row shows active visualizer mode label
* [x] 808 mode renders horizontal bars and oscilloscope via `v`
* [x] `cargo check` and `cargo test` pass after integration

## Alternatives Considered

* **Keep 808 fixed to LED columns only**: lower complexity, but inconsistent mode behavior
* **Add separate 808-only keybinding for visuals**: would diverge from shared controls and
  increase user confusion
* **Use static decorative borders without tachyonfx**: lower implementation cost, but misses
  requested animated style direction

## More Information

* Exabind inspiration: <https://junkdog.github.io/exabind/>
* Tachyonfx docs: <https://docs.rs/tachyonfx/latest/tachyonfx/>
