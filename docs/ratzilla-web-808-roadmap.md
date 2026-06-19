# Mighty Ratzilla 808 Web Roadmap

## North Star

AMP808 Web should feel like a browser-native TR-808 instrument panel, not a terminal app pasted into
a page. The fidelity target is:

- **TR-808 hardware identity**: canonical cream, orange rails, red/orange/yellow/ivory step keys,
  black faceplate, labeled instrument strips, round knobs, lamps, switches, and panel hierarchy.
- **Exabind-level Ratzilla polish**: dense but legible panels, strong colored focus borders, animated
  cell effects, no dead empty regions, and clear information hierarchy inside the terminal grid.
- **Real playback**: browser-owned audio transport with Web Audio analyser data driving the display.
- **Static hosting**: GitHub Pages compatible, with local file support and hosted URLs only when CORS
  allows browser playback and analysis.
- **Accessibility by default**: all text and critical controls meet contrast requirements; color is
  reinforced by labels, position, shape, and state.

## Design Principles

- Prefer the **canonical 808 cream** `#EEEADC` for primary text, rings, labels, and high-value
  controls. Use orange/yellow/red as accents, not as the only carrier of meaning.
- Keep normal text contrast at **WCAG AA 4.5:1 or better** against the black faceplate. Use red for
  lamps and buttons, not long text, unless the red is brightened enough to pass contrast.
- Render knobs and dials with **Ratatui Canvas + Braille markers** where available. Text glyph knobs
  are acceptable only as compact fallbacks.
- Treat Ratzilla as a **Ratatui buffer renderer**, not a web plugin host. Extensions should be
  custom widgets, Canvas drawing, and TachyonFX post-render cell effects.
- Use TachyonFX sparingly: active panel traces, focus pulses, fades, and analyser-reactive glow. The
  faceplate should feel alive, not noisy.
- Preserve the accepted web ADR boundaries: no native `cpal`, `crossterm`, `ffmpeg`, `yt-dlp`,
  Music.app automation, or native filesystem assumptions inside the web crate.

## Current State

- `apps/web` exists as a Ratzilla `WebGl2Backend` app.
- Browser playback is wired with `HTMLAudioElement`.
- Local file picker and hosted URL input exist.
- Web Audio analyser bytes are normalized in `amp808-core`.
- The web faceplate has an initial TR-808-inspired layout with transport state, analyser, step
  buttons, and browser controls below the canvas.
- `/apps/web/dist/` is ignored so Trunk builds do not dirty the repository.

## Phase 1: Fidelity Foundation

Goal: make the web faceplate look intentional and robust before adding more motion.

- [x] Move web 808 palette into a named Rust helper.
- [x] Define canonical colors:
  - `IVORY = #EEEADC`
  - `FACEPLATE = #090A08`
  - `ORANGE = #F05A28`
  - `AMBER = #F6A623`
  - `YELLOW = #FFD400`
  - `RED = #D7262E` for lamps/buttons only unless adjusted for AA text contrast
- [x] Add contrast tests for palette pairs used in normal text, labels, borders, lamps, and buttons.
- [x] Replace text knobs with Canvas/Braille round knobs for instrument controls.
- [x] Replace the ASCII tempo dial with a Canvas/Braille dial based on native 808 geometry.
- [x] Add compact fallbacks for tiny terminal widths.
- [x] Verify desktop and mobile screenshots after every visual pass.

Acceptance:

- Normal text and labels pass AA contrast.
- Knobs are visibly round at desktop width.
- Mobile width has no horizontal scroll and no text collisions.
- Existing playback and analyser behavior remains intact.

## Phase 2: Exabind-Grade Panel System

Goal: make the UI read like a polished Ratzilla application with clear panel ownership and active
state.

- [ ] Add a reusable panel renderer for 808 sections: title tab, border style, active/inactive
  state, and optional status lamp.
- [ ] Make active source/transport/focus visible with colored borders, not only text.
- [ ] Add a richer instrument strip: levels, tone/decay labels, and step grouping aligned to the
  hardware reference.
