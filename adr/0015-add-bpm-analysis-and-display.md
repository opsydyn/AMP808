---
status: proposed
date: 2026-03-16
decision-makers: alan
---

# Add BPM analysis and display

## Context and Problem Statement

`cliamp` already exposes rich playback state in both the standard and TR-808 views: time, status,
volume, EQ, visualizer state, and playlist context. What it does not expose is tempo.

A BPM display would improve parity with the rest of the player experience and fit the existing UI
language well:

- the standard view already has a compact status/info area where BPM can live without adding a new
  pane
- the 808 view already presents machine-style transport/status information where BPM is a natural
  fit
- the app already has sample access for local playback through the existing `Tap`/visualizer path,
  so tempo analysis can be derived for backends we control

At the same time, BPM estimation has architectural risks if implemented carelessly:

- running BPM analysis inside `render_*` would create avoidable frame-time regressions
- pushing BPM logic into `src/player/` would couple analysis concerns to the core audio pipeline
- backends without controlled sample access, especially `music-app`, cannot support true BPM
  estimation from live playback in the current architecture

We need a product and architecture decision that adds BPM in an idiomatic way, preserves current
render performance, and makes backend limitations explicit.

## Decision

1. Add a dedicated BPM analysis component in the UI/analysis layer, not in the render layer and not
   in `src/player/`.
2. Phase 1 BPM support is limited to backends with sample access we already control:
   - local decoded files
   - HTTP streams and yt-dlp-resolved tracks only if the existing sample path already provides
     stable windows suitable for estimation
3. `music-app` mode must not claim true BPM detection.
   - In `music-app` mode, BPM is rendered as unavailable rather than estimated or synthesized.
4. BPM analysis must run incrementally during normal app updates/ticks from captured sample windows,
   not during widget rendering.
5. The BPM state shown in the UI must include explicit estimation state, not just a number.
   - minimum states in v1: `estimating`, `locked`, `unavailable`
6. Both standard and 808 views must display BPM using the same underlying state model, with only
   presentation differing by theme/layout.
7. Phase 1 BPM is display-only.
   - no auto-DJ logic
   - no beat-sync actions
   - no sorting/filtering by BPM
   - no persistence of BPM to playlists or config in v1
8. Phase 1 BPM should prefer stability over fast updates.
   - a delayed but believable BPM is better than a noisy rapidly changing readout
9. Tagged BPM metadata lookup is not part of phase 1 unless it already exists in the current decode
   path at negligible cost.
   - the default source of truth in phase 1 is live estimation from samples
10. Phase 2 BPM estimation uses a custom onset-envelope autocorrelation algorithm in
    `src/ui/bpm.rs`.
    - derive a short onset-strength envelope from existing sample windows
    - maintain a rolling envelope history in the UI/analysis layer
    - estimate tempo from bounded-lag autocorrelation
    - only transition to `locked` after repeated near-agreement across updates

## Non-goals

- No BPM estimation in `music-app` mode.
- No fake/synthetic BPM in place of real analysis.
- No BPM analysis inside `render()` or widget construction.
- No new backend capability abstraction beyond what `PlaybackBackend` already exposes.
- No beat grid, beat markers, or rhythm quantization.
- No BPM-based search, filtering, or playlist organization.
- No cache/database for analyzed BPM values in v1.
- No dependency on external analysis tools or subprocesses in v1.

## Consequences

- Good, because BPM becomes available in both UIs without changing the core playback engine.
- Good, because analysis remains outside rendering, preserving a clean architecture boundary.
- Good, because unsupported backends are handled honestly with an explicit unavailable state.
- Good, because the feature can be rolled out incrementally without changing playlist or config
  models.
- Bad, because BPM in v1 may take time to settle and may be absent for some playback modes.
- Bad, because streams and noisy sources may yield less stable estimates than local files.
- Neutral, because later metadata/tag-based BPM support can still be added if justified by a future
  ADR.

## Implementation Plan

- **Affected paths**:
  - `src/ui/mod.rs` (store BPM state, trigger/update analysis on ticks and track changes)
  - `src/ui/view.rs` (render BPM in the standard layout)
  - `src/ui/view_808.rs` (render BPM in the 808 layout)
  - `src/ui/visualizer.rs` or a new sibling analysis module such as `src/ui/bpm.rs` (tempo
    estimation logic and state machine)
  - `src/playback_backend.rs` (capability/helper checks only if needed to distinguish sampled vs
    unsampled backends)
  - `src/player/tap.rs` or existing sample access points only if read-only helper methods are needed
    to expose analysis windows cleanly
  - `README.md` (document BPM availability and backend limitations)
  - `ROADMAP.md` (mark BPM as delivered if implemented)
- **Patterns to follow**:
  - Keep BPM analysis in a narrow module with a simple update/read interface.
  - Feed BPM analysis from existing sample access rather than introducing a parallel audio tap.
  - Reset BPM state when track context changes.
  - Expose BPM to views as plain app state, not through view-local computation.
  - Use deterministic tests with fixed sample fixtures where possible.
