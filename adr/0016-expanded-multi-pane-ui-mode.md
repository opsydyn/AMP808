---
status: accepted
date: 2026-06-13
updated: 2026-06-14
decision-makers: alan
---

# Add expanded multi-pane UI mode

## Context and Problem Statement

amp808 currently renders into a single centred pane (~80×28 cells) in both Compact and
808-chrome modes. All content — playlist, visualizer, EQ, cover art, status — is stacked
vertically with minimal horizontal use. On wider terminals (≥120 cols) this wastes space
and forces the user to context-switch between the playlist, metadata, and visualizer by
scrolling mentally through a single column.

Inspiration: SparkPlayer (ratatui forum, June 2025) demonstrates that a TUI player gains
significant UX clarity by splitting into dedicated panes — playlist on the left, now-playing
metadata top-right, album art centre, and a full-width heatmap spectrogram at the bottom.

Constraints:
- Compact and 808-chrome modes must remain pixel-identical (zero regression)
- Audio pipeline untouched (ADR-0002, ADR-0004)
- Must fit within existing `App` state model without a separate state machine
- W already = Winamp's classic expand/collapse toggle

## Decision

### 1. Replace `mode_808: bool` with a `ViewMode` enum

```rust
// src/ui/view_mode.rs
pub enum ViewMode {
    Compact,         // existing standard layout — unchanged
    Expanded,        // multi-pane layout with heatmap spectrogram
    Drum808,         // existing 808 chrome layout — unchanged
    Drum808Expanded, // TR-808 aesthetic applied to the full multi-pane layout
}
```

`App.view_mode: ViewMode` replaces `App.mode_808: bool`. Toggle rules:

| Key | From | To |
| --- | --- | --- |
| `W` | `Compact` | `Expanded` |
| `W` | `Expanded` | `Compact` |
| `W` | `Drum808` | `Drum808Expanded` |
| `W` | `Drum808Expanded` | `Drum808` |
| `8` | `Compact` | `Drum808` |
| `8` | `Drum808` | `Compact` |
| `8` | `Expanded` | `Drum808Expanded` |
| `8` | `Drum808Expanded` | `Expanded` |

`W` toggles size (compact ↔ expanded) within the same aesthetic family.  
`8` toggles aesthetic (standard ↔ 808) within the same size.

`App::render()` dispatches on `view_mode`; `refresh_palette()` applies `Palette::tr808()` for
both `Drum808` and `Drum808Expanded`. No other code changes outside the dispatch and rename.

### 2. Expanded layout — three permanent panes + full-width spectrogram

Minimum terminal: **120 × 36**. A size guard renders a plain warning if the terminal is smaller.

Album art is embedded inside the Now Playing column (not a separate column). Art is decoded
into a dedicated `cover_art_proto_expanded` at 32×24 cells for higher resolution than the
compact-mode proto.

```
┌────────────────────────────────────┬──────────────┬───────────────────┐
│  NOW PLAYING                       │ FILE BROWSER │  PLAYLIST         │
│  (50% — art left, metadata right)  │ (20%)        │  (30%)            │
│                                    │              │                   │
│ ┌──────┐  TITLE  …                │ ~/Music/     │  1. Track A       │
│ │      │  ARTIST …                │  dir/        │ ▶ 2. Track B      │
│ │ art  │  ALBUM  …                │ ▶ dir/       │  3. Track C       │
│ │      │  INFO   44kHz             │              │  …                │
│ └──────┘  ─────────────────────── │              │                   │
│           01:21 / 04:37  -03:16   │              │                   │
│           ▶ Playing                │              │                   │
│           [One] [Shuf] [126 BPM]  │              │                   │
│           VOL  ████░░  -9.0dB     │              │                   │
│           EQ  -12 +6 … 6k 12k 16k│              │                   │
│           NEXT  [dim track name]  │              │                   │
├────────────────────────────────────┴──────────────┴───────────────────┤
│  Heatmap Spectrogram — full width, 10 rows                            │
├───────────────────────────────────────────────────────────────────────┤
│  W compact  Tab focus  Space play  n/p next/prev  ←→ seek  +/- vol   │
└───────────────────────────────────────────────────────────────────────┘
```

