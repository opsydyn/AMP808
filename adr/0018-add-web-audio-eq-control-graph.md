---
status: accepted
date: 2026-06-20
decision-makers: alan
---

# Add a Web Audio EQ control graph for browser knobs

## Context and Problem Statement

The Ratzilla web target now plays browser-selected local files and CORS-enabled hosted URLs through
an `HTMLAudioElement`, with analyser data rendered into the 808-inspired UI. The instrument strip
currently renders knobs that look like TR-808 controls, but those knobs are visual only.

The native TUI already has real playback controls for master volume, ten EQ bands, mono, and named
EQ presets. The web target needs the same first layer of audible control without pretending that an
arbitrary mixed audio file has separately controllable drum stems.

The question is: how should the web player make the 808 knob strip affect browser playback while
preserving the static GitHub Pages and `HTMLAudioElement` decisions from ADR-0017?

## Decision Drivers

- Match the native/TUI control model where it applies to browser playback.
- Keep `HTMLAudioElement` as the browser transport owner for load, play, pause, seek, and duration.
- Keep the web app static-hostable with no backend, proxy, or native audio dependencies.
- Avoid misleading per-instrument stem controls for arbitrary full-track playback.
- Keep the first implementation test-first by separating pure control state from Web Audio wiring.
- Preserve analyser-driven visualizers and allow EQ changes to affect both playback and the scope.

## Considered Options

- Add a Web Audio gain plus ten-band peaking EQ chain after the media element.
- Build a custom Web Audio decoder and scheduler for deeper DSP control.
- Compile the native Rust DSP pipeline to wasm and run it on decoded samples.
- Keep the knobs visual-only until full drum-stem or synth playback exists.

## Decision Outcome

Chosen option: "Add a Web Audio gain plus ten-band peaking EQ chain after the media element".

The web graph will become:

```text
HTMLAudioElement
  -> MediaElementAudioSourceNode
  -> BiquadFilterNode[10]
  -> GainNode
  -> AnalyserNode
  -> AudioDestinationNode
```

The ten filters use the same center frequencies as the native EQ:

```text
70 Hz, 180 Hz, 320 Hz, 600 Hz, 1 kHz, 3 kHz, 6 kHz, 12 kHz, 14 kHz, 16 kHz
```

The web UI will keep the 808-inspired knob strip, but the first functional slice maps it to browser
audio controls:

- master volume in dB
- ten EQ band gains in dB
- named sound-mode/EQ presets matching the native TUI presets

The analyser will sit after the EQ and gain nodes, so visualizer output reflects the audible browser
playback rather than the unprocessed source.

## Non-goals

- No custom browser decoder or Web Audio sample scheduler in this slice.
- No per-drum stem mixing for arbitrary hosted/local tracks.
- No browser implementation of native `cpal` playback.
- No backend service or CORS proxy.
- No promise that arbitrary external URLs can be loaded or analysed without CORS support.
- No mouse-drag knob editing in the first slice; keyboard control is enough to prove the audio path.

## Consequences

- Good, because web knobs become audible while preserving the browser-native media transport.
- Good, because the same named EQ presets can exist in native TUI and web UI.
- Good, because the analyser and visual effects respond to the post-EQ signal.
- Good, because pure control math can be covered by normal Rust tests without a browser.
- Bad, because this duplicates some native EQ preset data until a smaller shared control module is
  extracted.
- Bad, because Web Audio filter behavior will not be sample-identical to the native Rust biquad DSP.
- Neutral, because the UI remains 808-inspired rather than hardware-accurate per-voice control for
  mixed audio.

## Implementation Plan

- Add the required `web-sys` features for `GainNode`, `BiquadFilterNode`, `BiquadFilterType`, and
  `AudioParam`.
- Add pure web audio control state for master volume, ten EQ bands, selected knob, and selected
  preset.
- Add tests for volume/EQ normalization, preset cycling, selection movement, clamping, and keyboard
  shortcut mapping.
- Replace fake knob values with values derived from the web audio control state.
- Add keyboard controls for:
  - `e`: cycle named sound mode/EQ preset
  - `[` and `]`: select previous/next audio control
  - up/down or `k`/`j`: adjust selected control
  - `+`/`-`: adjust master volume
- Wire the Web Audio graph to apply gain and EQ state after each control change.
- Update footer/status copy so the controls are discoverable without adding a separate help screen.

## Verification

- [ ] `cargo test -p amp808_web` passes.
- [ ] `cargo fmt --all --check` passes.
- [ ] `trunk build --release` from `apps/web` succeeds.
- [ ] In the browser, local audio still loads and plays.
- [ ] In the browser, `e` cycles sound modes and visibly moves the EQ knobs.
- [ ] In the browser, volume and EQ keyboard adjustments audibly affect playback.
- [ ] The analyser continues to render real audio data after the EQ/gain chain.

