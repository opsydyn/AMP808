---
status: accepted
date: 2026-11-04
decision-makers: alan
---

# Adopt a Moon-orchestrated Cargo monorepo layout

## Context and Problem Statement

AMP808 is already a Cargo workspace containing the native terminal application at the repository
root, the Ratzilla browser application in `apps/web`, and a web-oriented support crate in
`crates/amp808-core`. A separate native desktop player is now planned as a decoupled product, with
AMP808 available to it as a skin rather than making the desktop application another frontend of the
terminal binary.

Keeping the native package at the repository root makes the workspace ownership unclear: the root
simultaneously acts as the workspace coordinator and as one product application. We need an
explicit applications-and-crates layout before adding another product, while preserving the
existing `amp808` command, releases, native architecture, and browser application.

Moon v2 does not replace Cargo. It orchestrates Cargo commands and provides a consistent task
surface for the workspace. This decision therefore separates repository orchestration from Rust
compilation and dependency resolution.

Related decisions remain in force:

- ADR-0002 through ADR-0004 govern native audio, decoding, and concurrency.
- ADR-0012 governs the macOS Music.app playback backend.
- ADR-0017 through ADR-0019 govern the web application and browser audio boundaries.
- ADR-0020 governs the reopened native Apple Music integration.

## Decision Drivers

- Give every product application an explicit location under `apps/`.
- Preserve the published package name, executable name, command-line interface, and release assets.
- Keep the relocation independently reversible and free of runtime behaviour changes.
- Avoid extracting speculative shared abstractions before the TUI and desktop products prove them.
- Introduce Moon without making it a second build system or immediately changing CI authority.
- Apply BeautifulRust guidance without enabling a lint profile that the current workspace cannot
  yet pass under `-D warnings`.
- Preserve unrelated work already present in the working tree.

## Considered Options

- Move the native package to `apps/tui` and add workspace-level Moon orchestration in a dedicated
  relocation slice.
- Move the native package and extract shared crates at the same time.
- Keep the native package at the root and add only Moon configuration.
- Create the GPUI desktop application during the monorepo migration.

## Decision Outcome

Chosen option: "Move the native package to `apps/tui` and add workspace-level Moon orchestration in
a dedicated relocation slice", because it establishes clear product ownership while preserving
behaviour and keeping code extraction and desktop architecture as separately reviewable decisions.

The resulting repository shape is:

```text
/
├── .moon/
├── apps/
│   ├── tui/          # Cargo package and binary remain named amp808
│   └── web/          # existing Ratzilla application
├── crates/
│   └── amp808-core/  # unchanged during this migration
├── Cargo.toml        # virtual Cargo workspace
├── Cargo.lock
└── moon.yml          # workspace-level Cargo task orchestration
```

`apps/desktop` is the reserved destination for the future GPUI product, but it is not created by
this migration. Its framework, product boundary, skin contract, audio visualisation design, and
shared-crate needs require a separate ADR.

Moon is initially additive. Existing Cargo commands and CI/release workflows remain authoritative.
Moon tasks must mirror those commands closely enough to provide a convenient local orchestration
surface without changing their semantics.

BeautifulRust's `rust-best-practices` guidance applies to Rust changes made during the relocation.
The complete BeautifulRust workspace lint profile is explicitly deferred: the current workspace
has measured lint debt, and adding the profile while CI promotes warnings to errors would make the
structural migration depend on unrelated remediation.

### Consequences

- Good, because all current and future product applications have an explicit `apps/` boundary.
- Good, because the `amp808` package, executable, command-line interface, and release naming remain
  stable even though their source path changes.
- Good, because Moon provides one discoverable workspace task surface while Cargo remains the
  compiler, resolver, lockfile owner, and source of truth.
- Good, because later shared crates must be justified by proven reuse between products.
- Bad, because paths in manifests, documentation, workflows, packaging scripts, and include rules
  may need mechanical updates even though runtime behaviour is unchanged.
