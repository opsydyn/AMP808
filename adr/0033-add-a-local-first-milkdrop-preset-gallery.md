---
status: accepted
date: 2026-11-07
decision-makers: alan
consulted: ADR-0024, ADR-0025, presets-cream-of-the-crop README and licence statement
informed: AMP808 contributors
---

# Add a local-first MilkDrop preset gallery with reversible live preview

## Context and Problem Statement

AMP808-R3TR0 can load one local `.milk` file and safely restores the last working preset when
projectM rejects a replacement. It does not expose a searchable collection, type filters, or a
distinction between temporarily previewing a preset and choosing it as the active preset.

The projectM `presets-cream-of-the-crop` repository contains 9,795 presets grouped into useful
theme directories. Its README encourages inclusion in projectM applications, but its licence
statement says that most individual presets did not receive a specific licence and are only
assumed to be in the public domain. AMP808 must not convert that assumption into a redistribution
claim.

How should the desktop player provide a fast, accessible preset gallery and real audio-reactive
preview while retaining local-first operation, the existing projectM surface, and a defensible
distribution boundary?

## Decision

Add an AMP808-owned preset catalogue and gallery with two separately governed sources:

1. **AMP808 presets** are original presets covered by this repository's MIT licence. The existing
   `amp808-first-light.milk` is the initial built-in catalogue entry.
2. **Local presets** are `.milk` files from one folder selected by the user for the current
   session. AMP808 scans that folder recursively without copying or redistributing its contents.

The catalogue records a stable ID, display title, category, origin, and load source for each
entry. A local preset's first directory beneath the selected root is its category; a preset at the
root uses `Uncategorised`. The catalogue does not infer authorship or licensing from filenames.

Search and filtering remain dependency-free and in memory. Search is case-insensitive and uses a
deterministic ordered-subsequence score over title and category. Every whitespace-separated term
must match at least one searchable field. Results sort by match score, then category, title, and
stable ID. Category filters use the actual catalogue categories.

The gallery uses a virtualised GPUI list and one existing projectM view:

- selecting a row changes only the gallery selection;
- **Preview** installs the selected preset temporarily;
- **Use** promotes the current preview to the applied preset;
- **Restore** reinstalls the last applied preset;
- a failed preview retains the previous working projectM preset and reports the existing alert;
- loading a single `.milk` file through the existing picker treats that explicit choice as applied.

No renderer is created per gallery item. Static or animated thumbnails are deferred because the
upstream collection does not ship previews and generating thousands of images is a separate asset,
storage, performance, and licensing decision.

The community collection may be bundled later only through a reviewed resource manifest that
records provenance and redistribution status. Until that gate passes, AMP808 must not fetch,
vendor, package, or imply ownership of those files. Users may select a locally acquired copy.

## Consequences

- Good, because users can browse the complete local community collection without AMP808
  redistributing it.
- Good, because explicit Preview, Use, and Restore actions preserve user control and make temporary
  changes reversible.
- Good, because one virtualised list and one projectM surface keep work bounded for roughly ten
  thousand entries.
- Good, because catalogue search and categorisation are deterministic and testable without GPUI or
  projectM.
- Bad, because the selected local preset folder is session-only in this slice and must be selected
  again after relaunch.
- Bad, because the first gallery has textual cards rather than generated visual thumbnails.
- Bad, because shipping the upstream collection remains blocked on provenance despite the
  technical packaging path being straightforward.
- Neutral, because user-provided files retain their existing ownership and distribution terms;
  AMP808 does not certify them.

## Boundary Review

- **Preset catalogue** owns discovery metadata, deterministic category/filter/search results, and
  on-demand reading of a selected file.
- **MilkDrop host** owns projectM installation, rejection detection, and last-known-good rollback.
- **Desktop UI** owns session selection plus the Preview/Use/Restore interaction state.
- **Packaging** owns any future bundled collection manifest and must admit only reviewed material.

