---
status: accepted
date: 2026-11-06
decision-makers: alan
consulted: AMP808 desktop architecture, cliamp local-library prior art, rusqlite 0.41.1 metadata
informed: AMP808 contributors
---

# Add a persistent local music catalogue to AMP808-R3TR0

## Context and Problem Statement

AMP808-R3TR0 can open and play one local file at a time, but it cannot remember music folders,
browse previously discovered tracks, or search local metadata. The TUI already has an in-memory
`Playlist` and recursive argument resolver, but ADR-0022 deliberately keeps the desktop product
decoupled from TUI application state. Importing those types would couple two products without
providing persistence or a catalogue boundary.

The next product phase is a local-first music library. Spotify and Apple Music may later add their
own catalogue and playback or handoff capabilities, but local files must become useful before a
generic provider model is designed. Local files are directly decodable and expose PCM; Apple Music
handoff and a future Spotify adapter have different identifiers and capabilities.

How should AMP808-R3TR0 persist, scan, search, and play a local music catalogue without blocking
GPUI or the audio callback and without prematurely defining a universal provider abstraction?

## Decision Drivers

- A selected music folder and its indexed tracks must survive application restarts.
- Searching title, artist, and album must not require rescanning the filesystem.
- A failed or partial rescan must not destroy the last known-good catalogue.
- Filesystem traversal, metadata probing, and SQLite work must stay off GPUI and CPAL threads.
- The local catalogue must pass only resolved paths into the existing playback boundary.
- Corrupt and unsupported files must be counted honestly without aborting valid-track discovery.
- Folder symlinks must not create traversal cycles or escape the selected root unexpectedly.
- The first slice must be usable with keyboard and VoiceOver.
- Desktop code must not import `apps/tui` or move TUI application state into a shared crate.
- Provider-neutral abstractions must wait until a second concrete source establishes real commonality.

## Considered Options

- Persist an indexed catalogue in SQLite through `rusqlite`.
- Serialize the complete catalogue to JSON or TOML.
- Rescan selected folders into memory on every launch.
- Extract the TUI playlist and resolver into a shared crate first.

## Decision Outcome

Chosen option: **persist an indexed catalogue in SQLite through `rusqlite`**, because transactional
updates protect the last known-good catalogue, indexed queries provide durable search, and one
local file avoids a server or async runtime. AMP808 will use `rusqlite = 0.41.1` with default
features disabled and the `bundled` feature enabled so the macOS application has deterministic
SQLite linkage.

The first implementation remains inside `apps/desktop/src/library.rs`. A small cloneable catalogue
handle owns only the database path; each background operation opens its own short-lived connection.
The module exposes AMP808-owned value types instead of leaking `rusqlite` types. SQLite statements
remain behind private repository functions. The module should split into repository and scanner
submodules only when either responsibility grows beyond this bounded slice.

The catalogue owns:

- selected local roots;
- stable internal row identifiers for local tracks;
- canonical file paths;
- title, artist, album, duration, sample rate, channels, file size, and modified time;
- the outcome of the latest successful scan.

Canonical path is the first-slice local identity. Moving a file is represented as removing the old
entry and discovering a new one; content hashing, inode tracking, and cross-device move detection
are outside this decision.

Scanning runs on GPUI's background executor. It walks regular directories without following
directory symlinks, filters supported audio extensions, reuses unchanged metadata based on size and
modified time, and probes changed files through a metadata-only function extracted from
`media.rs`. A successful root scan commits additions, updates, and removals in one transaction.
Failure to enumerate the root rolls the transaction back and preserves the previous catalogue.
Individual corrupt or unsupported audio files increment a skipped count and do not remove other
valid results.

The desktop UI will expose a Library mode with an Add Folder action, loading/empty/error/ready
states, a search field, an accessible track list, and keyboard activation that starts the existing
`PlaybackSession` from the selected local path. Playback remains unaware of SQLite and scanning.

Spotify and Apple Music will not implement this repository. When a second source is built, its ADR
may introduce a capability-aware UI read model that composes source-specific entries without
converting provider items into local paths or promising PCM access they do not have.

### Consequences

- Good, because the local library survives restarts and supports indexed search.
- Good, because transactional root scans preserve the last good catalogue on traversal failure.
- Good, because playback, scanning, persistence, and presentation retain explicit ownership.
- Good, because later provider work begins from observed capability differences instead of a
  speculative `Provider` trait.
- Bad, because the desktop binary gains bundled SQLite and additional build time and size.
- Bad, because canonical paths do not preserve identity across moves or renames.
- Bad, because periodic manual rescan is required until a later filesystem-watching decision.
- Neutral, because the TUI retains its existing playlist and resolver in this slice.

## Domain and Boundary Map

- **Local catalogue** owns roots, indexed metadata, search, scan transactions, and local track IDs.
- **Local scanner** reads the filesystem and produces catalogue candidates; it does not play audio.
- **Playback** owns decode, transport, PCM, analysis, and visualisers; it accepts a selected path.
- **Desktop UI** owns query text, selection, focus, and user-visible scan state.
- **Future provider catalogues** own remote IDs, credentials, API DTOs, and provider capabilities.

