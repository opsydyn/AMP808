# Ratzilla 808 V2 Hardware Accuracy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make AMP808 Web read as a faithful TR-808-inspired hardware faceplate before adding the next audio-reactive spectacle layer.

**Architecture:** Stay inside the accepted `apps/web` Ratzilla/WebAudio architecture. Keep `HTMLAudioElement` playback and analyser behavior unchanged, use pure helper functions for testable palette/layout/keycap decisions, and update Ratatui rendering plus DOM control styling for the hardware pass.

**Tech Stack:** Rust 2021 in `apps/web`, Ratatui through Ratzilla `0.3.1`, `WebGl2Backend`, TachyonFX `0.25`, Web Audio analyser state, Trunk, HTML/CSS for the accessible browser control strip.

---

## File Structure

- Modify `apps/web/src/main.rs` for palette, hardware panel styles, layout geometry, instrument channels, step keycaps, analyser empty states, and pure tests.
- Modify `apps/web/index.html` for accessible DOM controls styled like an 808 button extension.
- Keep `crates/amp808-core` unchanged in this slice.
- Keep playback, CORS policy, analyser sampling, BPM estimation, and TachyonFX timing unchanged in this slice.

## Task 1: Hardware Faceplate Palette

**Files:**
- Modify: `apps/web/src/main.rs`

- [ ] **Step 1: Write the failing palette and style tests**

Add the new imports inside the `use super::{ ... }` list in `#[cfg(test)] mod tests`:

```rust
        classic_body_border_style, classic_hardware_body_style, classic_panel_inset_style,
        hardware_brand_style,
```

Add these tests near the existing palette contrast tests:

```rust
    #[test]
    fn hardware_body_palette_keeps_text_and_brand_readable() {
        assert!(
            contrast_ratio(Classic808Palette::IVORY, Classic808Palette::BODY) >= 4.5,
            "ivory body text should pass AA contrast on the dark hardware body"
        );
        assert!(
            contrast_ratio(Classic808Palette::BRAND_ORANGE, Classic808Palette::BODY) >= 4.5,
            "brand orange should pass AA contrast on the dark hardware body"
        );
        assert!(
            contrast_ratio(Classic808Palette::BODY, Classic808Palette::FACEPLATE) < 1.3,
            "hardware body should stay close to black so panel separation is subtle"
        );
    }

    #[test]
    fn hardware_styles_separate_body_from_black_inset_panels() {
        assert_eq!(
            classic_hardware_body_style().bg,
            Some(Classic808Palette::BODY.ratatui())
        );
        assert_eq!(
            classic_hardware_body_style().fg,
            Some(Classic808Palette::IVORY.ratatui())
        );
        assert_eq!(
            classic_panel_inset_style().bg,
            Some(Classic808Palette::FACEPLATE.ratatui())
        );
        assert_eq!(
            classic_panel_inset_style().fg,
            Some(Classic808Palette::IVORY.ratatui())
        );
        assert_eq!(
            hardware_brand_style().fg,
            Some(Classic808Palette::BRAND_ORANGE.ratatui())
        );
        assert_eq!(
            hardware_brand_style().bg,
            Some(Classic808Palette::BODY.ratatui())
        );
        assert_eq!(
            hardware_body_text_style().fg,
            Some(Classic808Palette::IVORY.ratatui())
        );
        assert_eq!(
            hardware_body_text_style().bg,
            Some(Classic808Palette::BODY.ratatui())
        );
        assert_eq!(
            classic_body_border_style().fg,
            Some(Classic808Palette::BRAND_ORANGE.ratatui())
        );
    }
```

- [ ] **Step 2: Run the tests to verify RED**

Run:

```bash
cargo test -p amp808_web hardware_body_palette_keeps_text_and_brand_readable
cargo test -p amp808_web hardware_styles_separate_body_from_black_inset_panels
```

Expected: FAIL because `BODY`, `BRAND_ORANGE`, and the new style helpers do not exist.