- [ ] Improve the analyser window so idle, loading, playing, paused, and error states all have
  intentional visuals.
- [ ] Add keyboard affordances inside the terminal grid for playback, loading, and visual modes.
- [ ] Keep browser controls below the canvas until terminal-native controls are complete and
  accessible.

Acceptance:

- The page has no visually empty “placeholder” areas at desktop size.
- Users can identify source, state, transport, and analyser status from the terminal canvas alone.
- The browser control strip remains accessible and does not overlap terminal content.

## Phase 3: TachyonFX Motion Layer

Goal: introduce motion like Exabind without compromising readability or playback.

- [ ] Add `tachyonfx` to `apps/web` only after confirming wasm target compatibility in CI/local
  checks.
- [ ] Port or adapt native 808 header/panel trace effects from `src/ui/view_808.rs`.
- [ ] Add active-panel border trace for the currently relevant area.
- [ ] Add short load/play/error transition effects using fades or color shifts.
- [ ] Add analyser-reactive glow for step keys and LEDs.
- [ ] Add a reduced-motion switch or compile/runtime guard if motion becomes distracting.

Acceptance:

- `cargo check -p amp808_web --target wasm32-unknown-unknown` passes with TachyonFX enabled.
- Effects are UI-only and do not touch audio graph timing.
- Motion remains subtle enough that text is always readable.

## Phase 4: Playback Depth

Goal: move from “plays a single source” to a lightweight web player.

- [ ] Add seek support through `HTMLAudioElement.currentTime`.
- [ ] Add duration/progress rendering in the 808 panel.
- [ ] Add recent source list in memory for the browser session.
- [ ] Add static demo/sample source support only if legally distributable and same-origin.
- [ ] Improve hosted URL error states:
  - media load failed
  - CORS/analyser unavailable
  - browser autoplay refused
  - unsupported codec/container
- [ ] Add explicit visual state for CORS-blocked hosted URLs without fake analyser motion.

Acceptance:

- Local files play, pause, seek, and drive analyser bars.
- CORS-enabled hosted URLs play and drive analyser bars.
- Non-CORS URLs fail with a clear visible message.

## Phase 5: Static Deployment

Goal: make the web target easy to ship.

- [ ] Add a documented Trunk build command for GitHub Pages.
- [ ] Add GitHub Actions build artifact for `apps/web/dist`.
- [ ] Add optional Pages publish workflow once the repository target URL is chosen.
- [ ] Add README section for local development, local files, hosted URL CORS, and browser support.
- [ ] Add screenshot artifacts for desktop and mobile QA.

Acceptance:

- The web build is reproducible from a clean checkout.
- The generated static output can be served without backend code.
- The deployment docs state exactly what hosted URLs must provide for CORS-compatible playback.

## Verification Matrix

Run these before marking any web roadmap phase complete:

- `cargo fmt --all --check`
- `cargo test --workspace --locked`
- `cargo check --locked -p amp808_web --target wasm32-unknown-unknown`
- `cargo clippy -p amp808_web --target wasm32-unknown-unknown -- -D warnings`
- `NO_COLOR=false trunk build --release --public-url /` from `apps/web`
- Browser smoke check at desktop width
- Browser smoke check around 390px mobile width
- Manual local-file playback check with analyser movement
- Hosted URL CORS success/failure checks when changing URL behavior

## Reference Links

- Exabind web demo: <https://junkdog.github.io/exabind/>
- TachyonFX FTL playground: <https://junkdog.github.io/tachyonfx-ftl/>
- TachyonFX docs: <https://docs.rs/tachyonfx/latest/tachyonfx/>
- Ratzilla backend docs: <https://docs.rs/ratzilla/latest/ratzilla/backend/index.html>
- Ratatui Canvas docs: <https://docs.rs/ratatui/latest/ratatui/widgets/canvas/struct.Canvas.html>
- Web target ADR: `adr/0017-add-ratzilla-web-808-player-target.md`
- Native 808 TachyonFX ADR: `adr/0010-tachyonfx-animated-808-chrome-and-visualizer-parity.md`