ratatui layout (outer vertical):
```
[Min(0),      // top three-column row
 Length(12),  // spectrogram (10 rows + 2 border)
 Length(1)]   // help bar
```

Top row horizontal:
```
[Percentage(50), Percentage(20), Percentage(30)]
```

Now Playing inner horizontal split (when art is available and width > 24):
```
art_w = min(inner_height, inner_width/2, proto.area().width)
meta_w = inner_width - art_w - 1  // 1-col gap
```

#### Now Playing metadata enhancements

- **Time remaining**: time row shows `01:21 / 04:37  -03:16` (elapsed / total · remaining).
  Omitted for streaming sources where duration is unknown.
- **EQ frequency colours**: each of the 10 EQ bands is styled with `palette.spectrum_style(i/9.0)`,
  warm (yellow) at bass, cool (red) at treble. Selected band uses accent override.
- **Next track preview**: `NEXT   [dim track name]` row at the bottom of metadata when
  `playlist.peek_next()` is `Some`. Disappears on last track.
- **Playlist track numbers**: each playlist row prefixes a right-aligned dim number
  (`" 1. "`, `"▶ 2. "`). Width adapts to `format!("{}", tracks.len()).len() + 2` digits.

### 3. File browser — always-on middle column

In Expanded mode the `PlaylistExplorer` is always initialised and rendered in the middle
column (20%), between Now Playing and Playlist. On first entry to Expanded mode,
`toggle_playlist_browser()` is called if
`explorer.is_none()`. The `L` key in Expanded mode focuses the Browser column rather than
toggling an overlay. `Esc` from Browser focus returns to Playlist focus. In Compact mode
`L` keeps its existing overlay-toggle behaviour.

### 4. Focus model in expanded mode

`Focus` enum gets no new variants; the three panes that accept key input reuse existing variants:

| Pane focused | `Focus` variant | ↑ / ↓ | ← / → | Enter |
|---|---|---|---|---|
| File Browser | `Browser` | navigate files | parent / enter dir | load playlist |
| Playlist | `Playlist` | scroll tracks | seek ±5s | play selected |
| EQ (within Now Playing pane) | `EQ` | band ±1 dB | cursor L/R | — |

`Tab` in Expanded mode cycles: `Browser → Playlist → EQ → Browser`.

The focused pane's border is rendered with `palette.accent` (same convention as compact EQ cursor).
Now Playing metadata panel has no focus state — always visible, always up-to-date, no cursor.

### 5. Heatmap spectrogram — data model and render

**Data model** — `Visualizer.spectrogram_history: VecDeque<[f64; 10]>`. Cap = `area.width`. Each
`on_tick()` pushes a snapshot of the 10 smoothed band levels from `vis.prev`. No extra FFT work.

**Render** — one terminal cell per (time, frequency) position with `bg` colour from a 7-stop
gradient:

| Range | Color |
|-------|-------|
| 0.00–0.10 | `Rgb(  8,   0,  20)` near-black |
| 0.10–0.25 | `Rgb( 60,   0,  80)` deep purple |
| 0.25–0.45 | `Rgb(140,   0,   0)` dark red |
| 0.45–0.65 | `Rgb(200,  60,   0)` orange |
| 0.65–0.80 | `Rgb(230, 180,   0)` amber |
| 0.80–0.95 | `Rgb(255, 240,  60)` bright yellow |
| 0.95–1.00 | `Rgb(255, 255, 255)` white (clip) |

### 6. 808 Expanded — TR-808 themed multi-pane mode

Minimum terminal: **120 × 44**. Layout (outer vertical):

```rust
[Length(6),   // TR-808 big-text header (full width)
 Min(0),      // top three-column row (NowPlaying/Browser/Playlist)
 Length(7),   // 808 knob bar (5 inner rows → 4 canvas + 1 label)
 Length(12),  // 808 visualizer with TEMPO dial + LED spectrum
 Length(1)]   // help bar (amber chip badges)
```