- Bad, because developers temporarily have two supported task entry points: Cargo and Moon.
- Neutral, because `apps/web` and `crates/amp808-core` retain their current names and responsibilities
  even though `amp808-core` is not yet a product-neutral core.

## Non-goals

- No GPUI dependency or desktop application scaffold.
- No shared-crate extraction, module redesign, or dependency-direction cleanup.
- No rename of the `amp808` package, executable, release, or command.
- No rename or semantic expansion of `crates/amp808-core`.
- No dependency or Rust toolchain upgrade.
- No change to audio, DSP, visualiser, provider, Apple Music, TUI, or web behaviour.
- No switch from Cargo commands to Moon in CI or release automation.
- No immediate adoption of the full BeautifulRust lint profile.

## Implementation Plan

- **Affected paths**:
  - `Cargo.toml`: replace the root package-plus-workspace manifest with a virtual workspace manifest;
    make `apps/tui` a member and preserve default-member behaviour.
  - `src/`: move intact to `apps/tui/src/`.
  - `apps/tui/Cargo.toml`: preserve the root package metadata, dependencies, features, binary name,
    and package identity.
  - `.moon/workspace.yml`: register the repository root as the single initial Moon project.
  - `.moon/toolchains.yml`: enable Rust using the repository's existing pinned toolchain policy.
  - `moon.yml`: define workspace-level build, check, format, lint, test, and release-gate tasks that
    invoke Cargo commands.
  - Structural tests or scripts: assert the virtual-root and package/binary identities before and
    after relocation.
  - Repository paths in workflows, scripts, documentation, and packaging configuration: update
    only where the source relocation makes an existing path incorrect.
- **Dependencies**: add no Rust dependencies and upgrade none. Moon is a developer orchestration
  prerequisite, not a Cargo dependency.
- **Patterns to follow**:
  - Keep one root `Cargo.lock` and one Cargo target directory.
  - Put workspace-level Moon tasks next to the root lockfile to avoid parallel per-crate Cargo
    processes contending for the build lock.
  - Preserve the current root package manifest content in `apps/tui/Cargo.toml` unless a path must
    change because the manifest moved.
  - Preserve `cargo build`, `cargo run -- <args>`, and release asset naming through workspace
    defaults and explicit package selection where necessary.
  - Use red-green-refactor: first add a structural assertion that fails against the old layout,
    then make the smallest relocation that satisfies it.
- **Patterns to avoid**:
  - Do not move reusable-looking modules into new crates during this change.
  - Do not make `apps/web` depend on the TUI package.
  - Do not make Moon responsible for compilation logic already owned by Cargo.
  - Do not add per-crate Moon projects during the first slice.
  - Do not enable BeautifulRust's complete lint set under the existing strict CI gate.
- **Configuration**:
  - Register one root project in `.moon/workspace.yml`.
  - Enable Rust in `.moon/toolchains.yml`.
  - Define a single root Moon project in `moon.yml` with file groups covering `apps/**`,
    `crates/**`, root manifests, tests, workflows, and relevant scripts.
  - Keep `rust-toolchain.toml` as the Rust version source; do not introduce a second divergent pin.
- **Migration steps**:
  1. Capture the current Cargo metadata, package identity, binary identity, and baseline gates.
  2. Add one failing structural test/assertion for the target workspace layout.
  3. Move the root package manifest and `src/` to `apps/tui` without editing Rust behaviour.
  4. Convert the root manifest to a virtual workspace and restore the same default build/run
     behaviour.
  5. Run the structural test and focused Cargo checks until green.
  6. Add root Moon configuration and prove its tasks mirror their Cargo equivalents.
  7. Run the full existing native, core, and web verification gates.
  8. Update only relocation-sensitive documentation and automation paths discovered by validation.

### Verification

- [x] A structural test confirms the root manifest is virtual and `apps/tui/Cargo.toml` defines
      package `amp808` with binary `amp808`.