- [ ] **Step 3: Implement the palette and style helpers**

Add these constants to `impl Classic808Palette`:

```rust
    const BODY: ClassicColor = ClassicColor::new(0x15, 0x17, 0x12);
    const BRAND_ORANGE: ClassicColor = ClassicColor::new(0xf0, 0x5a, 0x28);
```

Add these helpers near the existing style helpers:

```rust
fn classic_hardware_body_style() -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .bg(Classic808Palette::BODY.ratatui())
}

fn classic_panel_inset_style() -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .bg(Classic808Palette::FACEPLATE.ratatui())
}

fn hardware_brand_style() -> Style {
    Style::default()
        .fg(Classic808Palette::BRAND_ORANGE.ratatui())
        .bg(Classic808Palette::BODY.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn hardware_body_text_style() -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .bg(Classic808Palette::BODY.ratatui())
}

fn classic_body_border_style() -> Style {
    Style::default().fg(Classic808Palette::BRAND_ORANGE.ratatui())
}
```

- [ ] **Step 4: Apply the body and inset styles to the existing renderers**

In `render_web_808`, replace the outer block style and border style:

```rust
        .style(classic_hardware_body_style())
        .borders(Borders::ALL)
        .border_set(web_panel_border_set())
        .border_style(classic_body_border_style());
```

In `render_808_panel`, replace the block style:

```rust
        .style(classic_panel_inset_style())
```

Keep `classic_faceplate_style()` in place for compact text widgets that still need black-panel styling.

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```bash
cargo test -p amp808_web hardware_body_palette_keeps_text_and_brand_readable
cargo test -p amp808_web hardware_styles_separate_body_from_black_inset_panels
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/web/src/main.rs
git commit -m "feat(web): add 808 hardware faceplate palette"
```

## Task 2: Hardware Layout Proportions

**Files:**
- Modify: `apps/web/src/main.rs`

- [ ] **Step 1: Write failing layout tests**

Add these tests near the existing layout gutter tests:

```rust
    #[test]
    fn web_desktop_layout_gives_hardware_controls_and_step_keys_more_presence() {
        let deck = web_desktop_deck_layout(Rect::new(30, 0, 150, 54));

        assert_eq!(deck.len(), 3);
        assert_eq!(deck[0].height, 10);
        assert_eq!(deck[2].height, 7);
        assert_eq!(deck[1].y, deck[0].y + deck[0].height + 1);
        assert_eq!(deck[2].y, deck[1].y + deck[1].height + 1);
    }

    #[test]
    fn web_compact_layout_preserves_step_key_presence() {
        let deck = web_compact_deck_layout(Rect::new(0, 0, 80, 44));

        assert_eq!(deck.len(), 4);
        assert_eq!(deck[1].height, 9);
        assert_eq!(deck[3].height, 7);
        assert_eq!(deck[1].y, deck[0].y + deck[0].height + 1);
        assert_eq!(deck[3].y, deck[2].y + deck[2].height + 1);
    }
```

- [ ] **Step 2: Run the tests to verify RED**

Run:

```bash
cargo test -p amp808_web web_desktop_layout_gives_hardware_controls_and_step_keys_more_presence
cargo test -p amp808_web web_compact_layout_preserves_step_key_presence
```

Expected: FAIL because the current instrument row is `7`, compact instrument row is `7`, and step row is `5`.

- [ ] **Step 3: Update the deck layout constraints**

Replace `web_compact_deck_layout` constraints with:

```rust
        .constraints([
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Min(8),
            Constraint::Length(7),
        ])
```

Replace `web_desktop_deck_layout` constraints with:

```rust
        .constraints([
            Constraint::Length(10),
            Constraint::Min(10),
            Constraint::Length(7),
        ])
```

- [ ] **Step 4: Run tests to verify GREEN**

Run:

