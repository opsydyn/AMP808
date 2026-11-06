# Deterministic UI Challenger Research

**Status:** Research only — no architecture decision, dependency approval, or implementation plan  
**Date:** 2026-11-06  
**Applies to:** AMP808-R3TR0 desktop presentation and possible reusable UI research  
**Revisit when:** a measured product requirement cannot be met through GPUI Kit and GPUI's public APIs

## Purpose

Explore whether a small, framework-agnostic deterministic layer could challenge and strengthen
AMP808's GPUI frontend without forking GPUI, depending on private GPUI renderer internals, or
building a Rune-like UI framework.

This note does not reopen [ADR-0022](../../adr/0022-add-amp808-r3tr0-macos-desktop-player.md),
which selects GPUI Kit for AMP808-R3TR0, or
[ADR-0024](../../adr/0024-host-projectm-in-a-native-macos-opengl-subview.md), which owns the
projectM/AppKit boundary. Any proposal to change those decisions requires a new ADR.

## Current Position

Keep GPUI Kit as the shipping desktop frontend. Do not create a new renderer, compositor, scene
tree, component library, platform shell, or multi-framework adapter workspace from this research.

The promising research direction is narrower:

- deterministic motion mathematics that does not own presentation;
- product-semantic inspection and replay that does not mirror a framework element tree; and
- public-API GPUI adapters that can be removed without changing AMP808's playback or domain model.

The challenger should test a capability and expose a boundary. It should not become a second
application framework alongside GPUI.

## Context

Rune demonstrates that mature Rust foundations exist below a UI framework:

| Problem | Available foundation |
| --- | --- |
| GPU access | `wgpu` |
| Vector rendering | Vello / `vello_gpu` |
| Layout | Taffy |
| Text layout and shaping | Parley, skrifa, fontique, HarfRust |
| Accessibility | AccessKit |
| Window management | winit plus platform APIs |
| Large indexed collections | sum-tree structures |

These crates solve important algorithms and platform access, but they do not collectively provide
frame scheduling, state ownership, retained presentation, GPU resource lifetime, hit testing,
native-view composition, accessibility synchronization, or a coherent testing contract. Owning
those integrations would amount to building another UI framework even if no rasterizer were
written from scratch.

Rune's architecture and tooling remain useful reference material:

- <https://github.com/anomalyco/rune>
- <https://github.com/anomalyco/rune/blob/dev/PLAN.md>
- <https://github.com/anomalyco/rune/blob/dev/crates/rune/README.md>

## Product Evidence Today

- AMP808 already keeps audio decoding, PCM analysis, and the CPAL callback outside the UI runtime.
- The projectM visualizer uses GPUI's public raw-window-handle contract to mount a non-interactive
  `NSOpenGLView`; it does not require a GPUI fork or private renderer access.
- GPUI Kit currently provides controls, focus, accessibility semantics, file selection, keyboard
  actions, reduced-motion state, and headless interaction tests.
- No observed AMP808 defect currently proves that GPUI frame scheduling, composition, or animation
  prevents a required product behavior.
- The interrupted ten-minute MilkDrop qualification was interrupted by macOS locking, not by a
  demonstrated GPUI renderer failure.

The research must therefore begin with a measured product problem. A synthetic benchmark may
explain a framework boundary, but it is not by itself authority to replace working product
architecture.

## Research Thesis

A useful challenger may own deterministic values and evidence while leaving layout, text,
accessibility, input, components, window management, and final presentation with GPUI.

```text
AMP808 product state
        |
        v
deterministic protocol
clock · targets · motion tracks · semantic events · trace
        |
        v
GPUI public adapter
apply targets · request redraw · observe presentation
        |
        v
GPUI Kit / GPUI
layout · controls · accessibility · window · renderer
```

This boundary can improve reproducibility and inspection. It cannot guarantee animation while
GPUI's presentation thread is blocked. Independent presentation is a separate capability that
requires ownership of a native or GPU surface and must not be smuggled into the deterministic core.

## Candidate Modules

The names below describe research boundaries, not approved crates.

### Deterministic motion kernel

Potential responsibilities:

- monotonic or injected logical clock;
- analytic spring and interpolation tracks;
- interruption and retargeting with velocity continuity;
- explicit reduced-motion policy inputs;
- deterministic sampling independent of host frame cadence;
- trace serialization suitable for property and replay tests.

It must not own a render loop, GPU device, window, layout tree, component model, or framework state.

### Product-semantic inspection

Inspect AMP808 behavior rather than attempting to normalize framework element trees:

- transport and decode state;
- playhead, duration, volume, and current track;
- visualizer mode, availability, and frame counters;
- underflow and recoverable error diagnostics;
- available commands and their enabled state;
- focus intent where it is part of the product contract.

Each frontend remains responsible for proving that its rendered accessibility tree and input
behavior expose those semantics correctly.

### Deterministic trace and replay

A bounded protocol could record logical time, semantic input, expected product-state transitions,
and presentation observations. It should be usable by a pure test harness and a GPUI adapter
without making either depend on the other.

Do not record credentials, local file contents, raw audio, or unbounded per-frame data.

## Integration Levels

