---
status: accepted
date: 2026-11-06
decision-makers: alan
consulted: AMP808 desktop architecture, ADR-0022, ADR-0026, cliamp queue prior art
informed: AMP808 contributors
---

# Add persistent local playlists and a session playback queue

## Context and Problem Statement

ADR-0026 made AMP808-R3TR0's local catalogue durable and searchable, but every library action still
opens one track in isolation. A listener cannot collect tracks into a named list, choose what plays
next, inspect upcoming playback, or continue automatically after the current track ends. The TUI has
an in-memory `Playlist`, but ADR-0022 and ADR-0026 require the desktop product to retain its own state
and prohibit importing TUI application types.

The desktop now needs two related but different concepts. A **playlist** is a durable, user-named,
ordered collection of catalogue tracks. The **queue** is the disposable order of tracks scheduled
after the current playback session. Treating both as one persisted object would make temporary
listening choices durable by accident; treating both as transient would make named collections
disappear on relaunch.

How should AMP808-R3TR0 add useful local playlists and ordered upcoming playback without coupling
the catalogue to the audio engine, blocking GPUI or CPAL, or defining provider-neutral queue types
before an external playback source exists?

## Decision Drivers

- Named local playlists and their track order must survive application relaunch.
- Play Next, Add to Queue, remove, clear, and automatic end-of-track advancement must be predictable.
- Temporary queue contents and the current playback session must not reappear after relaunch.
- Queue operations must not perform SQLite, filesystem, decode, or allocation work on the CPAL callback.
- Playlist database work must remain outside GPUI rendering and use the existing catalogue connection boundary.
- The UI must expose playlist and queue state through keyboard-focusable, labelled controls.
- Missing or corrupt queued paths must produce an explicit error without discarding later queue entries.
- Local playlists may contain only local catalogue tracks in this slice.
- The desktop must not import the TUI `Playlist`, `Track`, `Provider`, or resolver modules.

## Considered Options

- Persist both playlists and the active queue in SQLite.
- Keep both playlists and the active queue only in memory.
- Persist named playlists in SQLite and keep the active queue in desktop memory.
- Extract and share the TUI playlist implementation.

## Decision Outcome

Chosen option: **persist named playlists in SQLite and keep the active queue in desktop memory**,
because durable user collections and disposable playback intent have different lifecycles. This
choice reuses the accepted catalogue repository without making playback depend on SQLite and keeps
provider-specific identifiers out of the local queue.

### Playlist persistence

1. Upgrade the catalogue schema from version 1 to version 2 inside one transaction.
2. Add `local_playlists` with a case-insensitively unique, trimmed, non-empty name.
3. Add `local_playlist_entries` with playlist ID, zero-based position, and local track ID. Its
   `(playlist_id, position)` primary key preserves order and allows the same track at multiple positions.
4. Reference `library_tracks(id)` with `ON DELETE CASCADE`. A successful library rescan that removes
   a missing track also removes its playlist entries; playlists never retain a hidden stale path.
5. Support create, rename, delete, list, replace ordered entries, append one track, and remove one
   position through `LibraryCatalogue`. All multi-row mutations are transactional.
6. Keep `rusqlite` types and SQL private to `library.rs`; expose AMP808-owned playlist values.

### Session queue

1. Add a desktop-owned `PlaybackQueue` that contains cloned local track snapshots for upcoming
   playback. It does not own the current `PlaybackSession`.
2. `play_next` inserts at the front, `add` appends, `remove` removes one position, `clear` removes
   every upcoming entry, and `take_next` removes and returns the first entry. Duplicate tracks are allowed.
3. Starting a library track directly clears the prior queue. Starting a playlist at one position
   plays that entry immediately and replaces the queue with the following playlist entries.
4. Natural `TransportState::Ended` consumes and opens the next queued track exactly once. Manually
   pausing, seeking, or reaching an error does not advance.
5. If a queued track fails to open, show the existing playback alert, remove only that failed queue
   entry, and preserve every later entry. Do not loop automatically through failures.
6. The queue starts empty on launch and is never written to SQLite in this decision.

### User interface

1. Extend Library mode with keyboard-focusable Play Now, Play Next, Add to Queue, and Add to Playlist
   actions whose accessible names include the track title.
2. Add an Up Next group showing ordered entries with remove and clear actions, an explicit empty
   state, and a visible count.
