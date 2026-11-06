---
status: accepted
date: 2026-11-06
decision-makers: alan
consulted: ADR-0022, ADR-0025, GPUI Kit 0.9.0 metadata, GPUI Fast 0.1.13 source
informed: AMP808 contributors
---

# Upgrade GPUI Kit to restore native macOS launch

## Context and Problem Statement

AMP808-R3TR0 uses GPUI Kit's `gpui-fast` feature to place a native Metal cosmic surface below a
transparent GPUI overlay. The pinned GPUI Kit 0.7.3 resolves GPUI Fast Apple 0.1.10. On the current
macOS runtime, that release aborts before the first window appears because its raw Objective-C
message for `CAMetalLayer.colorspace` declares the wrong return type.

GPUI Fast Apple 0.1.13 replaces the raw message with the typed `CAMetalLayer` accessor. GPUI Kit
0.9.0 selects that repaired backend, but its macOS crates use `Option::map_or_default`, which is not
available on the workspace's Rust 1.95.0 toolchain. An isolated dependency probe fails on Rust
1.95.0 and 1.97.1 and succeeds on Rust 1.99.1.

How should AMP808 restore native launch without disabling its composition architecture or carrying
a private framework fork?

## Decision Drivers

- Restore native macOS launch before qualifying the local Library UI.
- Preserve ADR-0025's GPUI Fast composition boundary and cosmic Metal underlay.
- Keep GPUI Kit as the desktop application's only GPUI ecosystem facade.
- Prefer released dependencies over a private patch or permanent fork.
- Keep the build reproducible through an exact GPUI Kit version and pinned Rust toolchain.
- Retain the existing audio, MilkDrop, accessibility, and local-library boundaries.
- Verify application API changes and runtime behavior rather than inferring compatibility from a
  dependency-only build.

## Considered Options

- Upgrade to GPUI Kit 0.9.0 / GPUI Fast 0.1.13 and Rust 1.99.1.
- Vendor a narrow patch for GPUI Fast Apple 0.1.10.
- Disable GPUI Fast composition and fall back to an opaque player.
- Wait for another upstream release while leaving native launch blocked.

## Decision Outcome

Chosen option: **upgrade to GPUI Kit 0.9.0 / GPUI Fast 0.1.13 and Rust 1.99.1**. This uses a
released backend containing the typed Core Animation call, avoids owning third-party source, and
preserves the public composition architecture selected by ADR-0025.

The workspace toolchain moves from Rust 1.95.0 to Rust 1.99.1 because GPUI Kit 0.9.0's selected
macOS crates do not compile on the older toolchain. GPUI Kit remains exactly pinned in both normal
and development dependencies so the composition backend cannot drift independently.

Application migration is limited to public GPUI Kit API changes required by 0.9.0. It must not add
direct dependencies on `gpui-fast`, `gpui-pre`, or upstream GPUI. If the released stack still
crashes or materially regresses composition, the migration is held rather than masked by disabling
the cosmic surface.

### Consequences

- Good, because the native crash is addressed by released upstream code.
- Good, because AMP808 retains one GPUI facade and no framework patch.
- Good, because the exact dependency and toolchain pins remain reproducible.
- Bad, because the Rust toolchain change affects every workspace member and CI environment.
- Bad, because GPUI Kit 0.9.0 may require application API migration beyond the dependency probe.
- Neutral, because the local-library implementation remains otherwise unchanged.

## Non-goals

- No Rune adoption, renderer rewrite, GPUI fork, or GPUI Kit fork.
- No removal or redesign of the cosmic Metal underlay.
- No audio, MilkDrop, catalogue-schema, provider, or web-player changes.
- No UI restyling except changes strictly required by the upgraded public API.
- No claim of native acceptance until the application is actually launched and observed.

## Implementation Plan

- **Affected paths**:
  - `rust-toolchain.toml`: pin Rust 1.99.1.
  - `apps/desktop/Cargo.toml`: pin GPUI Kit 0.9.0 for production and test support.
  - `Cargo.lock`: resolve the released GPUI Kit and GPUI Fast stack.
  - `apps/desktop/src/`: make only required public API migrations.
  - CI or Moon configuration only if an existing command embeds the old toolchain version.