```bash
cargo test -p amp808_web web_desktop_layout_gives_hardware_controls_and_step_keys_more_presence
cargo test -p amp808_web web_compact_layout_preserves_step_key_presence
cargo test -p amp808_web web_desktop_layout_leaves_gutters_between_heavy_borders
cargo test -p amp808_web web_compact_layout_leaves_vertical_gutters_between_panels
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/web/src/main.rs
git commit -m "feat(web): rebalance 808 hardware layout"
```

## Task 3: TR-808 Brand Rail

**Files:**
- Modify: `apps/web/src/main.rs`

- [ ] **Step 1: Write the failing brand-label test**

Add `machine_brand_label` to the `use super::{ ... }` list.

Add this test near the formatting tests:

```rust
    #[test]
    fn machine_brand_label_uses_tr_808_model_identity() {
        assert_eq!(
            machine_brand_label(),
            "Roland Rhythm Composer TR-808 WEB"
        );
    }
```

- [ ] **Step 2: Run the test to verify RED**

Run:

```bash
cargo test -p amp808_web machine_brand_label_uses_tr_808_model_identity
```

Expected: FAIL because `machine_brand_label` does not exist.

- [ ] **Step 3: Add the brand helper**

Add this helper near `render_machine_header`:

```rust
fn machine_brand_label() -> &'static str {
    "Roland Rhythm Composer TR-808 WEB"
}
```

- [ ] **Step 4: Render the brand rail with dark body styling**

Replace the first `Line` in `render_machine_header` with:

```rust
        Line::from(vec![
            Span::styled("Roland ", hardware_brand_style()),
            Span::styled(
                "Rhythm Composer ",
                Style::default()
                    .fg(Classic808Palette::FACEPLATE.ratatui())
                    .bg(Classic808Palette::BODY.ratatui())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("TR-808 WEB", hardware_brand_style()),
        ]),
```

Set the rendered paragraph style to the hardware body style:

```rust
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(classic_hardware_body_style())
            .alignment(Alignment::Center),
        area,
    );
```

- [ ] **Step 5: Run the test to verify GREEN**

Run:

```bash
cargo test -p amp808_web machine_brand_label_uses_tr_808_model_identity
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/web/src/main.rs
git commit -m "feat(web): add TR-808 hardware brand rail"
```

## Task 4: Instrument Hardware Channels

**Files:**
- Modify: `apps/web/src/main.rs`

- [ ] **Step 1: Write failing tests for roomier channels and label caps**

Add these imports to the `use super::{ ... }` list:

```rust
        instrument_channel_visible_count, instrument_label_cap_text,
```

Add these tests near `instrument_control_specs_match_808_web_strip`:

```rust
    #[test]
    fn instrument_channel_visible_count_prefers_roomier_hardware_slots() {
        assert_eq!(instrument_channel_visible_count(20), 2);
        assert_eq!(instrument_channel_visible_count(72), 9);
        assert_eq!(instrument_channel_visible_count(96), 12);
    }

    #[test]
    fn instrument_label_cap_text_keeps_short_and_parameter_labels_together() {
        let spec = instrument_control_specs()[9];

        assert_eq!(instrument_label_cap_text(&spec), "CB TUNE");
    }
```

- [ ] **Step 2: Run the tests to verify RED**

Run:

```bash
cargo test -p amp808_web instrument_channel_visible_count_prefers_roomier_hardware_slots
cargo test -p amp808_web instrument_label_cap_text_keeps_short_and_parameter_labels_together
```

Expected: FAIL because the helper functions do not exist.

- [ ] **Step 3: Add the channel helper functions**

Add these functions near `instrument_parameter_spans`:

```rust
fn instrument_channel_visible_count(width: u16) -> usize {
    (usize::from(width) / 8).clamp(1, instrument_control_specs().len())
}

fn instrument_label_cap_text(spec: &InstrumentControlSpec) -> String {
    format!("{} {}", spec.short_label, abbreviate_parameter_label(spec.parameter_label))
}
```

- [ ] **Step 4: Use roomier channel count and two-line hardware caps**

In `render_knob_bank`, replace:

