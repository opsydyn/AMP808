---
status: accepted
date: 2026-11-07
decision-makers: alan
consulted: ADR-0003, ADR-0005, ADR-0022, ADR-0025
informed: AMP808 contributors
---

# Stage remote media acquisition behind a provider-agnostic desktop job

## Context and Problem Statement

The Ratatui application can resolve remote media through `yt-dlp`, prepare unsupported media with
FFmpeg, and then hand a local playable result to its audio path. AMP808-R3TR0 deliberately began
as a local-file player, so its GPUI state currently has no honest way to describe remote source
inspection, download progress, media preparation, cancellation, or recovery.

How should the desktop product introduce that user-visible lifecycle without copying the TUI
application, coupling GPUI to subprocess output, or implying that SoundCloud and other remote
providers already work?

## Decision

Introduce an application-internal, provider-agnostic `RemoteSourceJob` state model in the desktop
crate before adding any network or subprocess adapter.

- A job moves through explicit `Inspecting`, `Downloading`, `Preparing`, `Ready`, `Failed`, and
  `Cancelled` states by applying typed events.
- Download progress may be determinate or indeterminate. Known totals are bounded, and reported
  bytes cannot move backwards.
- Cancellation is valid only while work is active. Retry is valid only after failure and returns
  the same job to inspection.
- GPUI renders the job state and sends user intentions such as cancel or retry; it does not parse
  provider output or own acquisition work.
- The first slice uses a clearly labelled deterministic preview job only. It performs no network,
  `yt-dlp`, FFmpeg, filesystem, persistence, or playback work.
- Future provider and preparation adapters may emit the same state events. A later decision must
  define their process ownership, temporary-file lifetime, cancellation, recovery, and hand-off to
  the existing local-path playback boundary.

ADR-0022's remote-media non-goal remains true for its original first-playable slice. This decision
extends the evolved desktop product only with a state and presentation boundary; it does not claim
remote playback support.

## Consequences

- Good, because the complete lifecycle and its legal transitions are deterministic and testable
  without network access or subprocess fixtures.
- Good, because provider details remain outside GPUI and existing local playback remains unchanged.
- Good, because the UI can establish visible progress, cancellation, retry, and accessibility
  semantics before irreversible adapter choices.
- Bad, because the preview surface is intentionally non-functional and must be labelled so users do
  not mistake it for completed SoundCloud support.
- Bad, because a future adapter must translate external progress formats into this smaller model.
- Neutral, because the model remains private to the desktop crate until a second application proves
  a genuinely shared contract.

## Boundary Review

- **Remote acquisition** owns source identity, lifecycle state, transition validation, and progress.
- **Desktop UI** owns presentation and translates user interaction into typed job events.
- **Provider adapters** will own provider inspection and media retrieval, but are absent here.
- **Media preparation adapters** will own FFmpeg process work, but are absent here.
- **Playback** continues to own decoded local media and receives no remote job dependency.

Allowed dependency: desktop UI -> remote state model. The remote model must not import GPUI,
playback, SQLite, provider, `yt-dlp`, FFmpeg, or platform-process types. Verdict: **safe with noted
risk**; future adapters will require a further boundary review before they can produce playable
media.

## Non-goals

- No URL entry, SoundCloud playback, provider discovery, remote saving, or catalogue persistence.
- No `yt-dlp` or FFmpeg invocation, process supervision, temporary files, networking, or new crate.
- No change to the CPAL callback, decoder, local library, queue, or playback state machine.
- No extraction to a shared crate, public provider trait, or copy of the TUI application layer.
- No claim that the preview progress represents live work.

## Implementation Plan

1. Add failing tests for legal lifecycle transitions, progress invariants, cancellation, and retry,
   implementing the smallest pure Rust model after each failure.
2. Add a failing GPUI render test for a labelled preview job and accessible progress semantics.
3. Render a deterministic cosmic-styled progress surface with cancel and recovery actions.
4. Verify desktop tests, the projectM feature, strict test-target Clippy, formatting, and diff
   hygiene without changing existing playback behavior.

### Verification

- [x] Legal lifecycle transitions and invalid transitions are covered by unit tests.
- [x] Known progress is monotonic and bounded; unknown totals remain indeterminate.
- [x] Cancel and retry availability follows the state model.
- [x] The preview is explicitly labelled and exposes status and progress semantics.
- [x] No dependency, provider adapter, subprocess, persistence, or playback path changes.
- [x] Base, projectM, strict test-target Clippy, formatting, and diff gates pass.

## Options Considered

### Copy the TUI resolver and FFmpeg flow into the desktop application

This would reach working remote playback faster, but would copy application ownership and process
assumptions into a UI that does not yet have a lifecycle or cancellation contract.

### Store acquisition booleans directly in `ui.rs`

This is initially small but permits contradictory states, hides transition rules in callbacks, and
makes adapter and accessibility behavior difficult to test independently.

### Extract a shared remote-media crate immediately

This may become appropriate after desktop and TUI behavior converge, but doing it now would freeze
an abstraction before the desktop adapter exists and expand the first slice unnecessarily.

## More Information

### 2026-11-07: Present full-file output as a yt-dlp integration

The product may expose `yt-dlp`'s existing capability to produce a complete local file. AMP808 is
an integration with that user-operated tool; it is not the source, host, or provider of the media.

- The user-facing action is **USE YT-DLP**, consistent with other explicit source options.
- User-facing progress uses transfer and preparation language rather than a **DOWNLOAD** action.
  Internal state and adapter events may retain precise technical names such as `Downloading`.
- Product terms and conditions must describe the integration boundary, user responsibility, and
  applicable rights before the capability is publicly released.
- This clarification does not itself add the adapter, legal text, provider support, persistence,
  or playback hand-off.