3. Add a Playlists group supporting create, select, rename, and delete. A selected playlist shows
   its ordered entries and can start playback from any entry.
4. Keep playlist and queue actions explicit. Do not overload a row click with a hidden append or
   replacement policy.

### Consequences

- Good, because named collections survive relaunch while temporary queue choices do not become stale sessions.
- Good, because the playback engine still accepts paths and remains unaware of catalogue persistence.
- Good, because queue order and failure behaviour are deterministic and independently testable.
- Bad, because library-track deletion also removes playlist membership; moved files are still new
  identities under ADR-0026.
- Bad, because playlist management adds schema, background-task, focus, and UI state complexity.
- Neutral, because external providers will require their own capability-aware queue decision later.

## Domain and Boundary Map

- **Local catalogue** owns playlist names, ordered local-track membership, schema migration, and transactions.
- **Session queue** owns ordered upcoming local track snapshots only; it owns no database connection or decoder.
- **Playback** owns the current path, decoder, transport, PCM, analysis, and visualisers.
- **Desktop UI** translates explicit user actions into catalogue, queue, and playback operations.
- **TUI and future providers** retain their existing models and do not participate in this slice.

Allowed dependencies are UI -> local catalogue, UI -> session queue, and UI -> playback. The queue
may contain `LibraryTrack` values but must not query the catalogue. Playback must not import the queue
or catalogue. The catalogue must not import GPUI, playback, queue, TUI, or provider modules.

Boundary verdict: **safe with noted risk**. The queue clones metadata and paths from catalogue rows,
so an entry may become stale after a rescan. Explicit open failure and preservation of the remaining
queue provide recovery without introducing synchronous catalogue checks into playback.

## Non-goals

- No queue or current-session persistence, resume-on-launch, history, or recently played list.
- No shuffle, repeat, crossfade, gapless playback, ReplayGain, or previous-track history.
- No drag-and-drop, multi-selection, bulk editing, smart playlists, folders, ratings, or favourites.
- No M3U/M3U8 import or export in the first desktop playlist slice.
- No Spotify, Apple Music, Music.app, HTTP, Navidrome, podcast, or yt-dlp entries.
- No generic provider item, universal track, queue backend, or shared TUI/desktop crate.
- No filesystem watcher, content hash, move tracking, or reconciliation of re-added paths.

## Implementation Plan

- **Affected paths**:
  - `apps/desktop/src/library.rs`: schema version 2 migration, playlist value types, validation,
    ordered queries, and transactional mutations.
  - `apps/desktop/src/queue.rs`: desktop-owned upcoming queue and its pure ordering operations.
  - `apps/desktop/src/main.rs`: register the queue module.
  - `apps/desktop/src/ui.rs`: playlist/queue state, background catalogue actions, explicit controls,
    automatic advancement, errors, focus, and accessibility.
  - `adr/README.md`: index ADR-0028 without altering unrelated ADR work.
- **Dependencies**: add no crate. Reuse `rusqlite`, GPUI Kit, and existing playback types.
- **Patterns to follow**:
  - Follow ADR-0026's short-lived SQLite connection and private repository pattern.
  - Execute catalogue mutations through GPUI background tasks, never during render.
  - Use typed local playlist IDs and explicit playlist/queue states rather than related booleans.
  - Build vertical TDD slices in order: playlist persistence, queue ordering, auto-advance decision,
    then rendered interaction and accessibility.
  - Add `// ADR-0028` once at the queue entry point and link this ADR from the schema migration.
- **Patterns to avoid**:
  - Do not store a `rusqlite::Connection` in `PlayerView` or `PlaybackQueue`.
  - Do not make `PlaybackSession` depend on playlist IDs, local track IDs, or queue state.
  - Do not perform database or filesystem work from `Render::render` or the CPAL callback.
  - Do not copy or import the TUI playlist implementation.
  - Do not add provider-neutral abstractions before a provider playback slice establishes commonality.
- **Configuration**: no environment variables, feature flags, credentials, or new files. Schema v2
  remains in the existing `<data_dir>/AMP808-R3TR0/library.sqlite3`.