The catalogue must not import GPUI, projectM, playback, SQLite, networking, or the local music
catalogue. The CPAL callback and PCM handoff do not change. Verdict: **safe with a mandatory
redistribution gate**.

## Non-goals

- No vendoring, downloading, Git submodule, or application bundling of
  `presets-cream-of-the-crop` in this slice.
- No claim that the upstream collection or an individual local file is MIT, public domain, or
  otherwise cleared for redistribution.
- No generated screenshots, video loops, animated card previews, transition engine, preset
  ratings, favourites, or persisted recent history.
- No second projectM surface, per-row renderer, GPUI fork, projectM fork, or renderer-thread change.
- No persistence of a selected preset directory and no macOS security-scoped bookmark yet.
- No changes to playback, analysis, the CPAL callback, or the local music catalogue database.

## Implementation Plan

1. Add `apps/desktop/src/preset_catalog.rs` with catalogue entries, recursive local discovery,
   deterministic categories, fuzzy search, and structured I/O errors. Add no dependency.
2. Update `apps/desktop/src/milkdrop.rs` to expose the repository-owned First Light preset as an
   owned `MilkdropPreset` suitable for applied/preview state while retaining the existing startup
   source and rollback behavior.
3. Update `apps/desktop/src/ui.rs` with session-only preset-folder selection, search input,
   category controls, a virtualised result list, and explicit Preview/Use/Restore actions.
4. Keep `apps/desktop/assets/presets/README.md` as the authoritative inventory and licensing note
   for bundled presets. Any future admitted third-party pack must update it and this ADR's
   provenance evidence before packaging.
5. Build in vertical TDD slices: built-in entry, local discovery/category, search/filter order,
   reversible preview state, then headless gallery interaction.
6. Run base and projectM desktop tests, strict desktop Clippy, formatting, and a separate native
   macOS preview/restore acceptance pass.

### Patterns to follow

- Reuse `MilkdropPreset::read` for size, UTF-8, extension, and interior-NUL validation.
- Reuse `MilkdropViewHost::install_preset` for projectM rollback and visible errors.
- Reuse the local-library in-memory fuzzy search and virtualised GPUI presentation approach without
  coupling the two catalogues.
- Scan local folders on the background executor and load only the selected preset source.
- Provide accessible group, search, category, selection, status, and action labels.

### Patterns to avoid

- Do not compile thousands of `include_str!` entries into the executable.
- Do not parse an author or licence from an upstream filename.
- Do not eagerly read every `.milk` file while scanning metadata.
- Do not rebuild or filter ten thousand UI nodes on every visualiser frame.
- Do not change the applied preset merely because keyboard focus or pointer hover moved.

### Configuration and dependencies

- Add no crate, environment variable, database table, network permission, or feature flag.
- The local folder and preview state are intentionally session-only.
- A future application-resource manifest is a follow-up gated by reviewed redistribution evidence.

### Verification

- [x] The built-in catalogue exposes First Light as an AMP808/MIT-owned entry.
- [x] Recursive discovery finds case-insensitive `.milk` files, ignores other files and symlinked
      directories, and derives deterministic categories from the first relative directory.
- [x] Multi-term fuzzy search and category filters return stable deterministic results.
- [x] Scanning a large local directory and reading a selected preset occur off the GPUI render path.
- [x] The MilkDrop view remains a single surface while the result list is virtualised.
- [x] Selection alone does not install a preset; Preview installs temporarily, Use promotes it, and
      Restore returns to the prior applied preset.
- [x] A rejected candidate retains the previous working preset and exposes the existing error.
- [x] The gallery exposes accessible search, category, selection, Preview, Use, Restore, and status
      semantics.
- [x] No file from `presets-cream-of-the-crop` is vendored, fetched, or packaged.
- [x] `cargo test --locked -p amp808-r3tr0` and the projectM-feature suite pass.
- [x] Strict desktop test-target Clippy and `cargo fmt --all --check` pass.
- [ ] Native macOS acceptance observes local-folder import, search, category filtering, preview,
      use, restore, failed-preset recovery, keyboard focus, and VoiceOver exposure separately.