```rust
    let visible = (usize::from(inner.width) / 6).clamp(1, specs.len());
```

with:

```rust
    let visible = instrument_channel_visible_count(inner.width).min(specs.len());
```

Replace the row constraints in `render_knob_bank` with:

```rust
        .constraints([Constraint::Min(5), Constraint::Length(2)])
```

Replace the final parameter row rendering in `render_knob_bank` with:

```rust
    let label_lines = Text::from(vec![
        Line::from(
            specs
                .iter()
                .map(|spec| instrument_short_span(spec, 8))
                .collect::<Vec<_>>(),
        ),
        Line::from(
            specs
                .iter()
                .map(|spec| instrument_parameter_span(spec, 8))
                .collect::<Vec<_>>(),
        ),
    ]);
    frame.render_widget(Paragraph::new(label_lines).alignment(Alignment::Center), rows[1]);
```

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```bash
cargo test -p amp808_web instrument_channel_visible_count_prefers_roomier_hardware_slots
cargo test -p amp808_web instrument_label_cap_text_keeps_short_and_parameter_labels_together
cargo test -p amp808_web instrument_control_specs_match_808_web_strip
cargo test -p amp808_web instrument_knob_canvas_bounds_tighten_wide_slots_toward_round_dials
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/web/src/main.rs
git commit -m "feat(web): tighten 808 instrument channels"
```

## Task 5: Physical Step Keycaps

**Files:**
- Modify: `apps/web/src/main.rs`

- [ ] **Step 1: Write failing keycap style tests**

Add these imports to the `use super::{ ... }` list:

```rust
        classic_step_keycap_style, classic_step_keycap_text_color,
```

Add these tests near `classic_pad_family_matches_tr_808_step_groups`:

```rust
    #[test]
    fn step_keycap_text_colors_pass_on_hardware_button_colors() {
        for family in [
            ClassicPadFamily::Red,
            ClassicPadFamily::Orange,
            ClassicPadFamily::Yellow,
            ClassicPadFamily::Ivory,
        ] {
            let style = classic_step_keycap_style(family, 0.0);
            let foreground = color_to_classic(style.fg.expect("keycap text color"));
            let background = color_to_classic(style.bg.expect("keycap background color"));

            assert!(
                contrast_ratio(foreground, background) >= 4.5,
                "{family:?} keycap text should pass AA contrast"
            );
        }
    }

    #[test]
    fn step_keycap_text_color_uses_ivory_on_dark_red_and_black_elsewhere() {
        assert_eq!(
            classic_step_keycap_text_color(ClassicPadFamily::Red),
            Classic808Palette::IVORY.ratatui()
        );
        assert_eq!(
            classic_step_keycap_text_color(ClassicPadFamily::Orange),
            Classic808Palette::FACEPLATE.ratatui()
        );
        assert_eq!(
            classic_step_keycap_text_color(ClassicPadFamily::Yellow),
            Classic808Palette::FACEPLATE.ratatui()
        );
        assert_eq!(
            classic_step_keycap_text_color(ClassicPadFamily::Ivory),
            Classic808Palette::FACEPLATE.ratatui()
        );
    }
```

- [ ] **Step 2: Run the tests to verify RED**

Run:

```bash
cargo test -p amp808_web step_keycap_text_colors_pass_on_hardware_button_colors
cargo test -p amp808_web step_keycap_text_color_uses_ivory_on_dark_red_and_black_elsewhere
```

Expected: FAIL because the keycap style functions do not exist.

- [ ] **Step 3: Add keycap text and style helpers**

Add these helpers near `classic_pad_style`:

