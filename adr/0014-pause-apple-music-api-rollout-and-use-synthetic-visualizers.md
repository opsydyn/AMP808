---
status: accepted
date: 2026-03-07
decision-makers: alan
---

# Pause Apple Music API rollout and use synthetic visualizers for Music.app parity

## Context and Problem Statement

`ADR-0012` established `Music.app` as the supported Apple Music playback surface on macOS.
`ADR-0013` established an Apple Music API metadata client and began a user-visible metadata browser
inside `music-app` mode.

Since then, two constraints have become clear:

1. A real productized Apple Music API path requires Apple Developer Program membership and a safe
   developer-token issuance story.
2. The Apple Music API and `Music.app` automation path do not give us real PCM/sample access for
   true waveform parity with local files, SoundCloud, or HTTP streams.

For the current product phase, the Apple Developer Program cost and token-delivery complexity are
not justified. We still want a polished Apple Music mode, but we need to finish it with the no-cost,
no-extra-account path we already have: `Music.app` transport control on macOS.

The correct short-term product direction is therefore:

- support Apple Music via `Music.app` only
- pause all user-visible features that require Apple Music API tokens
- restore feature parity where practical with synthetic visuals rather than real audio-derived
  waveforms

## Decision

1. The supported Apple Music feature set is limited to the `Music.app` backend from `ADR-0012`.
2. Pause all user-visible and productized work that requires:
   - Apple Developer Program membership
   - Apple Music API developer tokens
   - Apple Music user tokens
   - a token-issuing service or other external web infrastructure
3. Treat the existing Apple Music API client and browser code as dormant/internal work, not as part
   of the supported product surface.
4. Do not require an Apple Developer account for the supported Apple Music experience in the current
   product phase.
5. Achieve Apple Music visual parity through synthetic visualizers in `music-app` mode rather than
   true audio-derived waveform/scope data.
6. Synthetic visualizers in `music-app` mode must:
   - visually resemble the standard player variants
   - animate only from playback state and time, not decoded audio samples
   - pause or stop cleanly when playback pauses or stops
   - be clearly treated as decorative transport-linked visuals, not true waveform analysis
7. Standard view and 808 view should both expose synthetic visualizer support for `music-app` mode
   where the current UI already exposes visualizer selection.
8. No further Apple Music API product work should proceed until a future ADR explicitly reopens:
   - developer-token issuance/delivery
   - user authorization flow
   - product economics for the Apple Developer Program cost

## Non-goals

- No Apple Music API token issuance service.
- No end-user Apple Music API setup flow.
- No requirement for users to provide developer tokens.
- No true audio-derived waveform, scope, or FFT for `Music.app` playback.
- No attempt to capture PCM output from `Music.app`.
- No preview-clip-based waveform approximation from Apple Music metadata.
- No changes to local-file, HTTP-stream, SoundCloud, or provider playback architecture.

## Consequences

- Good, because the supported Apple Music path stays simple: macOS + `Music.app`, no paid developer
  setup required for end users.
- Good, because the current product can close Apple Music mode around a realistic and supportable
  surface.
- Good, because synthetic visualizers preserve perceived parity with the standard player without
  pretending we have real waveform data.
- Good, because this avoids introducing a token-delivery backend before it is economically justified.
- Bad, because Apple Music library/catalog browsing is no longer part of the supported product scope.
- Bad, because synthetic visuals are not true waveform analysis and must be presented honestly.
- Neutral, because the dormant Apple Music API client may remain in the codebase for possible future
  revival.

## Implementation Plan

- **Affected paths**:
  - `src/ui/visualizer.rs` (add synthetic output path for `music-app` mode)
  - `src/ui/mod.rs` (provide transport/time-driven inputs for synthetic visualizer animation)
  - `src/ui/view.rs` (re-enable visualizer rendering and help text for `music-app` mode)
  - `src/ui/view_808.rs` (re-enable visualizer rendering and help text for `music-app` mode)
  - `src/ui/keys.rs` (allow visualizer toggling in `music-app` mode)
  - `src/playback_backend.rs` (expose only the minimum state needed for synthetic animation)
  - `README.md` (document Apple Music support as `Music.app` transport + synthetic visuals only)
  - `adr/0012-add-macos-music-app-playback-backend.md` (note the visualizer exception introduced by this ADR)
  - `adr/0013-use-apple-music-api-as-a-metadata-only-integration.md` (mark as superseded for supported product direction)
- **Patterns to follow**:
  - Keep synthetic visualizer logic inside the UI/visualizer layer, not in `src/player/`.
  - Reuse the existing visualizer modes and styling vocabulary where possible.
  - Drive motion from playback state, elapsed time, and deterministic animation rules.
  - Ensure paused/stopped states visibly reduce or freeze animation.
  - Keep `music-app` startup fully functional with no Apple Music API env vars present.
- **Patterns to avoid**:
  - Do not imply that `music-app` visuals are sampled from Apple Music audio.
  - Do not add new Apple Music API dependencies or runtime requirements.
  - Do not expand the dormant Apple Music metadata browser as part of this slice.
  - Do not block local-player visualizer behavior or regress existing local/stream modes.

### Verification

- [x] `cliamp --backend music-app` works with no Apple Music API env vars set.
- [x] Standard view in `music-app` mode exposes visualizer toggling again.
- [x] 808 view in `music-app` mode exposes visualizer toggling again.
- [x] Synthetic visualizers animate while `Music.app` is playing.
- [x] Synthetic visualizers pause or settle when `Music.app` is paused or stopped.
- [x] Help text and README do not describe Apple Music API setup as part of the supported Apple Music path.
- [x] No supported runtime path requires an Apple Developer account or Apple Music API tokens.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [x] `cargo test` passes.

## Alternatives Considered

- **Continue the Apple Music API rollout now**: rejected because the Apple Developer Program cost and
  token-delivery complexity are not justified for the current product phase.
- **Build a token-issuing service now**: rejected because it adds infrastructure and operational work
  before the user value is proven.
- **Keep `music-app` mode transport-only with no visual parity work**: simpler, but leaves the Apple
  Music experience noticeably below the non-Apple experience.
- **Claim waveform parity without true sample access**: rejected because it would be technically
  misleading.

## More Information

Related existing decisions:

- ADR-0012: Add a macOS Music.app playback backend
- ADR-0013: Use the Apple Music API as a metadata-only integration

This ADR supersedes:

- the supported product rollout direction in ADR-0013

This ADR partially supersedes:

- the `ADR-0012` phase-1 restriction that waveform/scope features are simply hidden or disabled in
  `music-app` mode; after implementation they may be reintroduced as synthetic visuals only

Implementation note, March 7, 2026:

- `src/ui/keys.rs` now allows `v` in both standard and 808 `music-app` mode.
- `src/ui/visualizer.rs` now provides deterministic synthetic band generation driven by transport
  time/state and skips `Scope` when cycling in `music-app` mode.
- `src/ui/view.rs` now renders synthetic visualizer output for `music-app` mode and restores the
  standard-view help text entry for `[v]Vis`.
- `src/ui/view_808.rs` now renders synthetic band-driven output for `music-app` mode in the 808
  layout and exposes `VIS` in the control strip again.
- `README.md` now documents the supported Apple Music surface as `Music.app` transport plus
  synthetic visualizers, with no Apple Music API setup in the user-facing path.
