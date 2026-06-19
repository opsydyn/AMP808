# Ratzilla 808 Web V2 Hardware And Audio Design

## Goal

AMP808 Web V2 should move from an 808-themed terminal dashboard to a browser-native TR-808
instrument surface. The order is intentional: first make the UI read as credible 808 hardware, then
layer audio-reactive spectacle on top of that hardware.

## Context

V1 is live on GitHub Pages with the accepted Ratzilla web architecture:

- `apps/web` renders through Ratzilla `WebGl2Backend`.
- Browser playback is owned by `HTMLAudioElement`.
- Web Audio analyser data drives visual state.
- Local file picker and CORS-compatible hosted URL input are in scope.
- Static hosting remains a hard constraint.

The current UI has the right broad palette and a working playback shell, but it still reads as a
terminal control grid. The Roland TR-808 reference is more physical: cream faceplate, black inset
control panels, orange rails and branding, dense vertical instrument channels, small cream labels,
round knobs, switch caps, lamps, and large red/orange/yellow/ivory step buttons.

## Design Direction

### Track 1: Hardware Accuracy

The first V2 slice should make the app visibly closer to the original TR-808 before adding more
motion.

- Use the canonical ivory/cream as a structural faceplate color, not only as text.
- Keep black panels as inset hardware bays for controls and analyser content.
- Use orange rails and typography for the `Rhythm Composer TR-808 WEB` identity.
- Replace the small text-heavy step strip with large physical keycaps:
  - steps 1-4 red
  - steps 5-8 orange
  - steps 9-12 yellow
  - steps 13-16 ivory
- Rework the instrument area into tighter vertical channels:
  - top knob or two-knob stack where the instrument supports it
  - scale ticks around each knob
  - short instrument label cap
  - parameter label cap such as `LEVEL`, `TONE`, `TUNE`, `DECAY`, or `SNAP`
- Reduce the huge uninterrupted black void in the analyser area by giving paused, idle, blocked,
  and playing states distinct hardware-like surfaces.
- Make the lower browser controls visually belong to the machine. They can remain DOM controls for
  accessibility, but their styling should echo the 808 button strip instead of a generic web form.

### Track 2: Audio-Reactive Spectacle

After the hardware pass, TachyonFX and analyser rendering should make the machine feel alive without
faking audio data.

- Add three visual modes:
  - `SCOPE`: waveform trace with short history and decay.
  - `SPECTRUM`: frequency bars with peak hold.
  - `RHYTHM MAP`: step-key and instrument-lane activity derived from analyser energy.
- Add play-state motion:
  - step-key LED chase while playing
  - transient pulse on strong analyser peaks
  - active orange border trace around the current panel
  - short load, play, pause, error, and blocked-source transitions
- Keep reduced-motion behavior available through the current motion toggle.
- Preserve the rule from the web ADR: CORS-blocked or analyser-unavailable sources must show an
  explicit blocked state rather than fake motion.

## UI Composition

The desktop layout should read as a single 808 machine:

1. **Top Brand Rail**: model name, source, state, and transport time embedded into an orange/ivory
   brand strip.
2. **Left Utility Bay**: mode, playback state, recent sources, tempo/BPM gauge, master volume, and
   A/B variation status.
3. **Instrument Control Bay**: black inset panel with dense vertical instrument channels and knob
   stacks.
4. **Audio Surface**: large central analyser bay with hardware-framed scope, spectrum, or rhythm-map
   mode.
5. **Step Button Strip**: large colored 16-step keycaps with labels, playhead, tap/start/stop
   affordances, and motion-reactive LEDs.
6. **Accessible Browser Control Strip**: file picker, seek, play, URL input, load button, and motion
   toggle styled as an 808 control extension.

Mobile should preserve identity rather than simply shrinking the desktop grid. The first mobile
priority is no overlap, no horizontal scroll, and usable playback controls. It is acceptable for
mobile to stack the utility bay, analyser, and step strip while using compact instrument channels.