- [x] `cargo metadata --no-deps --format-version 1` reports `apps/tui`, `apps/web`, and
      `crates/amp808-core` as workspace packages with one root lockfile.
- [x] `cargo build --locked` builds the native `amp808` executable at the existing target path.
- [x] `cargo run --locked` prints the existing `amp808` startup usage summary and exits with the
      existing no-media status.
- [x] `cargo fmt --all --check` passes.
- [ ] `cargo nextest run --profile ci --locked` passes.
- [x] `cargo test --locked` passes for the native and core default members.
- [x] `cargo test --doc --quiet --locked` passes for workspace library targets.
- [x] `cargo clippy --all-targets --locked -- -D warnings` passes.
- [x] `cargo check --locked -p amp808_web --target wasm32-unknown-unknown` passes.
- [x] Existing web tests pass without behavioural changes.
- [ ] `bash scripts/check-release-gate.sh` passes without changing its Cargo authority.
- [x] Moon's layout, build, check, check-web, format, lint, and test tasks invoke and pass the
      equivalent Cargo/script commands.
- [ ] Moon's release-gate task invokes and passes `scripts/check-release-gate.sh`.
- [x] Release packaging still finds the executable as `target/release/amp808` or the equivalent
      existing cross-target paths used by `.github/workflows/release.yml`.
- [x] No GPUI dependency, `apps/desktop` implementation, shared-crate extraction, or complete
      BeautifulRust lint profile is introduced.

## Pros and Cons of the Options

### Move the native package first, then extract only proven shared crates

- Good, because the filesystem boundary becomes clear without changing runtime ownership.
- Good, because failures can be attributed to relocation rather than architecture refactoring.
- Good, because the change is straightforward to reverse.
- Bad, because existing coupling inside the TUI package remains visible for a later change.

### Move and extract shared crates together

- Good, because it could approach the eventual multi-product architecture in one change.
- Bad, because `src/player`, `playback_backend`, providers, async messages, and UI types currently
  have reverse dependencies that require behavioural design rather than mechanical movement.
- Bad, because relocation and abstraction regressions would be difficult to distinguish.

### Keep the native package at the root and add only Moon

- Good, because it produces the smallest immediate filesystem diff.
- Bad, because the root remains both workspace coordinator and product application.
- Bad, because a future desktop product would enter an asymmetrical layout.

### Create the GPUI product during migration

- Good, because the target application path would be immediately tangible.
- Bad, because it combines repository layout, framework adoption, product boundaries, and native UI
  architecture into one irreversible change.
- Bad, because the correct shared contracts have not yet been proven by two implementations.

## More Information

- Moon Rust handbook: <https://moonrepo.dev/docs/guides/rust/handbook>
- Hummingbird GPUI player prior art: <https://github.com/hummingbird-player/hummingbird>
- BeautifulRust: <https://github.com/morganlinton/BeautifulRust>

Hummingbird is implementation prior art for a Rust/GPUI audio application, not the workspace-layout
template for this decision: its current manifest describes a single application package.

Revisit this decision if Moon task parity requires replacing rather than invoking Cargo, if release
automation cannot preserve the `amp808` identity after relocation, or if the future desktop ADR
proves that a different product boundary is necessary.

Implementation verification on 2026-11-04:

- The relocation test was observed failing against the root package layout and passing after the
  move to `apps/tui`.
- Source and fixture files were byte-compared with their pre-migration Git versions; no content
  changes were found.
- Native/core tests passed with 314 tests passing and 4 ignored in `amp808`, plus 16 tests passing
  in `amp808-core`.
- All 72 `amp808_web` host-side tests passed, and the wasm target check passed.
- Formatting, doctests, strict baseline Clippy, Moon task wrappers, debug build, release build, and
  package-content inspection passed.
- The existing release-gate script could not run locally because `cargo-nextest` is not installed
  on this machine. CI installs that tool explicitly. No tool was installed and the gate remains
  unchecked; the equivalent available Cargo tests, doctests, and Clippy commands passed.
