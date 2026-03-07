---
status: proposed
date: 2026-03-02
decision-makers: alan
---

# Add inline browser-backed runtime playlist loading

## Context and Problem Statement

`cliamp` currently builds its playlist at startup from CLI args (`src/main.rs`) and can append
provider tracks while running (`src/ui/mod.rs`). We added an in-app `:load <path>` command path
for local playlist loading, but typed file paths are still error-prone in a terminal UI. This
conflicts with our UX heuristic of error prevention: users should browse and confirm valid playlist
files rather than manually type paths when a safer interaction is available.

`ratatui-explorer` now ships `v0.3.0` with `ratatui 0.30` compatibility, which matches this
project's TUI stack. That makes an in-app file browser viable without a ratatui version mismatch.

We need a runtime playlist-loading UX that prevents mistakes while preserving current architecture
constraints:
- Keep playback and decode paths unchanged (ADR-0002, ADR-0003, ADR-0004)
- Keep UI interactions in the UI layer (`src/ui/*`)
- Keep behavior consistent across standard and TR-808 views
- Keep the current local playlist load path reusable so explorer selection and typed commands do not
  diverge
- Restrict scope to loading local `.m3u` and `.m3u8` files, not arbitrary media browsing

## Decision

1. Adopt `ratatui-explorer 0.3.x` as the backing engine for loading local playlist files.
2. Expose the browser as an inline, focusable pane in both standard and TR-808 views rather than a modal overlay.
3. Keep `:load <path>` as a fallback for power users and automation-friendly workflows.
4. Configure the explorer to:
   - hide hidden files by default
   - show directories
   - allow selection of `.m3u` and `.m3u8` files only
   - start in the last successfully used playlist directory when available
5. On explorer confirm:
   - if the selected item is a directory, continue browsing
   - if the selected item is a supported playlist file, reuse the existing local playlist load path
   - if the selection is invalid, keep the current playlist unchanged and show a clear status/error
6. On successful playlist load, replace the current playlist atomically, reset playlist cursor/scroll,
   and start playback from track 1.
7. Keep all browser-specific state and crate interactions isolated in the UI layer.

## Non-goals

- No shell execution or arbitrary command language.
- No provider command integration in v1.
- No background file watching or auto-reload for playlist files.
- No new config persistence format for command history in v1.
- No general-purpose file manager for arbitrary media files.
- No replacement of the existing `:load` fallback.

## Consequences

* Good, because browse-and-select prevents path-typing mistakes and aligns better with error-prevention UX.
* Good, because the explorer can reuse the current atomic playlist replacement path rather than inventing a second load implementation.
* Good, because keeping `:load` preserves fast paths for advanced users and tests.
* Good, because the browser remains inside the main screen and avoids a focus-trap modal.
* Bad, because input routing still needs explicit handling for a new browser focus state.
* Bad, because adding `ratatui-explorer` introduces dependency surface and `unstable-widget-ref` coupling through ratatui.
* Neutral, because this is still limited to playlist-file loading rather than broader library navigation.

## Implementation Plan

* **Affected paths**:
  - `Cargo.toml` (`ratatui-explorer = "0.3"` dependency)
  - `src/ui/mod.rs` (browser state + selection handoff to existing playlist loader)
  - `src/ui/keys.rs` (browser focus routing and key handling)
  - `src/ui/view.rs` (render inline browser pane in standard view)
  - `src/ui/view_808.rs` (render inline browser pane in 808 view)
  - `src/ui/command.rs` (retain `:load` fallback without duplicating file-load logic)
  - `src/resolve/mod.rs` (existing local playlist-file parser remains the source of truth)
  - `src/ui/explorer.rs` (new adapter/wrapper isolating `ratatui-explorer` specifics)
* **Patterns to follow**:
  - Keep browser state isolated from playback state.
  - Keep playlist file parsing/loading logic testable and separated from key handling.
  - Keep command-mode and browser-focus handoff centralized in `App` methods.
  - Reuse existing error/status message mechanisms where possible.
  - Filter explorer-visible files to directories plus `.m3u` / `.m3u8` only.
* **Patterns to avoid**:
  - No playback-thread logic changes for explorer interaction.
  - No network-backed browsing or provider browsing through the explorer.
  - No duplicate playlist replacement code path separate from the current local playlist loader.
  - No exposing unsupported file types in the explorer list.

### Verification

* [ ] Pressing `L` focuses the inline playlist browser in both standard and TR-808 views.
* [ ] Explorer shows directories and `.m3u` / `.m3u8` files only, with hidden files hidden by default.
* [ ] `Esc` returns focus from the browser pane to the player without changing playlist or playback.
* [ ] Confirming a valid playlist file replaces playlist and starts track 1.
* [ ] Explorer selection reuses the same local playlist load path used by `:load`.
* [ ] `:load ./playlist.m3u` remains functional as a fallback.
* [ ] Invalid selection or malformed file leaves current playlist intact and shows clear error text.
* [ ] Existing `/` search, theme picker, provider navigation, and key bindings still behave correctly.
* [ ] `cargo test` passes with new explorer-mode and playlist-load handoff tests.

## Alternatives Considered

* **Keep CLI-only playlist loading**: simplest, but forces restart and breaks interactive flow.
* **Keep command-only `:load`**: lower implementation cost, but still encourages user path-entry mistakes.
* **Build an in-house explorer widget**: full control, but reinvents solved navigation behavior and increases maintenance burden.
* **Auto-load from watched playlist file**: useful for automation, but out of scope for immediate command-driven workflow.

## More Information

Related existing decisions:
- ADR-0004: Tokio async with dedicated cpal audio thread
- ADR-0007: Provider trait for external music services

Dependency verification references (checked 2026-03-06):
- <https://crates.io/crates/ratatui-explorer>
- <https://crates.io/crates/ratatui-explorer/0.3.0>
- <https://docs.rs/ratatui-explorer/latest/ratatui_explorer/struct.FileExplorer.html>
- <https://github.com/tatounee/ratatui-explorer/releases/tag/v0.3.0>