```rust
fn classic_step_keycap_text_color(family: ClassicPadFamily) -> Color {
    match family {
        ClassicPadFamily::Red => Classic808Palette::IVORY.ratatui(),
        ClassicPadFamily::Orange | ClassicPadFamily::Yellow | ClassicPadFamily::Ivory => {
            Classic808Palette::FACEPLATE.ratatui()
        }
    }
}

fn classic_step_keycap_style(family: ClassicPadFamily, glow: f32) -> Style {
    let glow = glow.clamp(0.0, 1.0);
    let base = match family {
        ClassicPadFamily::Red => Color::Rgb(0xa4, 0x21, 0x1a),
        ClassicPadFamily::Orange => Classic808Palette::ORANGE.ratatui(),
        ClassicPadFamily::Yellow => Classic808Palette::YELLOW.ratatui(),
        ClassicPadFamily::Ivory => Classic808Palette::IVORY.ratatui(),
    };
    let hot = match family {
        ClassicPadFamily::Red => Classic808Palette::RED_TEXT.ratatui(),
        ClassicPadFamily::Orange => Classic808Palette::AMBER.ratatui(),
        ClassicPadFamily::Yellow | ClassicPadFamily::Ivory => Classic808Palette::BODY.ratatui(),
    };

    Style::default()
        .fg(classic_step_keycap_text_color(family))
        .bg(mix_rgb(base, hot, glow * 0.25))
        .add_modifier(Modifier::BOLD)
}
```

- [ ] **Step 4: Render larger physical keycaps**

In `render_step_strip`, replace the `step_count` calculation with:

```rust
    let step_count = (usize::from(inner.width) / 6).clamp(1, 16);
```

Replace the `numbers` and `pads` loop body with:

```rust
        numbers.push(Span::styled(
            format!("{:^6}", step + 1),
            classic_small_label_style(),
        ));
        let energy = bands.get(step).copied().unwrap_or_default();
        let glow = step_glow_intensity(state.transport, energy);
        pads.push(Span::styled(
            format!("{:^6}", step + 1),
            classic_step_keycap_style(classic_pad_family(step), glow),
        ));
```

Remove the visual `" ## "` text for keycaps. The number stays visible inside the colored keycap.

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```bash
cargo test -p amp808_web step_keycap_text_colors_pass_on_hardware_button_colors
cargo test -p amp808_web step_keycap_text_color_uses_ivory_on_dark_red_and_black_elsewhere
cargo test -p amp808_web classic_pad_family_matches_tr_808_step_groups
cargo test -p amp808_web step_glow_intensity_uses_analyser_energy_without_faking_idle_motion
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/web/src/main.rs
git commit -m "feat(web): render physical 808 step keycaps"
```

## Task 6: Hardware Empty States For The Analyser Bay

**Files:**
- Modify: `apps/web/src/main.rs`

- [ ] **Step 1: Write failing presentation tests**

Add these imports to the `use super::{ ... }` list:

```rust
        analyser_empty_state_presentation, AnalyserEmptyPresentation,
```

Add these tests near `analyser_empty_state_text_tracks_browser_audio_state`:

```rust
    #[test]
    fn analyser_empty_state_presentation_names_idle_paused_and_blocked_hardware_states() {
        let mut state = WebAppState::default();
        assert_eq!(
            analyser_empty_state_presentation(&state),
            Some(AnalyserEmptyPresentation {
                title: "LOAD AUDIO",
                subtitle: "WEB AUDIO ANALYSER",
                hint: "LOCAL FILE OR CORS URL"
            })
        );

        state.transport = TransportState::Paused;
        assert_eq!(
            analyser_empty_state_presentation(&state),
            Some(AnalyserEmptyPresentation {
                title: "PAUSED",
                subtitle: "FROZEN ANALYSER",
                hint: "PRESS PLAY TO RESUME"
            })
        );

        state.transport = TransportState::Error;
        state.error = Some("CORS blocked".to_string());
        assert_eq!(
            analyser_empty_state_presentation(&state),
            Some(AnalyserEmptyPresentation {
                title: "CHECK SOURCE",
                subtitle: "CORS OR MEDIA ERROR",
                hint: "NO FAKE ANALYSER MOTION"
            })
        );
    }
```

- [ ] **Step 2: Run the test to verify RED**

Run:

```bash
cargo test -p amp808_web analyser_empty_state_presentation_names_idle_paused_and_blocked_hardware_states
```