## Architecture

This design should stay inside the existing web architecture:

- Keep Ratzilla `WebGl2Backend` as the primary renderer.
- Keep `HTMLAudioElement` as the browser transport.
- Keep Web Audio analyser bytes as the source of real audio visuals.
- Use Ratatui widgets, Canvas/Braille drawing, and TachyonFX post-render effects for visual polish.
- Do not add a shader layer, AudioWorklet, persistent browser storage, or bundled audio sample in
  this V2 design.

The likely code direction is to split the current large `apps/web/src/main.rs` rendering concerns
into small units as implementation work begins:

- palette and contrast helpers
- hardware panel primitives
- 808 keycap/step-strip widget
- instrument-channel widget
- analyser visual-mode renderer
- TachyonFX effect composition
- browser-control state mapping

The split should happen only where it supports the V2 work; unrelated refactors are out of scope.

## Data Flow

Playback state continues to flow from the browser audio layer into the terminal UI:

1. User selects a local file or enters a hosted URL.
2. `HTMLAudioElement` owns loading, play, pause, seek, duration, and media errors.
3. Web Audio analyser data is sampled only when available.
4. Normalized analyser data drives scope, spectrum, rhythm-map, BPM estimate, LEDs, and TachyonFX
   intensity.
5. UI state explicitly distinguishes idle, loading, playing, paused, ended, media error,
   autoplay-refused, unsupported codec, and CORS/analyser-blocked.

## Error Handling

V2 should make failure states visually deliberate:

- **No source**: hardware idle state with source prompt.
- **Loading**: short panel trace and source label transition.
- **Paused**: frozen analyser frame, dimmed LEDs, visible pause state.
- **CORS/analyser blocked**: blocked-source panel state with clear copy and no fake analyser motion.
- **Media/network error**: red/orange warning state with source retained for correction.
- **Unsupported codec**: explicit unsupported-source message.
- **Autoplay refused**: prompt the user to press play.

## Accessibility

The visual upgrade cannot reduce usability:

- Normal text and critical labels must pass WCAG AA contrast.
- Red, orange, yellow, and ivory step groups must also be distinguishable by position and labels.
- Motion must be disableable through the existing motion toggle.
- DOM controls remain keyboard accessible.
- Canvas-only state must be echoed in text where it affects operation.
- Mobile layout must avoid overlap, horizontal scroll, and clipped controls.

## Testing And Verification

The implementation plan should include:

- contrast tests for any new palette pairs
- unit tests for visual-mode state mapping where pure logic can be extracted
- wasm check for `amp808_web`
- workspace tests
- clippy for the web target
- Trunk release build from `apps/web`
- desktop screenshot smoke check
- mobile screenshot smoke check around 390px width
- manual local-file playback with analyser movement
- hosted CORS-compatible URL playback check
- hosted CORS-blocked URL error-state check
- reduced-motion visual check

## Non-Goals

- No native audio backend changes.
- No `cpal`, `ffmpeg`, `yt-dlp`, or native filesystem assumptions in the web crate.
- No AudioWorklet timing engine.
- No shader or custom WebGL layer outside Ratzilla.
- No bundled demo/sample audio until licensing and same-origin hosting are decided separately.
- No fake analyser motion for blocked or unavailable sources.
- No full desktop native UI redesign in this V2 web design.

## Acceptance Criteria

V2 is successful when:

- A first-time viewer can identify the UI as TR-808-inspired before reading any explanatory text.
- The step strip reads as physical colored 808-style buttons.
- Instrument controls read as tight hardware channels, not loose terminal labels.
- The analyser bay has meaningful idle, paused, playing, and blocked states.
- Motion enhances active audio state without making labels hard to read.
- Local file playback still works.
- CORS-compatible hosted URL playback still works.
- CORS-blocked hosted URLs fail clearly.
- Desktop and mobile screenshots show no incoherent overlaps or clipped controls.