#### Knob bar

11 Canvas+Braille knobs: VOL + 10 EQ bands (70Hz → 16kHz). Each knob uses:

- `x_bounds([-5.5, 5.5])` / `y_bounds([-4.0, 4.0])` / `radius = 3.5`
- Braille dot grid for a ~11-col × 4-row canvas: 22×16 dots → x:y = 1.375 (visually round)
- Ivory (`#EEEADC`) background arc ring; warm→hot active arc overlay; amber needle

#### TEMPO dial + LED spectrum

The 808 Visualizer row is split horizontally:

- **Left 24 cols**: large TEMPO dial — Canvas+Braille circle at `x_bounds([-12,12])` /
  `y_bounds([-9,9])` / `radius = 6.5`. Corrected 4:3 aspect ratio for round appearance in 2:1
  terminal cells. Ivory background ring, warm→hot active arc, amber needle, 0–10 scale labels,
  `BPM / value` centre readout, `TEMPO` label below.
- **Right remainder**: LED spectrum bars (`render_808_spectrum`) — existing warm→hot column bars.

BPM maps `60–200 BPM → 0.0–1.0` for needle position.

#### 808 colour palette

`Classic808Colors` (classic mode):

- Red `#D7262E`, Orange `#F05A28`, Amber `#F6A623`, Yellow `#FFD400`
- Ivory `#EEEADC` — used for knob/dial background rings and TEMPO label
- Grey `#C9C9C9`, Dim `#666666`

#### Next-level 808 usability and delight suggestions

The reference TR-808 hardware puts the user's eyes first on identity and sound-shaping controls,
then on the step sequencer. Expanded 808 should follow that same reading order:

- **Now Playing as the left anchor**: album art and transport metadata are first in the scan path.
  This gives the screen an immediate "record sleeve + deck" identity instead of making the user
  cross the terminal to understand what is playing.
- **Browser as the crate, Playlist as the sequencer**: the middle browser should feel like source
  selection; the right playlist should feel like the 16-step strip. Playlist rows can borrow
  step-button language: amber/red current row, dim grey queued rows, and small numbered cells for
  queue positions or repeat/shuffle state.
- **Section labels from the hardware**: prefer TR-808 phrasing where it helps orientation:
  `Rhythm Composer` for the top identity line, `Instrument Select` for browser/source state,
  `Basic Rhythm` or `Pattern` for playlist state, and `Master Volume` for volume.
- **Transport chips as hardware buttons**: render play/pause/stop/buffer/error as filled button
  states using the 808 yellow/orange/red family. Avoid relying only on text colour; the screenshot
  shows the paused state is readable, but it can feel detached from the rest of the chrome.
- **Step strip under playlist**: add a compact 16-cell row below or inside the playlist pane for
  track position, queue state, or beat subdivision. This is the strongest "soul of the 808"
  opportunity because it references the iconic red/orange/yellow/white buttons without adding new
  audio work.
- **Motion hierarchy**: keep animation strongest in the visualizer and transport feedback, softer
  on pane borders, and nearly absent in Browser focus. The existing motion tuning already points in
  this direction; expanded mode should preserve calm navigation while still feeling alive.
- **Context-aware help**: replace the static bottom chips with focus-aware chips. Browser focus
  should surface `Enter load`, `← parent`, `→ open`; Playlist focus should surface queue and play
  actions; EQ focus should surface band and gain controls.
- **Responsive fallback**: at widths below roughly 150 cols, keep the same pane order but let
  playlist metadata compress before Now Playing art does. Album art is the brand anchor in this
  mode and should be the last thing to disappear.

### 7. Config persistence

```toml
view_mode = "compact"   # "compact" | "expanded" | "808" | "808_expanded"
```

Loaded on startup, saved on clean exit. Defaults to `"compact"` if missing or unrecognised.
`"808_expanded"` is preserved through the `clamp()` normalisation (not reset to `"808"`).

## Consequences

