---
status: accepted
date: 2026-11-07
decision-makers: alan
consulted: Apple App Sandbox documentation, objc2-foundation 0.3.2 API, ADR-0022, ADR-0033
informed: AMP808 contributors
---

# Persist the local MilkDrop folder with a macOS security-scoped bookmark

## Context and Problem Statement

ADR-0033 added a searchable local MilkDrop preset gallery, but deliberately kept the selected
folder session-only. Remembering only its path would work in the current unsandboxed development
build while failing to preserve the user's permission once AMP808-R3TR0 is distributed with the
macOS App Sandbox. Apple requires an app-scoped security-scoped bookmark for access that survives
process exit, explicit activation of the resolved URL before use, balanced release afterward, and
bookmark renewal when the resolved bookmark is stale.

AMP808 must restore and rescan one user-selected preset folder after relaunch without copying the
presets, coupling preset state to the local music database, leaking security-scope resources, or
making built-in presets unavailable when restoration fails.

## Decision Drivers

- Preserve the permission represented by the user's folder selection, not merely its path.
- Keep bookmark bytes opaque and local to the preset-gallery context.
- Balance every successful security-scope activation through ownership and `Drop`.
- Keep the active permission alive while later Preview operations read files from the folder.
- Recover visibly from missing, denied, corrupt, or stale bookmarks while retaining First Light.
- Add no persistence responsibility to the music-library SQLite catalogue.
- Keep filesystem and Foundation work off GPUI rendering and the CPAL callback.
- Use typed Foundation bindings and the dependency versions already resolved in `Cargo.lock`.

## Considered Options

- Persist the canonical folder path only.
- Store opaque bookmark bytes in the music-library SQLite database.
- Store one opaque bookmark file and resolve it through a small macOS platform adapter.
- Depend on the `apple_scoped_bookmarks` convenience crate.
- Send untyped Objective-C messages through the existing `objc` crate.

## Decision Outcome

Chosen option: **store one opaque bookmark file and resolve it through a typed macOS adapter**.

The bookmark lives at `<dirs::data_dir()>/AMP808-R3TR0/preset-folder.bookmark`. It is a bounded,
opaque Foundation payload written atomically with user-only permissions. It is not a portable
configuration file and no code may interpret its bytes.

On folder selection, AMP808 creates a read-only app-scoped security bookmark, activates access,
discovers the presets, and persists the bookmark only after those operations succeed. It then
retains a `PresetFolderAccess` guard for as long as local catalogue entries may be previewed.
Replacing the folder drops the previous guard after the new folder is ready.

On launch, AMP808 loads and resolves the bookmark on the background executor, activates access,
renews stale bookmark bytes atomically, and rescans the resolved directory. Missing bookmark data
means the normal built-in-only state. A corrupt bookmark, denied access, missing folder, failed
renewal, or failed scan produces an accessible alert and keeps built-in presets usable; it must not
silently fall back to a remembered string path.

The macOS adapter uses `objc2 = "0.6.4"` and `objc2-foundation = "0.3.2"` with only the Foundation
features needed for `NSURL`, `NSData`, `NSError`, `NSArray`, and `NSString`. Both versions already
exist in the workspace lockfile through GPUI dependencies. The adapter contains the smallest
possible unsafe surface for Foundation APIs whose stale-result pointer is generated as unsafe,
with an explicit safety comment at every call.

Distribution entitlements remain a packaging responsibility. AMP808 records the required
app-scoped bookmark and user-selected read-only keys, but an ad-hoc development signature is not
evidence that a production sandboxed build has the correct stable signing identity.

### Consequences

- Good, because the selected preset folder and its permission can survive relaunch.
- Good, because a guard makes balanced scope release the default even on early return or error.
- Good, because bookmark persistence remains independent of music-library schema and transactions.
- Good, because moved or renamed folders can resolve through Foundation rather than a stale path.
- Bad, because the desktop target gains direct typed Foundation dependencies and a small unsafe
  adapter boundary.
- Bad, because bookmarks are bound to the creating signing identity and cannot be qualified fully
  with an ad-hoc development signature.
- Bad, because a missing removable volume remains unavailable until it returns or the user selects
  another folder.
- Neutral, because AMP808 still supports exactly one local preset folder.

## Domain and Boundary Map

- **Preset catalogue** owns discovered preset metadata and deterministic search/filter results.
- **Preset-folder grant** owns opaque bookmark persistence, resolution, and active access lifetime.
- **Desktop UI** owns selection, status, retry through Add Folder, and the active grant guard.
- **Packaging** owns sandbox activation, entitlements, stable signing identity, and release proof.
- **Music library** owns local audio roots and tracks only; it must not store preset bookmarks.

