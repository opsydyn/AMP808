---
status: accepted
date: 2026-11-07
decision-makers: alan
consulted: ADR-0022, ADR-0026, ADR-0028, ADR-0029, GPUI Kit 0.9 table API
informed: AMP808 contributors
---

# Use GPUI Kit DataTable for the local library

## Context and Problem Statement

The local library currently renders every matching track as a hand-built vertical list. It has no
column headers, deterministic user-controlled sort, field filter, or typo-tolerant search, and it
cannot virtualise a large result set. GPUI Kit 0.9 already provides an accessible, keyboard-aware,
virtual `DataTable` with sortable and resizable columns.

How should AMP808-R3TR0 make the local catalogue sortable, filterable, and fuzzy searchable while
preserving local-first behaviour, artwork, playback actions, and the existing catalogue boundary?

## Decision

Replace only the main local-library result list with GPUI Kit's public `DataTable` and an
AMP808-owned read-model delegate.

- The catalogue continues to load the full local track snapshot on a background executor. SQLite
  remains the persisted source of truth and receives no schema or fuzzy-search extension.
- The table delegate owns cloned loaded rows plus a derived visible index. It filters and sorts
  in memory, then exposes virtual rows to GPUI Kit.
- Search uses a small deterministic, dependency-free subsequence scorer. Whitespace-separated
  terms must each match the selected field scope; contiguous and word-start matches rank higher.
- Filter scope is explicit: all metadata, title, artist, or album. Empty queries show every row.
- Title, artist, album, and duration headers cycle GPUI Kit's ascending/descending sort state.
  Ties use title and canonical path for deterministic ordering.
- Artwork, Play Now, Play Next, Add to Queue, and Add to Playlist remain public table cells and
  actions. Double-clicking a row plays it.
- Every cell supplies a screen-reader label, and the table retains GPUI Kit's row selection,
  keyboard navigation, resizable columns, and virtual scrolling.

No fuzzy-search crate is added. Revisit a dedicated index only if measured large-library latency
or memory shows the session read-model is insufficient.

## Consequences

- Good, because the table follows the UI framework's native interaction and accessibility model.
- Good, because fuzzy matching and ordering are pure, deterministic, and unit-testable.
- Good, because database, playback, queue, and playlist ownership do not change.
- Bad, because the desktop keeps a cloned in-memory track snapshot and derived visible index.
- Bad, because the lightweight scorer will not provide language-specific stemming or phonetics.
- Neutral, because the existing catalogue already loads every track for an empty query; this does
  not introduce a new whole-library read.

## Boundary Review

- **Local catalogue** owns persisted track metadata and background loading.
- **Library read-model** owns transient query, filter, sort, and visible row order.
- **GPUI Kit table** owns presentation, virtual scrolling, focus, and header interaction.
- **Desktop UI** translates table actions into existing playback, queue, and playlist operations.

Allowed dependencies are UI table -> catalogue value type and UI table -> existing UI actions.
The catalogue must not import GPUI, fuzzy matching, playback, queue, or table state. Verdict:
**safe with noted risk**; synchronous in-memory filtering must be measured with a large library
before claiming large-catalogue performance.

## Non-goals

- No SQLite FTS table, schema migration, persisted sort/filter preference, or background search index.
- No external provider, network catalogue, Spotify, Apple Music, or Music.app search.
- No playlist-entry reordering, drag-and-drop, multi-select, bulk actions, or queue redesign.
- No new fuzzy-matcher dependency or GPUI Kit fork.
- No redesign of the playlist-management or up-next panels in this slice.

## Implementation Plan

1. Add one failing pure test for ordered subsequence matching, then implement the scorer.
2. Add failing tests for field scopes, deterministic ordering, and sort direction one at a time.
3. Add the GPUI Kit `TableDelegate`, accessible cell labels, action cells, and row activation.
4. Replace database-per-keystroke search with table read-model refresh over the loaded snapshot.
5. Preserve the bounded artwork cache and update table artwork snapshots after probes resolve.
6. Verify base and projectM tests, strict desktop Clippy, formatting, and native macOS interaction.

### Verification

- [x] A non-contiguous query such as `csmpltn` matches `Cosmopolitan`.
- [x] Every query term must match within the selected all/title/artist/album scope.
- [x] Title, artist, album, and duration sort ascending and descending with deterministic ties.
- [x] The GPUI Kit table exposes labelled cells and keyboard-selectable rows.
- [x] Artwork and all existing row actions remain available after filtering and sorting.
- [x] Search and sort perform no SQLite, filesystem, decoder, or CPAL callback work.
- [x] Base, projectM, strict Clippy, and formatting gates pass.
- [ ] Native macOS acceptance covers search, scope filters, header sort, keyboard, and playback.

The 2026-11-07 native pass confirmed fuzzy search, scope filters, header sorting, the dark-skinned
table, and macOS accessibility labels for rows, cells, headers, and actions. Keyboard activation
and playback are covered by automated tests but remain unchecked here until both are observed in
one native acceptance pass.

## Options Considered

### Keep the hand-built list and add sort buttons

This is a smaller diff but duplicates table semantics, keeps eager row rendering, and provides a
weaker keyboard and accessibility contract than the framework primitive.

### Add SQLite FTS with fuzzy extensions

This scales beyond the current snapshot but changes schema and deployment complexity before a
large-library measurement demonstrates the need.

### Add a third-party fuzzy-matching crate

This may improve ranking sophistication, but it adds supply-chain and maintenance cost for a
bounded local matcher whose required behaviour is straightforward and testable.