Expected: FAIL because `AnalyserEmptyPresentation` and `analyser_empty_state_presentation` do not exist.

- [ ] **Step 3: Add the presentation type and helper**

Add this type near `analyser_empty_state_text`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AnalyserEmptyPresentation {
    title: &'static str,
    subtitle: &'static str,
    hint: &'static str,
}

fn analyser_empty_state_presentation(state: &WebAppState) -> Option<AnalyserEmptyPresentation> {
    match state.transport {
        TransportState::Idle | TransportState::Ended => Some(AnalyserEmptyPresentation {
            title: "LOAD AUDIO",
            subtitle: "WEB AUDIO ANALYSER",
            hint: "LOCAL FILE OR CORS URL",
        }),
        TransportState::Ready => Some(AnalyserEmptyPresentation {
            title: "READY",
            subtitle: "ANALYSER ARMED",
            hint: "PRESS PLAY",
        }),
        TransportState::Paused => Some(AnalyserEmptyPresentation {
            title: "PAUSED",
            subtitle: "FROZEN ANALYSER",
            hint: "PRESS PLAY TO RESUME",
        }),
        TransportState::Error => Some(AnalyserEmptyPresentation {
            title: "CHECK SOURCE",
            subtitle: "CORS OR MEDIA ERROR",
            hint: "NO FAKE ANALYSER MOTION",
        }),
        TransportState::Playing => None,
    }
}
```

- [ ] **Step 4: Render the hardware empty state**

In `render_visualizer`, replace:

```rust
    if let Some(message) = analyser_empty_state_text(state) {
        render_analyser_empty_state(frame, inner, message, state.transport);
        return;
    }
```

with:

```rust
    if let Some(presentation) = analyser_empty_state_presentation(state) {
        render_analyser_empty_state(frame, inner, presentation, state.transport);
        return;
    }
```

Change the signature of `render_analyser_empty_state`:

```rust
fn render_analyser_empty_state(
    frame: &mut Frame<'_>,
    area: Rect,
    presentation: AnalyserEmptyPresentation,
    transport: TransportState,
)
```

Replace the text lines in `render_analyser_empty_state` with:

```rust
    lines.push(Line::from(Span::styled(
        presentation.title,
        Style::default()
            .fg(transport_color(transport))
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        presentation.subtitle,
        classic_small_label_style(),
    )));
    lines.push(Line::from(Span::styled(
        presentation.hint,
        classic_value_style(),
    )));
```

Leave `analyser_empty_state_text` in place until the next cleanup. It remains useful for existing tests and can be removed in a separate refactor if unused by production code.

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```bash
cargo test -p amp808_web analyser_empty_state_presentation_names_idle_paused_and_blocked_hardware_states
cargo test -p amp808_web analyser_empty_state_text_tracks_browser_audio_state
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/web/src/main.rs
git commit -m "feat(web): add hardware analyser empty states"
```

## Task 7: 808-Styled Browser Control Strip

**Files:**
- Modify: `apps/web/index.html`

- [ ] **Step 1: Record the intended browser-control visual contract**

No Rust test covers CSS. Before editing, capture the required visual contract in the commit message body or local notes:

```text
Browser controls remain DOM controls for keyboard and screen-reader access.
The visual treatment should echo the 808 lower button strip:
- dark hardware faceplate background
- black inset URL field
- red/orange/yellow/ivory hardware button colors
- visible focus outlines
- no overlap around 390px mobile width
```

- [ ] **Step 2: Update the CSS controls to match the 808 hardware strip**

In `apps/web/index.html`, replace the `#amp808-controls` rule with:

```css
      #amp808-controls {
        align-items: center;
        background: #eeeadc;
        border: 3px solid #b83d1f;
        border-top-width: 4px;
        box-sizing: border-box;
        color: #090a08;
        display: flex;
        flex: 0 0 auto;
        flex-wrap: wrap;
        gap: 9px;
        padding: 10px 12px;
        width: 100%;
      }
```