Allowed direction is UI -> preset-folder grant -> Foundation/filesystem and UI -> preset catalogue.
The grant adapter may return only an accessible `Path` plus its RAII lifetime; Foundation types and
bookmark bytes do not enter the catalogue, playback, projectM, CPAL, or music-library APIs.

Boundary verdict: **safe with noted risk**. Release signing and App Sandbox qualification cannot be
closed until the application has a production packaging pipeline and identity.

## Non-goals

- No second preset folder, recent-folder list, filesystem watcher, removable-volume monitor, cloud
  sync, iCloud container, or cross-device bookmark transfer.
- No copying, vendoring, downloading, editing, or redistributing user presets.
- No bookmark column or table in `library.sqlite3`.
- No generic settings database, provider trait, shared TUI module, or cross-platform bookmark API.
- No automatic deletion of an unreadable bookmark; Add Folder replaces it after a successful scan.
- No claim that ad-hoc signing proves production App Sandbox access.

## Implementation Plan

- **Affected paths**:
  - `apps/desktop/src/preset_folder.rs`: bounded atomic bookmark store, structured errors, and the
    `PresetFolderAccess` lifetime boundary.
  - `apps/desktop/src/preset_folder/macos.rs`: typed bookmark create/resolve/start/stop adapter and
    stale renewal data.
  - `apps/desktop/src/main.rs`: register the preset-folder module.
  - `apps/desktop/src/ui.rs`: background restore on construction, successful-selection persistence,
    retained guard, and accessible restoration errors.
  - `apps/desktop/Cargo.toml`: direct macOS-only `objc2` and minimal-feature `objc2-foundation`
    dependencies at the versions already locked.
  - `apps/desktop/macos/AMP808-R3TR0.entitlements`: document app-scoped bookmarks and user-selected
    read-only access for future packaging and signed-bundle acceptance.
  - `adr/README.md`: index ADR-0034.
- **Dependencies**:
  - `objc2 = "0.6.4"` on macOS.
  - `objc2-foundation = { version = "0.3.2", default-features = false, features = ["std",
    "NSArray", "NSData", "NSError", "NSString", "NSURL"] }` on macOS.
  - Add no serialization, settings, database, async-runtime, or convenience-bookmark crate.
- **Patterns to follow**:
  - Follow `LibraryCatalogue::open_default` for the application data root, without importing or
    opening the library database.
  - Use a typed error with actionable variants and source chaining for filesystem failures.
  - Bound bookmark reads before allocating, write a sibling temporary file, sync it, and rename.
  - Start access before scanning and hold the guard until local entries are replaced or the app exits.
  - Run restore, bookmark creation, renewal, and scanning on GPUI's background executor.
  - Add `// ADR-0034` at the platform adapter entry point only.
- **Patterns to avoid**:
  - Do not persist a path as a substitute for bookmark data.
  - Do not expose `NSURL`, `NSData`, `NSError`, or raw bookmark bytes outside `preset_folder`.
  - Do not call `startAccessingSecurityScopedResource` without balancing a successful call.
  - Do not hold access only for the scan; Preview needs the same permission later.
  - Do not read or write bookmark state from `Render::render` or the CPAL callback.
  - Do not overwrite the last working bookmark until a replacement folder is accessible and scans.
- **Configuration**:
  - Bookmark path: `<dirs::data_dir()>/AMP808-R3TR0/preset-folder.bookmark`.
  - Maximum persisted bookmark size: 1 MiB.
  - Bookmark access is read-only.
  - No environment variable, user preference, network permission, or database migration.
- **Migration steps**:
  1. Add a failing store test proving opaque bytes survive reopening and an oversized payload is
     rejected without replacing the previous value.
  2. Implement the bounded atomic store and structured errors.
  3. Add a failing macOS adapter test for corrupt bookmark rejection, then implement create/resolve
     and the RAII access guard.
  4. Add a failing UI test proving a restoration failure remains visible while First Light remains.
  5. Wire background restore and successful Add Folder persistence into `PlayerView`.
  6. Add entitlement documentation and package notes without enabling a release sandbox implicitly.
  7. Run base and projectM desktop tests, strict focused Clippy, formatting, and diff checks.
  8. Perform a separate native relaunch check with one unchanged signed test bundle; label stable
     production signing and sandbox enforcement unverified.

### Verification