## Options Considered

### Bundle the complete upstream collection immediately

- Good, because every preset is available offline with no setup.
- Good, because upstream already supplies useful thematic folders.
- Bad, because the collection's own licence statement does not provide an unambiguous grant for
  most individual presets.
- Bad, because application size, QA surface, and generated preview cost expand before usage is
  measured.

### Ship only a curated upstream subset

- Good, because application size and compatibility testing remain bounded.
- Good, because AMP808 can shape a strong default experience.
- Bad, because selecting fewer files does not resolve their individual redistribution status.
- Bad, because curation and removal policy become ongoing product work.

### Browse a user-selected local collection, with AMP808-owned starters

- Good, because it delivers the complete catalogue, search, category, and preview architecture now.
- Good, because AMP808 redistributes only material it owns or has explicitly reviewed.
- Bad, because users must obtain and select third-party collections themselves.
- Bad, because relaunch persistence requires a later macOS bookmark decision.

This option is chosen because it satisfies the product interaction goal without treating uncertain
copyright status as technical metadata.

## More Information

### 2026-11-07 automated verification

- The base desktop suite passed 153 tests and the `milkdrop-projectm` suite passed 155 tests.
- `cargo clippy --locked -p amp808-r3tr0 --tests --all-features -- -D warnings` and
  `cargo fmt --all -- --check` passed.
- The broader all-target strict audit still reports the same 19 pre-existing production dead-code
  findings recorded by ADR-0025. It reports no finding in `preset_catalog.rs`, `milkdrop.rs`, or the
  new gallery/preview code.
- After the Mac was unlocked, native acceptance observed folder import, fuzzy search, category
  filtering, projectM playback, Preview, Use, and Restore with the 60-second local fixture. The
  first visual pass found virtual rows with zero usable width even though they remained present in
  the accessibility tree. A failing bounds regression test reproduced that defect; giving the
  virtual list full width made the test pass, and the rebuilt macOS app visibly rendered the First
  Light card. Failed-preset recovery, keyboard traversal, and spoken VoiceOver output remain
  deliberately unchecked for this gallery slice.

### 2026-11-07 keyboard and recovery follow-up

- A GPUI regression test verifies that the virtual gallery exposes one selected-row tab stop,
  Up/Down changes selection without previewing, focus follows the selected row, and Tab leaves the
  virtual rows for the Preview action rather than walking every card.
- A gallery integration test loads an invalid UTF-8 local preset through the asynchronous preview
  path, verifies the visible error alert, and verifies that First Light remains the applied preset.
  The existing projectM-host test separately verifies renderer rejection restores the last-known-
  good preset.
- The base desktop suite passed 155 tests and the `milkdrop-projectm` suite passed 157 tests.
  Strict desktop test-target Clippy, `cargo fmt --all -- --check`, and `git diff --check` passed.
- Native macOS acceptance imported the bundled preset folder, moved selection from First Light to
  the local preset with Down, then used Tab and Return to activate Preview. The player exposed
  `Previewing: amp808-first-light` while PCM and MilkDrop remained live.
- Spoken VoiceOver output and a native rejected-preset injection remain unchecked. The broader
  native-acceptance checkbox therefore remains open.

- ADR-0024: [Host projectM in a native macOS OpenGL subview](0024-host-projectm-in-a-native-macos-opengl-subview.md)
- ADR-0025: [Add a native cosmic Metal underlay beneath GPUI](0025-add-a-native-cosmic-metal-underlay.md)
- projectM collection: <https://github.com/projectM-visualizer/presets-cream-of-the-crop>
- Collection README: <https://raw.githubusercontent.com/projectM-visualizer/presets-cream-of-the-crop/master/README.md>
- Collection licence statement: <https://raw.githubusercontent.com/projectM-visualizer/presets-cream-of-the-crop/master/LICENSE.md>

Revisit third-party application bundling only when AMP808 can record explicit redistribution terms
for every admitted file or a collection-level grant that clearly covers the application package.