- **Patterns to follow**:
  - Keep `gpui-kit` as the sole GPUI dependency.
  - Use compile failures and focused desktop tests as the red phase, then make the smallest API
    migration required for green.
  - Preserve explicit accessible names, focus handles, keyboard actions, and composition fallback.
  - Keep unrelated dirty files and the pending Library slice intact.
- **Patterns to avoid**:
  - Do not patch Cargo registry sources or add `[patch]` overrides.
  - Do not bypass the crash by removing `gpui-fast` or the native underlay.
  - Do not broaden the migration into visual redesign or framework abstraction.
- **Migration steps**:
  1. Record the current native abort and older-toolchain compile failures as red evidence.
  2. Pin Rust 1.99.1 and GPUI Kit 0.9.0.
  3. Resolve the lockfile and compile the desktop target.
  4. Adapt only public API changes exposed by compilation and focused tests.
  5. Run desktop feature combinations, workspace tests, formatting, and focused strict Clippy.
  6. Launch the native application and observe first render, composition, controls, and Library UI.
  7. Continue separate playback, keyboard, and VoiceOver acceptance only after launch is restored.

### Verification

- [x] The lockfile resolves GPUI Kit 0.9.0 and GPUI Fast Apple 0.1.13 without a patch source.
- [x] The desktop manifest contains no direct GPUI Fast, GPUI Pre, or upstream GPUI dependency.
- [x] `cargo test --locked -p amp808-r3tr0` passes.
- [x] `cargo test --locked -p amp808-r3tr0 --features milkdrop-projectm` passes on macOS.
- [x] `cargo test --locked -p amp808-r3tr0 --features milkdrop-test-hooks` passes on macOS.
- [x] `cargo test --locked --workspace` passes.
- [x] Strict desktop test-target Clippy and `cargo fmt --all --check` pass.
- [x] A direct native launch reaches the first rendered frame without the prior colorspace abort.
- [x] The cosmic underlay, GPUI overlay, focus, keyboard shortcuts, and Library mode remain present.
- [x] Local track playback and MilkDrop remain separate native acceptance gates.

Verification evidence on 2026-11-06: the locked desktop suites passed with 73 base tests, 75
projectM tests, and 78 test-hook tests. The workspace suite passed 475 tests with four intentional
live/manual ignores. Strict desktop test-target Clippy passed with and without projectM, and
formatting plus diff checks passed. A direct debug launch remained alive and visibly rendered the
animated cosmic underlay beneath translucent GPUI controls. The Library button exposed a semantic
accessibility description, opened the searchable empty catalogue view, and the documented `L`
shortcut returned to Player with a visible focus ring. Playback, MilkDrop rendering, Tab traversal,
and VoiceOver were not requalified in this dependency-recovery slice.

## Pros and Cons of the Options

### Released dependency and toolchain upgrade

- Good, because the corrected Core Animation call is maintained upstream.
- Good, because it avoids local source ownership and patch drift.
- Bad, because it expands the migration to the workspace toolchain.

### Vendored compatibility patch

- Good, because it could retain the old application API and toolchain.
- Bad, because AMP808 would own framework code and contradict the no-fork constraint.
- Bad, because the patch would need repeated security and compatibility review.

### Disable native composition

- Good, because the opaque fallback could launch without GPUI Fast.
- Bad, because it removes an accepted product capability instead of repairing it.

### Wait upstream

- Good, because it performs no immediate migration.
- Bad, because native launch and Library acceptance remain blocked.

## More Information

- ADR-0022: [Add the macOS-first AMP808-R3TR0 desktop player with GPUI Kit](0022-add-amp808-r3tr0-macos-desktop-player.md)
- ADR-0025: [Add a native cosmic Metal underlay beneath GPUI](0025-add-a-native-cosmic-metal-underlay.md)
- GPUI Kit: <https://github.com/longbridge/gpui-kit>
- GPUI Fast: <https://github.com/longbridge/gpui-fast>

Revisit this decision before another GPUI Kit upgrade, if GPUI Fast changes its composition
contract, or if the native runtime still fails after the released-stack migration.