- [x] Opaque bookmark bytes survive closing and reopening the bookmark store.
- [x] Missing bookmark data returns the built-in-only state without an error.
- [x] A bookmark larger than 1 MiB is rejected without replacing the last working bookmark.
- [x] Bookmark writes are atomic and user-only on macOS.
- [x] Corrupt bookmark bytes produce a structured error without a panic.
- [x] Every successful security-scope start is balanced by `Drop`.
- [ ] Stale resolution renews the persisted bookmark before the restored folder is reported ready.
- [ ] Add Folder retains its previous working catalogue and grant when create, scan, or save fails.
- [x] Relaunch restoration rescans off the GPUI render path and exposes an accessible status.
- [x] Restore failure leaves First Light searchable and exposes an accessible alert.
- [x] No Foundation type, bookmark byte payload, or music-library database type crosses the
      `preset_folder` boundary.
- [x] Base and projectM desktop tests pass.
- [x] Strict desktop test-target Clippy, `cargo fmt --all -- --check`, and `git diff --check` pass.
- [ ] Native relaunch with one unchanged signed test bundle restores the folder and previews a
      local preset; production stable-signing and App Sandbox proof remain separately open.

## Pros and Cons of the Options

### Persist the canonical path only

- Good, because it needs no platform API or new direct dependency.
- Bad, because a path does not preserve the Powerbox permission after sandboxed relaunch.
- Bad, because it would create behavior that passes development but fails the release security model.

### Store bookmark bytes in the music-library database

- Good, because AMP808 already has transactional SQLite persistence.
- Bad, because preset permissions are unrelated to audio-library roots and would couple their
  schema, failure, and lifecycle boundaries.

### Store one opaque bookmark file behind a typed adapter

- Good, because the persistence shape matches the one-folder product rule.
- Good, because raw bytes can be replaced atomically without a schema or serializer.
- Good, because Foundation and unsafe calls remain inside one platform module.
- Bad, because release signing and entitlements still require separate packaging evidence.

### Depend on `apple_scoped_bookmarks`

- Good, because it provides a compact safe API and an RAII guard.
- Bad, because version 0.1.0 adds a young wrapper when the typed Foundation dependency and lockfile
  are already present.

### Use untyped Objective-C messages

- Good, because the desktop target already depends on `objc` 0.2.
- Bad, because selectors, return types, error pointers, and ownership would be checked less strongly
  and would expand the unsafe surface.

## Simplicity Check

- **Outcome**: automatically restore one accessible local preset folder after relaunch.
- **Options checked**: path, SQLite blob, raw bookmark file, convenience crate, untyped messages.
- **Selected path**: one bounded raw file plus a typed, RAII macOS adapter.
- **Entanglement avoided**: no library schema, settings framework, catalogue-owned permission, or
  Foundation type outside the adapter.
- **Safeguards preserved**: atomic replacement, size bound, visible recovery, background work,
  balanced scope release, TDD, and release-evidence separation.
- **Omitted scope**: multiple folders, watching, provider reuse, distribution, and cloud sync.
- **Remaining uncertainty**: stable-signing and sandbox behavior await a production package identity.

## More Information

### 2026-11-07 implementation evidence

- Store coverage proved missing-state handling, reopen survival, the 1 MiB bound, non-replacement
  after rejection, atomic rename, and mode `0600` on macOS.
- Adapter coverage proved corrupt bookmark rejection. Source review confirmed that only a
  successful `startAccessingSecurityScopedResource` sets the guard state balanced by `Drop`.
- GPUI coverage proved corrupt restoration retains First Light and exposes an alert, Add Folder
  persists a bookmark, and a fresh `PlayerView` restores and rescans the selected folder.
- Automated gates passed with 159 default-feature tests and 164 all-feature/projectM tests, strict
  test-target Clippy, formatting, diff checks, and entitlement plist linting.
- Native same-bundle evidence used one ad-hoc signed app with the declared bookmark entitlements.
  After selecting the bundled preset folder and terminating the process, the relaunched app
  exposed `Restored 1 local MilkDrop presets from .../apps/desktop/assets/presets`. The generated
  868-byte mode-`0600` acceptance bookmark was then deleted; the pre-existing library database was
  left untouched.
- Still open: direct stale-bookmark renewal evidence, failure-injection coverage for replacement
  create/scan/save, native preview after restored access, and production stable-signing plus App
  Sandbox enforcement.

- ADR-0022: [Add the macOS-first desktop player](0022-add-amp808-r3tr0-macos-desktop-player.md)
- ADR-0033: [Add a local-first MilkDrop preset gallery](0033-add-a-local-first-milkdrop-preset-gallery.md)
- Apple: <https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox>
- objc2-foundation `NSURL`: <https://docs.rs/objc2-foundation/0.3.2/objc2_foundation/struct.NSURL.html>

Revisit this decision before adding multiple folders, a settings repository, a production sandbox
profile, or a non-macOS desktop release.