- **Migration steps**:
  1. Add a failing test that opens a literal schema-v1 database, migrates it, and preserves its track.
  2. Add schema v2 and a failing persistence test for an ordered playlist with a duplicate entry.
  3. Add create/rename/delete and missing-track cascade tests one behaviour at a time.
  4. Add `PlaybackQueue` through failing play-next, append, remove, clear, and duplicate tests.
  5. Add a failing advancement-policy test proving only a new natural-ended transition advances once.
  6. Wire queue-driven playback and explicit failure preservation into `PlayerView`.
  7. Add failing GPUI tests for accessible queue and playlist controls before rendering them.
  8. Run desktop feature combinations, workspace tests, formatting, strict focused Clippy, and a
     separate native macOS playback/keyboard/VoiceOver acceptance pass.

### Verification

- [x] A literal schema-v1 fixture migrates to v2 without losing roots or tracks.
- [x] Playlist names reject empty or whitespace-only input and are unique case-insensitively.
- [x] A named playlist and ordered duplicate entries survive catalogue reopen.
- [x] Rename, delete, append, remove-position, and replace-order operations are transactional.
- [x] Removing a catalogue track cascades only its matching playlist entries and preserves order.
- [x] Play Next prepends, Add to Queue appends, duplicates remain distinct, remove is positional,
      and clear empties the queue.
- [x] A natural ended transition advances once; pause, seek, repeated renders, and errors do not.
- [x] A failed queued path exposes a playback alert and preserves later queue entries.
- [x] Starting a library track clears the queue; starting a playlist replaces it with following entries.
- [x] Queue and playlist controls expose names, roles, focus, empty states, and keyboard activation.
- [x] SQLite, filesystem, decode, and allocation work remain absent from the CPAL callback.
- [x] Base, projectM, test-hook, workspace, strict desktop Clippy, and formatting gates pass.
- [ ] Native macOS acceptance covers playlist creation/relaunch, ordering, Play Next, natural
      advancement, removal/clear, explicit error, recovery, keyboard focus, and VoiceOver semantics.

Automated evidence recorded on 2026-11-06:

- `cargo test --locked -p amp808-r3tr0`: 87 passed.
- `cargo test --locked -p amp808-r3tr0 --features milkdrop-projectm`: 89 passed.
- `cargo test --locked -p amp808-r3tr0 --features milkdrop-test-hooks`: 92 passed.
- `cargo test --locked --workspace --quiet`: 489 passed, 4 ignored.
- Strict base and `milkdrop-projectm` Clippy profiles passed with `-D warnings`.
- `cargo fmt --all --check` and `git diff --check` passed.
- Native macOS and VoiceOver acceptance remain unobserved and are deliberately not promoted by the
  automated results above.

## Simplicity Check

- **Outcome**: durable named local playlists plus predictable upcoming playback.
- **Options checked**: persist both, keep both transient, split durable playlists/transient queue,
  or share the TUI model.
- **Selected path**: reuse SQLite only for user collections and add one private in-memory queue module.
- **Entanglement avoided**: no database-aware playback, persisted session, TUI import, or provider trait.
- **Safeguards preserved**: transactions, explicit errors, accessibility, TDD, callback isolation, and rollback.
- **Omitted scope**: shuffle, repeat, history, import/export, providers, and cross-product sharing.
- **Remaining uncertainty**: large-playlist interaction and future provider queue composition require
  evidence from later product slices.

## Pros and Cons of the Options

### Persist playlists and queue

- Good, because all state survives relaunch.
- Bad, because temporary intent becomes stale durable state and needs resume/reconciliation policy.

### Keep playlists and queue in memory

- Good, because it requires no schema change.
- Bad, because named user collections disappear when the application exits.

### Persist playlists and keep queue transient

- Good, because storage follows the distinct lifetime of each concept.
- Good, because playback and queue operations remain independent of SQLite.
- Bad, because the UI coordinates two state owners.

### Share the TUI playlist

- Good, because shuffle, repeat, and queue algorithms already exist.
- Bad, because it couples independent products and imports provider/stream assumptions that the local
  desktop catalogue deliberately excludes.

## More Information

- ADR-0022: [Add the macOS-first AMP808-R3TR0 desktop player with GPUI Kit](0022-add-amp808-r3tr0-macos-desktop-player.md)
- ADR-0026: [Add a persistent local music catalogue to AMP808-R3TR0](0026-add-a-persistent-local-music-catalogue.md)
- Revisit this decision before external-provider queue entries, queue persistence, smart playlists,
  cross-device sync, or extraction into a shared crate.