* Good, because wider terminals are fully utilised — no dead space
* Good, because playlist, art, metadata, and spectrogram are simultaneously visible
* Good, because `ViewMode` enum eliminates the impossible `mode_808 && mode_expanded` state
* Good, because the heatmap reuses existing 10-band FFT data — no new audio work
* Good, because compact and 808 paths are untouched — zero regression risk
* Good, because 808 Expanded brings the physical TR-808 layout into TUI form: knobs above,
  TEMPO dial left of spectrum bars, just like the hardware
* Bad, because expanded mode is unusable below ~120 × 36 / ~120 × 44 (mitigated by size guard)
* Bad, because `PlaylistExplorer` is always alive in Expanded mode (small allocation cost)
* Neutral, because the file-browser-as-column changes the `L` key semantic in Expanded mode
  only; compact behaviour is preserved

## Implementation Status

Original implementation steps complete. The 2026-06-14 refinement moves Now Playing/art to
the left, followed by File Browser and Playlist, and adds render-order regression coverage.

| Step | Status | Notes |
| --- | --- | --- |
| `ViewMode` enum + `mode_808` removal | ✅ | `view_mode.rs` |
| W/8 key toggle symmetry | ✅ | `keys.rs` |
| Standard Expanded render | ✅ | `view_expanded.rs` |
| Expanded pane order: Now Playing → Browser → Playlist | ✅ | covered by render-order tests |
| File browser always-on column | ✅ | |
| Playlist pane with track numbers | ✅ | |
| Album art embedded in Now Playing | ✅ | `cover_art_proto_expanded` 32×24 |
| Now Playing metadata (time rem, next track, freq EQ colours) | ✅ | |
| Heatmap spectrogram | ✅ | |
| Focus system (Tab cycle, border highlight) | ✅ | |
| Config persistence for all 4 variants | ✅ | `config.rs` |
| Size guards (120×36 / 120×44) | ✅ | |
| 808 Expanded: TR-808 header | ✅ | `render_808_header` |
| 808 Expanded: knob bar (VOL + 10 EQ) | ✅ | `render_808_expanded_knobs` |
| 808 Expanded: TEMPO dial (Canvas+Braille) | ✅ | `render_tempo_dial` |
| 808 Expanded: LED spectrum | ✅ | `render_808_spectrum` |
| 808 Expanded: chip help bar | ✅ | `render_808_expanded_help` |
| Knob roundness fix (4-row canvas, corrected bounds) | ✅ | `Length(7)` bar, `radius=3.5` |
| Ivory knob rings + cream colour in palette | ✅ | `Classic808Colors.ivory` |

## Alternatives Considered

* **Four-column layout (Browser/Playlist/Art/Now Playing)**: originally planned but album art
  as a standalone column wasted space and split the reading eye. Merged art into Now Playing
  as a left-side subcolumn — better cohesion.
* **Two booleans (`mode_808`, `mode_expanded`)**: allows impossible combined state; rejected
* **Trait-based `View` objects (`Box<dyn View>`)**: too much abstraction for four variants;
  existing pattern of render function per mode is simpler and consistent
* **More than 10 bands in spectrogram**: would require a second FFT pass; 10 bands reuse
  existing smoothed data at zero extra cost
* **Heatmap using ▀/▄ half-block packing**: halves row count to 5 terminal rows; saves space
  but reduces frequency resolution clarity — can be revisited
* **Browser as overlay in expanded mode**: inconsistent with the "always-on sidebar" mental
  model; rejected in favour of permanent column
* **ASCII/Unicode dots for knob arcs**: snaps to cell grid, produces uneven arcs. Replaced
  with `CanvasLine` segment loops for sub-character Braille smoothing.
* **TEMPO dial as separate dedicated row**: costs extra vertical space. Split into the existing
  visualizer row instead (left 24 cols = dial, right = spectrum) at zero height cost.

## More Information

* SparkPlayer reference: <https://forum.ratatui.rs/t/sparkplayer-a-fun-tui-music-and-video-player/305>
* ratatui Canvas docs: <https://ratatui.rs/examples/widgets/canvas/>
* Related ADRs: ADR-0010 (808 chrome), ADR-0009 (oscilloscope), ADR-0008 (cover art)