Replace the shared control rule with:

```css
      #amp808-controls button,
      #amp808-controls input[type="url"],
      .amp808-file,
      .amp808-motion {
        border: 2px solid #090a08;
        border-radius: 0;
        box-shadow:
          inset 0 2px 0 rgba(255, 255, 255, 0.24),
          0 3px 0 #090a08;
        font: inherit;
        font-weight: 800;
        min-height: 38px;
      }
```

Replace `.amp808-file`, `#amp808-toggle`, `#amp808-load-url`, `.amp808-motion`, and `#amp808-control-status` color rules with:

```css
      .amp808-file {
        background: #eeeadc;
        color: #090a08;
      }

      .amp808-motion {
        align-items: center;
        background: #24251d;
        color: #eeeadc;
        display: inline-flex;
        gap: 7px;
        min-height: 38px;
      }

      #amp808-toggle {
        background: #f05a28;
        color: #090a08;
      }

      #amp808-load-url {
        background: #ffd400;
        color: #090a08;
      }

      #amp808-control-status {
        color: #090a08;
        font-size: 12px;
        font-weight: 800;
        max-width: min(52vw, 640px);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
```

Add this focus rule after the disabled button rule:

```css
      #amp808-controls button:focus-visible,
      #amp808-controls input[type="url"]:focus-visible,
      .amp808-file:focus-visible,
      .amp808-motion:focus-within {
        outline: 3px solid #090a08;
        outline-offset: 2px;
      }
```

- [ ] **Step 3: Build the web app**

Run:

```bash
cd apps/web
NO_COLOR=false trunk build --release --public-url /
```

Expected: PASS and writes `apps/web/dist`.

- [ ] **Step 4: Commit**

```bash
git add apps/web/index.html
git commit -m "feat(web): style browser controls as 808 hardware"
```

## Task 8: Final Verification

**Files:**
- Verify: `apps/web/src/main.rs`
- Verify: `apps/web/index.html`

- [ ] **Step 1: Run Rust formatting**

Run:

```bash
cargo fmt --all --check
```

Expected: PASS.

- [ ] **Step 2: Run workspace tests**

Run:

```bash
cargo test --workspace --locked
```

Expected: PASS.

- [ ] **Step 3: Run wasm check**

Run:

```bash
cargo check --locked -p amp808_web --target wasm32-unknown-unknown
```

Expected: PASS.

- [ ] **Step 4: Run web clippy**

Run:

```bash
cargo clippy -p amp808_web --target wasm32-unknown-unknown -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Run Trunk release build**

Run:

```bash
cd apps/web
NO_COLOR=false trunk build --release --public-url /
```

Expected: PASS.

- [ ] **Step 6: Browser smoke check desktop**

Run:

```bash
cd apps/web
trunk serve --public-url / --port 8080
```

Open `http://127.0.0.1:8080/` and verify:

- dark hardware body is visible with subtle black inset panels
- brand rail reads `Roland Rhythm Composer TR-808 WEB`
- instrument controls look denser and more physical
- step strip uses large colored keycaps
- analyser idle or paused state is deliberate, not a blank void
- DOM controls visually match the hardware strip

- [ ] **Step 7: Browser smoke check mobile width**

At about `390px` wide, verify:

- no horizontal scroll
- no panel overlap
- step keycaps remain legible
- URL field and buttons wrap without clipping
- motion toggle remains reachable

- [ ] **Step 8: Manual playback check**

Use a local audio file and verify:

- play/pause still works
- seek buttons still work
- analyser still moves while playing
- BPM gauge still shows an estimate when enough signal exists
- reduced-motion toggle still disables TachyonFX effects

- [ ] **Step 9: Commit verification notes only if code changed during verification**

If verification caused additional fixes, commit those fixes:

```bash
git add apps/web/src/main.rs apps/web/index.html
git commit -m "fix(web): polish 808 hardware pass"
```

If verification produced no file changes, do not create an empty commit.