Allowed dependency direction is UI -> local catalogue -> filesystem/SQLite, and UI -> playback.
Playback must not import the local catalogue. The local catalogue must not import GPUI, playback,
TUI, Spotify, Apple Music, or provider credentials.

Boundary verdict: **safe with noted risk**. The known risk is duplicated local-track metadata between
the catalogue and playback. The mitigation is a narrow metadata probe shared inside the desktop
application; the catalogue passes a path, not its stored metadata, when playback starts.

## Non-goals

- No Spotify, Apple Music, Music.app, Navidrome, Plex, HTTP, podcast, or yt-dlp catalogue entries.
- No generic provider, universal track, playback-backend, or capability trait.
- No import from `apps/tui` and no shared-crate extraction.
- No playlists, queues, favorites, ratings, history, recently played, or resume state.
- No album, artist, or genre drill-down; the first view is a searchable flat track list.
- No artwork extraction, artwork cache, lyrics, waveform cache, BPM persistence, or audio fingerprint.
- No filesystem watcher, automatic background rescan, removable-volume monitor, or cloud sync.
- No content hashing, inode identity, duplicate detection, or move tracking.
- No provider credentials or tokens in the catalogue database.
- No database encryption or multi-user profile system.

## Implementation Plan

- **Affected paths**:
  - `apps/desktop/src/library.rs`: catalogue facade, local value types, schema version 1, private
    SQL, transactional scans, supported-extension traversal, symlink policy, metadata candidates,
    search, and scan summaries.
  - `apps/desktop/src/media.rs`: extract metadata-only probing reusable by scanner and decoder.
  - `apps/desktop/src/ui.rs`: Library mode, Add Folder prompt, background scan, search, accessible
    rows, selection, activation, and explicit states.
  - `apps/desktop/src/main.rs`: register the library module.
  - `apps/desktop/Cargo.toml` and `Cargo.lock`: add `rusqlite` and `dirs`.
  - `moon.yml`: include SQLite-backed desktop tests in existing desktop gates; add no new task if
    current source groups and commands already cover them.
  - `adr/README.md`: index ADR-0026.
- **Dependencies**:
  - `rusqlite = { version = "0.41.1", default-features = false, features = ["bundled"] }`.
  - `dirs = "6"`, already present in the workspace lockfile, for the macOS application data root.
- **Patterns to follow**:
  - Follow `media.rs` error/value conventions and keep third-party errors private.
  - Follow the existing `choose_file` GPUI prompt/update flow, but execute scan and database work on
    the background executor.
  - Build one behavior-first TDD slice at a time: persistence, scan transaction, search, then UI.
  - Use `Path`/`PathBuf`, typed local IDs, explicit `LibraryStatus`, and structured `LibraryError`.
  - Use parameterized SQL exclusively and set `PRAGMA foreign_keys = ON` on every connection.
  - Store schema version with `PRAGMA user_version` and migrate inside a transaction.
  - Add `// ADR-0026` at the catalogue entry point only.
- **Patterns to avoid**:
  - Do not expose `rusqlite::Connection`, `Row`, or `Error` outside `library.rs`.
  - Do not keep a database connection in `PlayerView` or behind a UI-thread mutex.
  - Do not scan, probe metadata, or query SQLite inside `Render::render`.
  - Do not follow directory symlinks or silently index files outside the chosen root.
  - Do not delete prior rows until a root traversal and metadata pass reaches its commit point.
  - Do not reuse the TUI `Track`, `Playlist`, `Provider`, or resolver modules.
  - Do not add a provider abstraction before a provider slice has its own accepted ADR.
- **Configuration**:
  - Database path: `<dirs::data_dir()>/AMP808-R3TR0/library.sqlite3` on macOS.
  - No environment variables, feature flags, network calls, or credential storage.
  - Tests use temporary database and media directories; they never touch the user catalogue.
- **Migration steps**:
  1. Add a failing persistence test proving a track survives catalogue reopen.
  2. Add schema version 1 and the minimal catalogue repository.
  3. Add a failing scan test for one valid fixture and implement metadata-only discovery.
  4. Add transactional rescan tests for update, removal, corrupt-file skip, and root failure.
  5. Add a failing case-insensitive metadata search test and indexed search query.
  6. Add a failing GPUI test for Library empty state and Add Folder prompt.
  7. Add Library state rendering and background folder scan.
  8. Add a failing GPUI test proving an accessible result row activates local playback selection.
  9. Add keyboard navigation/search behavior and native macOS acceptance.
  10. Run desktop feature combinations, workspace tests, formatting, and strict focused Clippy.

### Verification