### Level 1 — GPUI public APIs

Allowed research scope:

- deterministic state and motion sampling;
- GPUI actions and redraw requests;
- application-semantic inspection;
- headless interaction and trace comparison;
- frame and latency observations available through supported APIs.

This is the preferred boundary. It must not retain GPUI objects inside portable modules.

### Level 2 — Explicit native surface

Use only for a measured capability that cannot be expressed through GPUI presentation, such as the
existing projectM `NSOpenGLView`. Native surfaces remain platform adapters with explicit lifecycle,
geometry, focus, accessibility, and teardown responsibilities.

They do not make motion or composition generally framework-agnostic.

### Level 3 — Renderer integration

Direct GPUI renderer access, a GPUI source patch, or a permanent fork is outside this research
boundary. Reaching this level is a stop condition, not an implementation milestone.

If an essential product requirement truly needs renderer ownership, compare a separate native
surface, upstream GPUI work, Rune adoption, and a different frontend through a new ADR. Do not grow
the Level 1 challenger into a renderer by stealth.

## Deliberately Omitted Scope

- a `compositor-core` abstraction over nested layers, masks, blur, or backdrop sampling;
- a shared scene tree or retained element model;
- Vello/wgpu rendering owned by AMP808;
- GPUI, Rune, Iced, or other framework adapter matrices;
- a generic component library or design system;
- native glass emulation or cross-platform window-shell parity;
- virtualized million-row lists or large-text infrastructure;
- independent 120 Hz presentation without a measured AMP808 requirement;
- extraction into a new repository before two real consumers prove the same contract.

## Minimum Credible Challenger

When a measured requirement justifies revisiting this research, the smallest credible experiment
is one deterministic behavior exercised through GPUI's public APIs:

1. Express one AMP808 decorative transition as target values and a logical-time motion track.
2. Prove continuity, retargeting, convergence, and reduced-motion behavior with pure tests.
3. Drive the same track through a removable GPUI adapter without changing playback, analysis,
   accessibility, or projectM ownership.
4. Capture a bounded semantic trace and compare deterministic results under regular and irregular
   host frame cadence.
5. Measure the original product defect again. Do not substitute a passing pure test for native
   presentation evidence.

This experiment may show that deterministic semantics are useful while independent presentation
is impossible through public GPUI APIs. That is a valid result, not a reason to broaden the crate.

## Success Conditions

- The motivating user-visible defect is reproduced and measured before introducing new structure.
- Portable logic contains no GPUI, AppKit, winit, wgpu, Vello, Taffy, Parley, or AccessKit types.
- The GPUI adapter uses only documented public APIs and can be deleted without changing product
  state or playback behavior.
- Fixed inputs and logical time produce deterministic traces across repeated runs.
- Native acceptance separately confirms the intended visible behavior and accessibility remains
  intact.
- The result removes uncertainty or code from a real product path; it does not merely establish a
  new platform roadmap.

## Stop Conditions

Stop and return to architecture review if the experiment requires any of the following:

- a GPUI fork, patch, private renderer API, or copied GPUI internals;
- ownership of a GPU surface or second general-purpose renderer;
- a shared scene tree, layout engine, text system, hit-testing model, or accessibility bridge;
- framework-specific behavior hidden inside a supposedly portable protocol;
- synchronization that could block, allocate, log, or dispatch UI work from the audio callback;
- weakening the existing keyboard, reduced-motion, accessibility, or native MilkDrop evidence;
- speculative adapters for frameworks that have no active AMP808 consumer;
- a new repository or public crate before the contract has two proven consumers.

## Questions for Revisit

1. What measured AMP808 behavior is GPUI currently preventing?
2. Is the requirement deterministic semantics, smooth host-driven animation, or independent
   presentation? These are different scopes.
3. Can the requirement be met through GPUI Kit or GPUI public APIs?
4. Does a pure protocol remove meaningful duplication across at least two real consumers?
5. Would contributing a missing public seam upstream be smaller than owning an adapter forever?
6. If renderer ownership is essential, is adopting an existing renderer-owning framework more
   responsible than creating another one?

## Simplicity Check

```text
outcome:
Preserve a future path to deterministic, inspectable UI behavior without replacing GPUI.

options checked:
Extend GPUI through public APIs; build a modular challenger; migrate to Rune; build a renderer and
multi-framework toolkit.

selected path:
Research a pure deterministic protocol and removable GPUI adapter only after a measured product
need exists.

entanglement avoided:
Renderer ownership, native shell ownership, shared scene semantics, private GPUI integration, and
speculative framework adapters.

safeguards preserved:
The bounded audio boundary, GPUI accessibility and keyboard ownership, projectM's native adapter,
and separate automated versus native acceptance evidence.

omitted scope:
Implementation, crate creation, dependency selection, performance claims, and migration planning.

remaining uncertainty:
Whether AMP808 will expose a product requirement that benefits from deterministic UI protocols but
does not require independent presentation.
```

## Research Outcome

Retain this as a challenger hypothesis. Continue the current GPUI Kit product path. Revisit only
with a measured requirement and create or supersede an ADR before changing runtime architecture or
adding a durable dependency.