- **Patterns to avoid**:
  - Do not compute BPM inside `render_808_*` or `render_*` methods.
  - Do not block the UI thread on heavy full-track analysis.
  - Do not present changing low-confidence estimates as final BPM.
  - Do not report BPM in `music-app` mode unless a future ADR adds a real metadata or playback
    source.

### Phase 1 development plan

1. Add `src/ui/bpm.rs` with:
   - `BpmState`
   - `BpmDisplayState` or equivalent enum (`Estimating`, `Locked`, `Unavailable`)
   - a small incremental analyzer API such as `update(samples: &[f64])` and `snapshot()`
2. Wire BPM state into `App` in `src/ui/mod.rs`.
   - initialize unavailable/estimating state based on backend capability
   - reset on track changes, playlist changes, and backend changes
   - update from existing sample windows during `on_tick()`
3. Render BPM in the standard view.
   - place it near time/status/EQ status without adding a new panel
4. Render BPM in the 808 view.
   - place it in the transport/status strip in a way that matches the machine-style presentation
5. Add tests in vertical TDD slices:
   - state reset on track change
   - unavailable state for `music-app`
   - stable lock behavior for deterministic sample input
   - view rendering of `estimating`, `locked`, and `unavailable`
6. Update docs after implementation.

### Phase 2 algorithm plan

1. Extend `src/ui/bpm.rs` from state-only to incremental estimation.
   - derive onset strength from short fixed-size frames within each incoming sample window
   - append onset values to a rolling buffer covering several seconds of history
2. Compute tempo candidates using autocorrelation on the rolling onset buffer.
   - search only a bounded tempo range suitable for popular music, e.g. `70..190 BPM`
   - convert best lag to BPM using frame-hop duration
3. Add stability gating before exposing a locked BPM.
   - require repeated candidates within a narrow tolerance window before switching from
     `Estimating` to `Locked`
4. Keep the estimator dependency-free in phase 2.
   - no `aubio`
   - no `bpm-analyzer`
   - no `beat-detector`
   - no `timestretch`
5. Revisit external crates only if the custom estimator proves too unstable or too expensive.

### Verification

- [ ] Standard view shows BPM state during local playback.
- [ ] 808 view shows BPM state during local playback.
- [ ] `music-app` mode shows BPM as unavailable rather than a fake estimate.
- [ ] BPM state resets when the current track changes.
- [ ] BPM analysis does not run inside render methods.
- [ ] Automated tests cover state transitions and rendering behavior.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test` passes.

## Alternatives Considered

- **Do nothing**: rejected because BPM is a useful, high-signal addition that fits both UI modes.
- **Compute BPM directly in render paths**: rejected because it is architecturally wrong and risks
  frame regressions.
- **Put BPM logic into `src/player/`**: rejected because tempo estimation is a UI/analysis concern,
  not part of the core playback engine.
- **Show synthetic BPM for unsupported backends**: rejected because it would be misleading.
- **Rely only on metadata tags for BPM**: rejected for phase 1 because it would be incomplete and
  backend-dependent, and current architecture already has sample access for controlled playback.
- **Use `aubio`**: rejected for phase 2 because it adds C/FFI build complexity that is not
  justified until the custom estimator fails to meet quality needs.
- **Use `bpm-analyzer`**: rejected for phase 2 because its API is oriented around its own real-time
  audio capture pipeline rather than the existing `Tap -> App::on_tick()` sample flow in this app.
- **Use `beat-detector`**: rejected for phase 2 because it is older, narrower, and a weaker fit
  for the current incremental sample-window architecture.
- **Use `timestretch` BPM helpers**: rejected for phase 2 because the crate is broader than needed
  and more aligned with offline/preanalysis workflows than a lightweight live TUI readout.

## More Information

Related existing decisions:

- ADR-0002: Use cpal directly for audio output
- ADR-0004: Tokio async with dedicated cpal audio thread
- ADR-0010: Tachyonfx animated 808 chrome and visualizer parity
- ADR-0012: Add a macOS Music.app playback backend
- ADR-0014: Pause Apple Music API rollout and use synthetic visualizers for Music.app parity

Revisit this decision if:

- BPM estimation proves too unstable on the current sample windowing approach
- a future metadata path provides trusted BPM values at low complexity
- the feature begins to require caching, persistence, or backend-specific heuristics beyond this
  scope

Implementation note, March 17, 2026:

- Phase 1 state wiring is complete:
  - `src/ui/bpm.rs` defines `Estimating`, `Locked`, and `Unavailable`
  - `src/ui/mod.rs` initializes BPM state by backend capability and resets it on track changes
  - `src/ui/view.rs` and `src/ui/view_808.rs` now render BPM status
- Phase 2 algorithm choice is now fixed:
  - custom onset-envelope autocorrelation
  - no external BPM dependency in the current product phase