- [x] Opening a new catalogue creates schema version 1 without touching user data in tests.
- [x] A persisted local track survives closing and reopening the catalogue.
- [x] Scanning a fixture root discovers supported audio recursively and ignores unrelated files.
- [x] Directory symlinks are not followed and cannot produce traversal cycles.
- [x] Re-scanning updates changed metadata and removes files absent from a successfully scanned root.
- [x] A root enumeration failure rolls back and preserves the previous root catalogue.
- [x] Corrupt or unsupported audio files are counted as skipped while valid tracks commit.
- [x] Case-insensitive search matches title, artist, and album and returns deterministic ordering.
- [x] Database and filesystem work execute outside GPUI render and CPAL callback paths.
- [x] The Library UI exposes accessible loading, empty, failure, and ready states.
- [x] Add Folder accepts a directory, persists it, and refreshes the visible track list.
- [x] Keyboard and VoiceOver users can focus, identify, and activate a local track row.
- [x] Activating a result starts the existing local playback path without catalogue coupling.
- [x] Relaunching AMP808-R3TR0 reloads the indexed catalogue without rescanning first.
- [x] Existing spectrum, waveform, MilkDrop, cosmic background, transport, errors, and shortcuts
      continue to work with library playback.
- [x] `cargo test --locked -p amp808-r3tr0` passes.
- [x] `cargo test --locked -p amp808-r3tr0 --features milkdrop-projectm` passes on macOS.
- [x] Strict desktop test-target Clippy and `cargo fmt --all --check` pass; broader pre-existing
      all-target dead-code findings remain reported separately until their own cleanup slice.

Verification evidence on 2026-11-06: the locked base, projectM, test-hook, and workspace suites
passed after the Library implementation, together with strict focused Clippy and formatting. A
native disposable-folder run discovered one valid WAV, counted one corrupt WAV as skipped, and
showed the valid row immediately. Uppercase `COSMIC` search matched the lowercase filename-derived
title. Tab navigation reached the search field and track row; Enter activated the focused row,
switched to Player, produced real spectrum energy with `PCM / LIVE`, and reached `PCM / ENDED` at
00:08. Relaunching and pressing `L` showed the indexed track without rescanning. The pre-acceptance
catalogue was backed up and restored after the run, verified at zero roots and zero tracks, and the
temporary fixture directory was removed. VoiceOver and the combined waveform/MilkDrop/error
regression remained open rather than being inferred from that run.

Follow-up verification on 2026-11-06 closed those gaps. A GPUI headless regression tabs from the
player root to the first local track, asserts the accessible `Play Signal Bloom` action, presses
Enter, and observes the production playback error after the indexed fixture is deliberately
removed. In a native macOS run, VoiceOver was enabled while AMP808 exposed the library row as a
button labelled `Play voiceover-milkdrop — Unknown artist / Unknown album`, including its 44.1 kHz,
two-channel, one-minute technical description; the focus ring tracked native controls and the row
action started playback. The same projectM-enabled run showed live Spectrum and Waveform output,
live native MilkDrop output, the animated cosmic background, responsive transport, an explicit
corrupt-WAV playback alert, and recovery by reopening the valid library track. Automated shortcut
tests remained green. VoiceOver and AMP808 were stopped afterward, the pre-run catalogue was
restored and verified at zero roots and zero tracks, and the exact temporary fixture directory was
removed.

## Simplicity Check

- **Outcome**: durable local folder browsing, search, and playback.
- **Options checked**: launch-time scan, serialized catalogue, SQLite catalogue, shared TUI extraction.
- **Selected path**: desktop-owned SQLite repository plus background scanner.
- **Entanglement avoided**: no UI-owned connection, playback dependency, TUI import, or provider trait.
- **Safeguards preserved**: transactional scans, explicit errors, accessibility, TDD, and rollback.
- **Omitted scope**: grouping, playlists, artwork, watching, providers, credentials, and shared crates.
- **Remaining uncertainty**: native scan responsiveness and large-library behavior require measurement
  after the first complete vertical slice.

## Pros and Cons of the Options

### SQLite catalogue

- Good, because transactions preserve consistent root snapshots.
- Good, because indexed search and schema evolution fit later local-library features.
- Bad, because bundled SQLite increases dependency and binary weight.

### JSON or TOML catalogue

- Good, because the persisted file is human-readable.
- Bad, because every update rewrites the complete catalogue and query logic stays in memory.
- Bad, because crash-safe partial updates and schema migration become application responsibilities.

### Rescan on every launch

- Good, because it adds no database.
- Bad, because launch time and metadata I/O grow with the collection.
- Bad, because there is no durable search or last-known-good state.

### Extract TUI playlist/resolver first

- Good, because some extension filtering and track concepts could be reused.
- Bad, because those types model an active playback list, not a persistent catalogue.
- Bad, because it contradicts the proven decoupled desktop boundary before reuse exists.

## More Information

- ADR-0021: [Adopt a Moon-orchestrated Cargo monorepo layout](0021-adopt-moon-cargo-monorepo-layout.md)
- ADR-0022: [Add the macOS-first AMP808-R3TR0 desktop player with GPUI Kit](0022-add-amp808-r3tr0-macos-desktop-player.md)
- ADR-0026 implementation should be revisited before extracting a shared catalogue crate, adding a
  second source, or introducing automatic filesystem watching.
- rusqlite 0.41.1 metadata checked through crates.io on 2026-11-06; MSRV 1.89 is below the
  repository's Rust 1.95 toolchain.
