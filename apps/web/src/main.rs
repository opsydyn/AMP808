use std::{cell::RefCell, io, rc::Rc};

use amp808_core::web_audio::{
    analyser_bands_to_heights, analyser_bins_to_bands, BrowserMediaError, WebAudioSource,
    WebBpmDisplayState, WebBpmState, WEB_BPM_MAX, WEB_BPM_MIN,
};
use ratzilla::backend::webgl2::WebGl2BackendOptions;
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    symbols::{border, Marker},
    text::{Line, Span, Text},
    widgets::{
        canvas::{Canvas, Line as CanvasLine},
        Block, Borders, Paragraph,
    },
    Frame, Terminal,
};
use ratzilla::{WebGl2Backend, WebRenderer};
use serde::{Deserialize, Serialize};
use tachyonfx::{fx, CellFilter, EffectRenderer, Interpolation};
use tui_big_text::{BigText, PixelSize};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    window, AnalyserNode, AudioContext, BiquadFilterNode, BiquadFilterType, Document, DragEvent,
    Event, EventTarget, GainNode, HtmlAudioElement, HtmlButtonElement, HtmlElement,
    HtmlInputElement, KeyboardEvent, MediaElementAudioSourceNode, MouseEvent, Url,
};

const BAND_COUNT: usize = 24;
const WEB_SEEK_STEP_SECONDS: f64 = 15.0;
const WAVEFORM_SAMPLE_COUNT: usize = 96;
const WEB_PANE_GAP: u16 = 1;
const INSTRUMENT_CHANNEL_FULL_HEIGHT: u16 = 7;
const RECENT_SOURCE_LIMIT: usize = 4;
const WEB_BPM_DEFAULT_HOP_SECONDS: f64 = 1.0 / 60.0;
const MACHINE_HEADER_HEIGHT: u16 = 8;
const MACHINE_LOGO_WIDTH: u16 = 16;
const MACHINE_LOGO_GUTTER: u16 = 2;
const MACHINE_HEADER_TEXT_MARGIN: u16 = 4;
const WEB_VISUALIZER_BANDS: usize = 10;
const WEB_EQ_BAND_COUNT: usize = 10;
const WEB_EQ_CONTROL_COUNT: usize = WEB_EQ_BAND_COUNT + 2;
const WEB_VOLUME_MIN_DB: f64 = -30.0;
const WEB_VOLUME_MAX_DB: f64 = 6.0;
const WEB_EQ_MIN_DB: f64 = -12.0;
const WEB_EQ_MAX_DB: f64 = 12.0;
const WEB_AUDIO_CONTROL_STEP_DB: f64 = 1.0;
const WEB_808_STEP_COUNT: usize = 16;
const WEB_LOCAL_STORAGE_KEY: &str = "amp808.web.settings.v1";
const WEB_AUDIO_DROP_REJECTION: &str = "Drop an audio file to load it into AMP808 web.";
const WEB_AUDIO_DROP_EXTENSIONS: &[&str] = &[
    ".aac", ".flac", ".m4a", ".mp3", ".oga", ".ogg", ".opus", ".wav", ".webm",
];
const WEB_EQ_Q: f32 = 1.2;
const WEB_EQ_FREQS: [f32; WEB_EQ_BAND_COUNT] = [
    70.0, 180.0, 320.0, 600.0, 1_000.0, 3_000.0, 6_000.0, 12_000.0, 14_000.0, 16_000.0,
];
const LOGO_GLYPH_W: usize = 5;
const LOGO_GLYPH_H: usize = 7;
const LOGO_GLYPH_GAP: usize = 2;
const LOGO_GLYPH_COUNT: usize = 6;
const LOGO_TOTAL_W: usize =
    LOGO_GLYPH_COUNT * LOGO_GLYPH_W + (LOGO_GLYPH_COUNT - 1) * LOGO_GLYPH_GAP;
const LOGO_BAND_MAP: [usize; LOGO_GLYPH_COUNT] = [0, 2, 4, 5, 7, 9];

const LOGO_GLYPHS: [[u8; LOGO_GLYPH_H]; LOGO_GLYPH_COUNT] = [
    [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
    [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
    [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClassicColor {
    red: u8,
    green: u8,
    blue: u8,
}

impl ClassicColor {
    const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    const fn ratatui(self) -> Color {
        Color::Rgb(self.red, self.green, self.blue)
    }
}

struct Classic808Palette;

impl Classic808Palette {
    const FACEPLATE: ClassicColor = ClassicColor::new(0x09, 0x0a, 0x08);
    const BODY: ClassicColor = ClassicColor::new(0x15, 0x17, 0x12);
    const IVORY: ClassicColor = ClassicColor::new(0xee, 0xea, 0xdc);
    const ORANGE: ClassicColor = ClassicColor::new(0xf0, 0x5a, 0x28);
    const BRAND_ORANGE: ClassicColor = ClassicColor::new(0xf0, 0x5a, 0x28);
    const AMBER: ClassicColor = ClassicColor::new(0xf6, 0xa6, 0x23);
    const YELLOW: ClassicColor = ClassicColor::new(0xff, 0xd4, 0x00);
    const RED: ClassicColor = ClassicColor::new(0xd7, 0x26, 0x2e);
    const RED_TEXT: ClassicColor = ClassicColor::new(0xff, 0x5a, 0x45);
    const GREY: ClassicColor = ClassicColor::new(0xc9, 0xc9, 0xc9);
    const DIM: ClassicColor = ClassicColor::new(0x66, 0x66, 0x66);
    const LABEL: ClassicColor = ClassicColor::new(0xa7, 0xaa, 0x7a);
    const OLIVE: ClassicColor = ClassicColor::new(0x48, 0x4b, 0x30);
}

#[cfg(test)]
fn contrast_ratio(foreground: ClassicColor, background: ClassicColor) -> f64 {
    let foreground = relative_luminance(foreground);
    let background = relative_luminance(background);
    let lighter = foreground.max(background);
    let darker = foreground.min(background);
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
fn relative_luminance(color: ClassicColor) -> f64 {
    fn channel(value: u8) -> f64 {
        let value = f64::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassicPadFamily {
    Red,
    Orange,
    Yellow,
    Ivory,
}

fn classic_pad_family(step_index: usize) -> ClassicPadFamily {
    match step_index / 4 {
        0 => ClassicPadFamily::Red,
        1 => ClassicPadFamily::Orange,
        2 => ClassicPadFamily::Yellow,
        _ => ClassicPadFamily::Ivory,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InstrumentControlSpec {
    short_label: &'static str,
    instrument_label: &'static str,
    parameter_label: &'static str,
    family: ClassicPadFamily,
}

fn instrument_control_specs() -> &'static [InstrumentControlSpec; 12] {
    &[
        InstrumentControlSpec {
            short_label: "AC",
            instrument_label: "ACCENT",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "BD",
            instrument_label: "BASS",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "SD",
            instrument_label: "SNARE",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "LT",
            instrument_label: "LOW TOM",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "MT",
            instrument_label: "MID TOM",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "HT",
            instrument_label: "HI TOM",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "CL",
            instrument_label: "CLAVES",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "RS",
            instrument_label: "RIM",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "CP",
            instrument_label: "CLAP",
            parameter_label: "SNAP",
            family: ClassicPadFamily::Yellow,
        },
        InstrumentControlSpec {
            short_label: "CB",
            instrument_label: "COWBELL",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Yellow,
        },
        InstrumentControlSpec {
            short_label: "CY",
            instrument_label: "CYMBAL",
            parameter_label: "DECAY",
            family: ClassicPadFamily::Yellow,
        },
        InstrumentControlSpec {
            short_label: "OH",
            instrument_label: "OPEN HAT",
            parameter_label: "DECAY",
            family: ClassicPadFamily::Ivory,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct WebEqPreset {
    name: &'static str,
    bands: [f64; WEB_EQ_BAND_COUNT],
}

const WEB_EQ_PRESETS: &[WebEqPreset] = &[
    WebEqPreset {
        name: "Flat",
        bands: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    WebEqPreset {
        name: "Rock",
        bands: [5.0, 4.0, 2.0, -1.0, -2.0, 2.0, 4.0, 5.0, 5.0, 5.0],
    },
    WebEqPreset {
        name: "Pop",
        bands: [-1.0, 2.0, 4.0, 5.0, 4.0, 1.0, -1.0, -1.0, 1.0, 2.0],
    },
    WebEqPreset {
        name: "Jazz",
        bands: [3.0, 4.0, 2.0, 1.0, -1.0, -1.0, 1.0, 2.0, 3.0, 4.0],
    },
    WebEqPreset {
        name: "Classical",
        bands: [3.0, 2.0, 1.0, 0.0, -1.0, -1.0, 0.0, 2.0, 3.0, 4.0],
    },
    WebEqPreset {
        name: "Bass Boost",
        bands: [8.0, 6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    WebEqPreset {
        name: "Treble Boost",
        bands: [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 6.0, 7.0],
    },
    WebEqPreset {
        name: "Vocal",
        bands: [-2.0, -1.0, 1.0, 4.0, 5.0, 4.0, 2.0, 0.0, -1.0, -2.0],
    },
    WebEqPreset {
        name: "Electronic",
        bands: [6.0, 4.0, 1.0, -1.0, -2.0, 1.0, 3.0, 4.0, 5.0, 6.0],
    },
    WebEqPreset {
        name: "Acoustic",
        bands: [3.0, 3.0, 2.0, 0.0, 1.0, 2.0, 3.0, 3.0, 2.0, 1.0],
    },
];

#[derive(Debug, Clone, PartialEq)]
struct WebAudioControls {
    volume_db: f64,
    eq_bands: [f64; WEB_EQ_BAND_COUNT],
    preset_index: Option<usize>,
    selected_control: usize,
    control_revision: u64,
}

impl Default for WebAudioControls {
    fn default() -> Self {
        Self {
            volume_db: 0.0,
            eq_bands: WEB_EQ_PRESETS[0].bands,
            preset_index: Some(0),
            selected_control: 0,
            control_revision: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WebAudioControlHitZone {
    index: usize,
    area: Rect,
}

fn web_volume_to_normalized(volume_db: f64) -> f64 {
    normalize_range(volume_db, WEB_VOLUME_MIN_DB, WEB_VOLUME_MAX_DB)
}

fn web_eq_band_to_normalized(gain_db: f64) -> f64 {
    normalize_range(gain_db, WEB_EQ_MIN_DB, WEB_EQ_MAX_DB)
}

fn normalize_range(value: f64, min: f64, max: f64) -> f64 {
    ((value.clamp(min, max) - min) / (max - min)).clamp(0.0, 1.0)
}

fn web_audio_gain_from_db(volume_db: f64) -> f32 {
    10.0_f32.powf((volume_db.clamp(WEB_VOLUME_MIN_DB, WEB_VOLUME_MAX_DB) as f32) / 20.0)
}

fn web_eq_preset_label(controls: &WebAudioControls) -> &'static str {
    controls
        .preset_index
        .and_then(|index| WEB_EQ_PRESETS.get(index))
        .map(|preset| preset.name)
        .unwrap_or("CUSTOM")
}

fn web_audio_control_value(controls: &WebAudioControls, index: usize) -> f64 {
    match index {
        0 => web_volume_to_normalized(controls.volume_db),
        1..=WEB_EQ_BAND_COUNT => web_eq_band_to_normalized(controls.eq_bands[index - 1]),
        _ => controls
            .preset_index
            .map(|index| normalize_range(index as f64, 0.0, (WEB_EQ_PRESETS.len() - 1) as f64))
            .unwrap_or(1.0),
    }
}

fn web_audio_control_hit_zones(
    cells: &[Rect],
    visible_count: usize,
) -> Vec<WebAudioControlHitZone> {
    cells
        .iter()
        .copied()
        .take(visible_count)
        .enumerate()
        .filter(|(_, area)| area.width > 0 && area.height > 0)
        .map(|(index, area)| WebAudioControlHitZone { index, area })
        .collect()
}

fn web_audio_control_at_cell(
    hit_zones: &[WebAudioControlHitZone],
    x: u16,
    y: u16,
) -> Option<usize> {
    hit_zones
        .iter()
        .find(|zone| rect_contains_terminal_cell(zone.area, x, y))
        .map(|zone| zone.index)
}

fn rect_contains_terminal_cell(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

fn web_pointer_cell_from_canvas_offset(
    offset_x: f64,
    offset_y: f64,
    pixel_width: f64,
    pixel_height: f64,
    terminal_area: Rect,
) -> Option<(u16, u16)> {
    if !offset_x.is_finite()
        || !offset_y.is_finite()
        || !pixel_width.is_finite()
        || !pixel_height.is_finite()
        || offset_x < 0.0
        || offset_y < 0.0
        || offset_x >= pixel_width
        || offset_y >= pixel_height
        || pixel_width <= 0.0
        || pixel_height <= 0.0
        || terminal_area.width == 0
        || terminal_area.height == 0
    {
        return None;
    }

    let cell_x = ((offset_x / pixel_width) * f64::from(terminal_area.width)).floor() as u16;
    let cell_y = ((offset_y / pixel_height) * f64::from(terminal_area.height)).floor() as u16;

    Some((
        terminal_area.x.saturating_add(cell_x),
        terminal_area.y.saturating_add(cell_y),
    ))
}

fn web_audio_selected_control_readout(controls: &WebAudioControls) -> String {
    let specs = instrument_control_specs();
    let spec = specs
        .get(controls.selected_control.min(specs.len() - 1))
        .unwrap_or(&specs[0]);
    let mode = web_eq_preset_label(controls);

    match controls.selected_control {
        0 => format!(
            "{} / MASTER VOL / {} / {mode}",
            spec.short_label,
            format_db(controls.volume_db)
        ),
        1..=WEB_EQ_BAND_COUNT => {
            let band_index = controls.selected_control - 1;
            format!(
                "{} / {} / {} / {mode}",
                spec.short_label,
                web_eq_band_label(band_index),
                format_db(controls.eq_bands[band_index])
            )
        }
        _ => format!("{} / SOUND MODE / {mode}", spec.short_label),
    }
}

fn format_db(value: f64) -> String {
    format!("{value:+.0} dB")
}

fn web_audio_control_status(controls: &WebAudioControls) -> String {
    match controls.selected_control {
        0 => format!("Master volume {:+.0} dB", controls.volume_db),
        1..=WEB_EQ_BAND_COUNT => {
            let band_index = controls.selected_control - 1;
            format!(
                "EQ {} {:+.0} dB",
                web_eq_band_label(band_index),
                controls.eq_bands[band_index]
            )
        }
        _ => format!("Sound mode {}", web_eq_preset_label(controls)),
    }
}

fn web_eq_band_label(index: usize) -> &'static str {
    match index {
        0 => "70Hz",
        1 => "180Hz",
        2 => "320Hz",
        3 => "600Hz",
        4 => "1k",
        5 => "3k",
        6 => "6k",
        7 => "12k",
        8 => "14k",
        _ => "16k",
    }
}

fn web_audio_controls_after_action(controls: &mut WebAudioControls, action: WebAction) -> bool {
    let changed = match action {
        WebAction::SelectPreviousAudioControl => {
            controls.selected_control =
                (controls.selected_control + WEB_EQ_CONTROL_COUNT - 1) % WEB_EQ_CONTROL_COUNT;
            true
        }
        WebAction::SelectNextAudioControl => {
            controls.selected_control = (controls.selected_control + 1) % WEB_EQ_CONTROL_COUNT;
            true
        }
        WebAction::AdjustSelectedAudioControlUp => {
            adjust_selected_web_audio_control(controls, WEB_AUDIO_CONTROL_STEP_DB);
            true
        }
        WebAction::AdjustSelectedAudioControlDown => {
            adjust_selected_web_audio_control(controls, -WEB_AUDIO_CONTROL_STEP_DB);
            true
        }
        WebAction::CycleSoundMode => {
            cycle_web_eq_preset(controls, 1);
            true
        }
        WebAction::VolumeUp => {
            controls.volume_db = (controls.volume_db + WEB_AUDIO_CONTROL_STEP_DB)
                .clamp(WEB_VOLUME_MIN_DB, WEB_VOLUME_MAX_DB);
            true
        }
        WebAction::VolumeDown => {
            controls.volume_db = (controls.volume_db - WEB_AUDIO_CONTROL_STEP_DB)
                .clamp(WEB_VOLUME_MIN_DB, WEB_VOLUME_MAX_DB);
            true
        }
        _ => false,
    };

    if changed {
        controls.control_revision = controls.control_revision.wrapping_add(1);
    }

    changed
}

fn adjust_selected_web_audio_control(controls: &mut WebAudioControls, delta_db: f64) {
    match controls.selected_control {
        0 => {
            controls.volume_db =
                (controls.volume_db + delta_db).clamp(WEB_VOLUME_MIN_DB, WEB_VOLUME_MAX_DB);
        }
        1..=WEB_EQ_BAND_COUNT => {
            let band_index = controls.selected_control - 1;
            controls.eq_bands[band_index] =
                (controls.eq_bands[band_index] + delta_db).clamp(WEB_EQ_MIN_DB, WEB_EQ_MAX_DB);
            controls.preset_index = None;
        }
        _ if delta_db >= 0.0 => cycle_web_eq_preset(controls, 1),
        _ => cycle_web_eq_preset(controls, -1),
    }
}

fn cycle_web_eq_preset(controls: &mut WebAudioControls, direction: isize) {
    let len = WEB_EQ_PRESETS.len() as isize;
    let current = controls.preset_index.unwrap_or(0) as isize;
    let next = (current + direction).rem_euclid(len) as usize;
    controls.preset_index = Some(next);
    controls.eq_bands = WEB_EQ_PRESETS[next].bands;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebFocus {
    Transport,
    LocalFile,
    HostedUrl,
    Analyser,
    AudioControls,
    Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebAction {
    TogglePlayback,
    FocusLocalFile,
    FocusHostedUrl,
    FocusAudioControls,
    SelectPreviousAudioControl,
    SelectNextAudioControl,
    AdjustSelectedAudioControlUp,
    AdjustSelectedAudioControlDown,
    CycleSoundMode,
    VolumeUp,
    VolumeDown,
    CycleVisualMode,
    ToggleMotion,
    SeekBack,
    SeekForward,
    ClearFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebVisualMode {
    Bars,
    Wave,
    Retro,
    Logo,
    Split,
}

fn web_action_for_key(key: &str) -> Option<WebAction> {
    match key {
        " " | "Spacebar" | "Space" => Some(WebAction::TogglePlayback),
        "l" | "L" => Some(WebAction::FocusLocalFile),
        "u" | "U" => Some(WebAction::FocusHostedUrl),
        "i" | "I" => Some(WebAction::FocusAudioControls),
        "[" => Some(WebAction::SelectPreviousAudioControl),
        "]" => Some(WebAction::SelectNextAudioControl),
        "ArrowUp" | "k" | "K" => Some(WebAction::AdjustSelectedAudioControlUp),
        "ArrowDown" | "j" | "J" => Some(WebAction::AdjustSelectedAudioControlDown),
        "e" | "E" => Some(WebAction::CycleSoundMode),
        "=" | "+" => Some(WebAction::VolumeUp),
        "-" => Some(WebAction::VolumeDown),
        "v" | "V" => Some(WebAction::CycleVisualMode),
        "m" | "M" => Some(WebAction::ToggleMotion),
        "ArrowLeft" => Some(WebAction::SeekBack),
        "ArrowRight" => Some(WebAction::SeekForward),
        "Escape" => Some(WebAction::ClearFocus),
        _ => None,
    }
}

fn web_visual_mode_after_action(current: WebVisualMode, action: WebAction) -> WebVisualMode {
    if action != WebAction::CycleVisualMode {
        return current;
    }

    match current {
        WebVisualMode::Bars => WebVisualMode::Wave,
        WebVisualMode::Wave => WebVisualMode::Retro,
        WebVisualMode::Retro => WebVisualMode::Logo,
        WebVisualMode::Logo => WebVisualMode::Split,
        WebVisualMode::Split => WebVisualMode::Bars,
    }
}

fn web_visual_mode_label(mode: WebVisualMode) -> &'static str {
    match mode {
        WebVisualMode::Bars => "BARS",
        WebVisualMode::Wave => "WAVE",
        WebVisualMode::Retro => "RETRO",
        WebVisualMode::Logo => "LOGO",
        WebVisualMode::Split => "SPLIT",
    }
}

fn web_focus_after_action(_current: WebFocus, action: WebAction) -> WebFocus {
    match action {
        WebAction::TogglePlayback
        | WebAction::SeekBack
        | WebAction::SeekForward
        | WebAction::ClearFocus => WebFocus::Transport,
        WebAction::FocusLocalFile => WebFocus::LocalFile,
        WebAction::FocusHostedUrl => WebFocus::HostedUrl,
        WebAction::FocusAudioControls
        | WebAction::SelectPreviousAudioControl
        | WebAction::SelectNextAudioControl
        | WebAction::AdjustSelectedAudioControlUp
        | WebAction::AdjustSelectedAudioControlDown
        | WebAction::CycleSoundMode
        | WebAction::VolumeUp
        | WebAction::VolumeDown => WebFocus::AudioControls,
        WebAction::CycleVisualMode => WebFocus::Analyser,
        WebAction::ToggleMotion => WebFocus::Motion,
    }
}

fn web_seek_target_seconds(current_time: f64, duration: Option<f64>, delta_seconds: f64) -> f64 {
    let current_time = if current_time.is_finite() {
        current_time.max(0.0)
    } else {
        0.0
    };
    let target = (current_time + delta_seconds).max(0.0);
    duration
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .map(|duration| target.min(duration))
        .unwrap_or(target)
}

fn analyser_bands_for_scope_width(bands: &[f32], width: u16) -> Vec<f32> {
    if bands.is_empty() || width < 2 {
        return Vec::new();
    }

    let slots = (usize::from(width) / 2).max(1);
    resample_f32(bands, slots)
}

fn waveform_bytes_to_samples(bytes: &[u8], sample_count: usize) -> Vec<f32> {
    if sample_count == 0 {
        return Vec::new();
    }
    if bytes.is_empty() {
        return vec![0.0; sample_count];
    }

    let samples = bytes
        .iter()
        .map(|byte| (f32::from(*byte) - 128.0) / 128.0)
        .map(|sample| sample.clamp(-1.0, 1.0))
        .collect::<Vec<_>>();
    resample_f32(&samples, sample_count)
}

#[derive(Debug, Clone, PartialEq)]
struct WebTempoDisplay {
    normalized: f64,
    label: String,
}

fn bpm_to_normalized(bpm: u16) -> f64 {
    ((f64::from(bpm) - WEB_BPM_MIN) / (WEB_BPM_MAX - WEB_BPM_MIN)).clamp(0.0, 1.0)
}

fn web_tempo_display(state: &WebBpmState) -> WebTempoDisplay {
    match state.display_state() {
        WebBpmDisplayState::Locked(bpm) => WebTempoDisplay {
            normalized: bpm_to_normalized(bpm),
            label: bpm.to_string(),
        },
        WebBpmDisplayState::Estimating if state.provisional_bpm().is_some() => {
            let bpm = state.provisional_bpm().unwrap();
            WebTempoDisplay {
                normalized: bpm_to_normalized(bpm),
                label: format!("~{bpm}"),
            }
        }
        WebBpmDisplayState::Estimating => WebTempoDisplay {
            normalized: 0.5,
            label: "EST".to_string(),
        },
        WebBpmDisplayState::Unavailable => WebTempoDisplay {
            normalized: 0.5,
            label: "--".to_string(),
        },
    }
}

fn resample_f32(values: &[f32], target_len: usize) -> Vec<f32> {
    if values.is_empty() || target_len == 0 {
        return Vec::new();
    }
    if target_len == 1 {
        return vec![values[0]];
    }

    let last = values.len().saturating_sub(1) as f64;
    (0..target_len)
        .map(|index| {
            let source = ((index as f64 / (target_len - 1) as f64) * last).round() as usize;
            values[source.min(values.len() - 1)]
        })
        .collect()
}

fn web_motion_enabled_after_action(current: bool, action: WebAction) -> bool {
    if action == WebAction::ToggleMotion {
        !current
    } else {
        current
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelRole {
    Transport,
    Instrument,
    Analyser,
    Steps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelState {
    Idle,
    Armed,
    Active,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WebPanelSpec {
    role: PanelRole,
    title: &'static str,
    state: PanelState,
    lamp: Option<PanelState>,
}

fn web_panel_spec(role: PanelRole, state: &WebAppState) -> WebPanelSpec {
    let focused = matches!(
        (role, state.focus),
        (
            PanelRole::Transport,
            WebFocus::Transport | WebFocus::LocalFile | WebFocus::HostedUrl
        ) | (PanelRole::Instrument, WebFocus::AudioControls)
            | (PanelRole::Analyser, WebFocus::Analyser)
    );
    let panel_state = match (role, state.transport, focused) {
        (_, TransportState::Error, _) => PanelState::Error,
        (_, _, true) => PanelState::Active,
        (PanelRole::Transport, TransportState::Playing, _) => PanelState::Active,
        (PanelRole::Transport, TransportState::Ready | TransportState::Paused, _) => {
            PanelState::Armed
        }
        (PanelRole::Analyser | PanelRole::Steps, TransportState::Playing, _) => PanelState::Active,
        (
            PanelRole::Analyser | PanelRole::Steps,
            TransportState::Ready | TransportState::Paused,
            _,
        ) => PanelState::Armed,
        (PanelRole::Instrument, TransportState::Idle | TransportState::Ended, _) => {
            PanelState::Idle
        }
        (PanelRole::Instrument, _, _) => PanelState::Armed,
        _ => PanelState::Idle,
    };

    WebPanelSpec {
        role,
        title: match role {
            PanelRole::Transport => " BASIC RHYTHM ",
            PanelRole::Instrument => " INSTRUMENT SELECT / LEVEL ",
            PanelRole::Analyser => " SCOPE / ANALYSER ",
            PanelRole::Steps => " BASIC RHYTHM STEP BUTTONS ",
        },
        state: panel_state,
        lamp: matches!(
            role,
            PanelRole::Transport | PanelRole::Analyser | PanelRole::Steps
        )
        .then_some(panel_state),
    }
}

struct WebPanelFx {
    cycle_ms: u32,
    effect: tachyonfx::Effect,
}

#[derive(Default)]
struct WebFxRuntime {
    last_frame_ms: Option<f64>,
    frame_counter: u64,
    last_transition_transport: Option<TransportState>,
    last_audio_control_revision: u64,
    transition_panel: Option<WebPanelFx>,
    audio_control_panel: Option<WebPanelFx>,
    header_panel: Option<WebPanelFx>,
    header_identity_panel: Option<WebPanelFx>,
    transport_panel: Option<WebPanelFx>,
    instrument_panel: Option<WebPanelFx>,
    analyser_panel: Option<WebPanelFx>,
    steps_panel: Option<WebPanelFx>,
}

impl WebFxRuntime {
    fn next_tick(&mut self, now_ms: f64) -> tachyonfx::Duration {
        let tick_ms = self
            .last_frame_ms
            .map(|last_frame_ms| web_fx_tick_ms(now_ms - last_frame_ms))
            .unwrap_or(16);
        self.last_frame_ms = Some(now_ms);
        self.frame_counter = self.frame_counter.wrapping_add(1);
        tachyonfx::Duration::from_millis(tick_ms)
    }

    fn visual_frame(&self) -> u64 {
        self.frame_counter
    }

    fn panel_effect(&mut self, spec: WebPanelSpec) -> Option<&mut tachyonfx::Effect> {
        let Some(cycle_ms) = web_panel_fx_signature(spec.state) else {
            self.clear_panel(spec.role);
            return None;
        };
        let slot = self.panel_slot(spec.role);
        let should_rebuild = slot
            .as_ref()
            .is_none_or(|panel_fx| panel_fx.cycle_ms != cycle_ms);
        if should_rebuild {
            *slot = Some(WebPanelFx {
                cycle_ms,
                effect: make_web_panel_trace_effect(spec.state),
            });
        }
        slot.as_mut().map(|panel_fx| &mut panel_fx.effect)
    }

    fn header_effect(&mut self, transport: TransportState) -> Option<&mut tachyonfx::Effect> {
        let Some(cycle_ms) = web_header_fx_signature(transport) else {
            self.header_panel = None;
            return None;
        };
        let should_rebuild = self
            .header_panel
            .as_ref()
            .is_none_or(|panel_fx| panel_fx.cycle_ms != cycle_ms);
        if should_rebuild {
            self.header_panel = Some(WebPanelFx {
                cycle_ms,
                effect: make_web_header_effect(transport),
            });
        }
        self.header_panel
            .as_mut()
            .map(|panel_fx| &mut panel_fx.effect)
    }

    fn header_identity_effect(
        &mut self,
        transport: TransportState,
    ) -> Option<&mut tachyonfx::Effect> {
        let Some(cycle_ms) = web_header_identity_fx_signature(transport) else {
            self.header_identity_panel = None;
            return None;
        };
        let should_rebuild = self
            .header_identity_panel
            .as_ref()
            .is_none_or(|panel_fx| panel_fx.cycle_ms != cycle_ms);
        if should_rebuild {
            self.header_identity_panel = Some(WebPanelFx {
                cycle_ms,
                effect: make_web_header_identity_effect(transport),
            });
        }
        self.header_identity_panel
            .as_mut()
            .map(|panel_fx| &mut panel_fx.effect)
    }

    fn transition_effect(&mut self, transport: TransportState) -> Option<&mut tachyonfx::Effect> {
        if self.last_transition_transport == Some(transport) {
            return self
                .transition_panel
                .as_mut()
                .map(|panel_fx| &mut panel_fx.effect);
        }
        self.last_transition_transport = Some(transport);

        let Some(cycle_ms) = web_transition_fx_signature(transport) else {
            self.transition_panel = None;
            return None;
        };

        self.transition_panel = Some(WebPanelFx {
            cycle_ms,
            effect: make_web_transition_effect(transport),
        });
        self.transition_panel
            .as_mut()
            .map(|panel_fx| &mut panel_fx.effect)
    }

    fn audio_control_effect(
        &mut self,
        controls: &WebAudioControls,
    ) -> Option<&mut tachyonfx::Effect> {
        let Some(cycle_ms) = web_audio_control_fx_signature(controls.control_revision) else {
            self.last_audio_control_revision = controls.control_revision;
            self.audio_control_panel = None;
            return None;
        };

        if self.last_audio_control_revision != controls.control_revision {
            self.last_audio_control_revision = controls.control_revision;
            self.audio_control_panel = Some(WebPanelFx {
                cycle_ms,
                effect: make_web_audio_control_pulse_effect(),
            });
        }

        self.audio_control_panel
            .as_mut()
            .map(|panel_fx| &mut panel_fx.effect)
    }

    fn clear_panel(&mut self, role: PanelRole) {
        *self.panel_slot(role) = None;
    }

    fn clear_effects(&mut self) {
        self.last_transition_transport = None;
        self.last_audio_control_revision = 0;
        self.transition_panel = None;
        self.audio_control_panel = None;
        self.header_panel = None;
        self.header_identity_panel = None;
        self.transport_panel = None;
        self.instrument_panel = None;
        self.analyser_panel = None;
        self.steps_panel = None;
    }

    fn panel_slot(&mut self, role: PanelRole) -> &mut Option<WebPanelFx> {
        match role {
            PanelRole::Transport => &mut self.transport_panel,
            PanelRole::Instrument => &mut self.instrument_panel,
            PanelRole::Analyser => &mut self.analyser_panel,
            PanelRole::Steps => &mut self.steps_panel,
        }
    }
}

fn web_panel_fx_signature(state: PanelState) -> Option<u32> {
    match state {
        PanelState::Active => Some(1800),
        PanelState::Error => Some(900),
        PanelState::Idle | PanelState::Armed => None,
    }
}

fn web_fx_tick_ms(delta_ms: f64) -> u32 {
    if !delta_ms.is_finite() {
        return 16;
    }
    delta_ms.round().clamp(12.0, 80.0) as u32
}

fn web_transition_fx_signature(transport: TransportState) -> Option<u32> {
    match transport {
        TransportState::Ready => Some(360),
        TransportState::Playing => Some(320),
        TransportState::Error => Some(520),
        TransportState::Idle | TransportState::Paused | TransportState::Ended => None,
    }
}

fn web_header_fx_signature(transport: TransportState) -> Option<u32> {
    match transport {
        TransportState::Playing => Some(1400),
        TransportState::Error => Some(600),
        TransportState::Idle
        | TransportState::Ready
        | TransportState::Paused
        | TransportState::Ended => None,
    }
}

fn web_header_identity_fx_signature(transport: TransportState) -> Option<u32> {
    match transport {
        TransportState::Playing => Some(2200),
        TransportState::Idle
        | TransportState::Ready
        | TransportState::Paused
        | TransportState::Ended
        | TransportState::Error => None,
    }
}

fn web_audio_control_fx_signature(control_revision: u64) -> Option<u32> {
    (control_revision > 0).then_some(420)
}

fn make_web_transition_effect(transport: TransportState) -> tachyonfx::Effect {
    let cycle_ms = web_transition_fx_signature(transport).unwrap_or(320);
    let flash = WebTransitionFlash {
        accent: match transport {
            TransportState::Ready => Classic808Palette::AMBER.ratatui(),
            TransportState::Playing => Classic808Palette::YELLOW.ratatui(),
            TransportState::Error => Classic808Palette::RED_TEXT.ratatui(),
            TransportState::Idle | TransportState::Paused | TransportState::Ended => {
                Classic808Palette::IVORY.ratatui()
            }
        },
    };

    fx::run_once(fx::effect_fn(
        flash,
        (cycle_ms, Interpolation::SineOut),
        |state, ctx, cells| {
            let intensity = 1.0 - ctx.alpha();
            cells.for_each_cell(|_pos, cell| {
                if cell.symbol().trim().is_empty() {
                    return;
                }
                cell.set_fg(mix_rgb(cell.fg, state.accent, intensity * 0.58));
            });
        },
    ))
}

#[derive(Clone, Copy)]
struct WebTransitionFlash {
    accent: Color,
}

fn make_web_header_effect(transport: TransportState) -> tachyonfx::Effect {
    let cycle_ms = web_header_fx_signature(transport).unwrap_or(1400);
    let trace = match transport {
        TransportState::Error => WebHeaderTrace {
            dim: Classic808Palette::RED_TEXT.ratatui(),
            accent: Classic808Palette::YELLOW.ratatui(),
            warm: Classic808Palette::RED.ratatui(),
        },
        _ => WebHeaderTrace {
            dim: Classic808Palette::DIM.ratatui(),
            accent: Classic808Palette::YELLOW.ratatui(),
            warm: Classic808Palette::ORANGE.ratatui(),
        },
    };

    fx::repeating(fx::effect_fn(
        trace,
        (cycle_ms, Interpolation::SineInOut),
        |state, ctx, cells| {
            let width = ctx.area.width.max(1) as f32;
            cells.for_each_cell(|pos, cell| {
                if cell.symbol().trim().is_empty() {
                    return;
                }

                let offset = pos.x.saturating_sub(ctx.area.x) as f32 / width;
                let wave = (ctx.alpha() * std::f32::consts::TAU + offset * 6.0).sin() * 0.5 + 0.5;
                let base = cell.fg;
                let glow = mix_rgb(state.warm, state.accent, wave);
                let lift = if base == state.dim { 0.28 } else { 0.14 };
                cell.set_fg(mix_rgb(base, glow, lift));
            });
        },
    ))
}

#[derive(Clone, Copy)]
struct WebHeaderTrace {
    dim: Color,
    accent: Color,
    warm: Color,
}

fn make_web_header_identity_effect(transport: TransportState) -> tachyonfx::Effect {
    let cycle_ms = web_header_identity_fx_signature(transport).unwrap_or(2200);
    fx::repeating(fx::ping_pong(
        fx::hsl_shift_fg([0.0, 8.0, 14.0], (cycle_ms / 2, Interpolation::SineInOut))
            .with_filter(CellFilter::NonEmpty),
    ))
}

fn make_web_audio_control_pulse_effect() -> tachyonfx::Effect {
    fx::run_once(fx::effect_fn(
        WebAudioControlPulse {
            accent: Classic808Palette::YELLOW.ratatui(),
            warm: Classic808Palette::ORANGE.ratatui(),
        },
        (420, Interpolation::CircOut),
        |state, ctx, cells| {
            let intensity = 1.0 - ctx.alpha();
            cells.for_each_cell(|_pos, cell| {
                if cell.symbol().trim().is_empty() {
                    return;
                }
                let glow = mix_rgb(state.warm, state.accent, intensity);
                cell.set_fg(mix_rgb(cell.fg, glow, 0.22 + intensity * 0.5));
                cell.set_bg(mix_rgb(
                    cell.bg,
                    Classic808Palette::OLIVE.ratatui(),
                    intensity * 0.2,
                ));
            });
        },
    ))
}

#[derive(Clone, Copy)]
struct WebAudioControlPulse {
    accent: Color,
    warm: Color,
}

fn make_web_panel_trace_effect(state: PanelState) -> tachyonfx::Effect {
    let cycle_ms = web_panel_fx_signature(state).unwrap_or(900);
    let trace = match state {
        PanelState::Error => WebPanelTrace {
            base: Classic808Palette::RED_TEXT.ratatui(),
            head: Classic808Palette::YELLOW.ratatui(),
            tail: Classic808Palette::RED.ratatui(),
        },
        _ => WebPanelTrace {
            base: Classic808Palette::YELLOW.ratatui(),
            head: Classic808Palette::IVORY.ratatui(),
            tail: Classic808Palette::AMBER.ratatui(),
        },
    };

    fx::repeating(fx::effect_fn(
        trace,
        (cycle_ms, Interpolation::Linear),
        |state, ctx, cells| {
            if ctx.area.width < 2 || ctx.area.height < 2 {
                return;
            }

            let perimeter = panel_perimeter_len(ctx.area);
            if perimeter == 0 {
                return;
            }

            let head = ctx.alpha() * perimeter as f32;
            let tail_len = (perimeter as f32 * 0.18).max(5.0);
            cells.for_each_cell(|pos, cell| {
                let Some(index) = panel_perimeter_index(ctx.area, pos) else {
                    return;
                };

                let distance = (head - index as f32).rem_euclid(perimeter as f32);
                if distance <= tail_len {
                    let hotspot = (1.0 - distance / tail_len).powf(1.8);
                    let accent = mix_rgb(state.tail, state.head, hotspot);
                    cell.set_fg(mix_rgb(state.base, accent, 0.32 + hotspot * 0.48));
                }
            });
        },
    ))
}

#[derive(Clone, Copy)]
struct WebPanelTrace {
    base: Color,
    head: Color,
    tail: Color,
}

fn panel_perimeter_len(area: Rect) -> u16 {
    area.width
        .saturating_mul(2)
        .saturating_add(area.height.saturating_mul(2))
        .saturating_sub(4)
}

fn panel_perimeter_index(area: Rect, pos: Position) -> Option<u16> {
    if area.width < 2 || area.height < 2 {
        return None;
    }

    let left = area.x;
    let right = area.right().saturating_sub(1);
    let top = area.y;
    let bottom = area.bottom().saturating_sub(1);

    if pos.y == top && pos.x >= left && pos.x <= right {
        Some(pos.x - left)
    } else if pos.x == right && pos.y > top && pos.y <= bottom {
        Some(area.width + (pos.y - top - 1))
    } else if pos.y == bottom && pos.x >= left && pos.x < right {
        Some(area.width + (area.height - 1) + (right - pos.x - 1))
    } else if pos.x == left && pos.y > top && pos.y < bottom {
        Some(area.width + (area.height - 1) + (area.width - 1) + (bottom - pos.y - 1))
    } else {
        None
    }
}

fn mix_rgb(a: Color, b: Color, ratio_to_b: f32) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let t = ratio_to_b.clamp(0.0, 1.0);
            let lerp = |lhs: u8, rhs: u8| -> u8 {
                ((lhs as f32 * (1.0 - t)) + (rhs as f32 * t)).round() as u8
            };
            Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
        }
        _ => a,
    }
}

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
            hint: "LOCAL FILE, DROP, OR CORS URL",
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

#[cfg(test)]
fn analyser_empty_state_text(state: &WebAppState) -> Option<&'static str> {
    match state.transport {
        TransportState::Idle | TransportState::Ended => Some("LOAD AUDIO OR CORS URL"),
        TransportState::Ready => Some("READY - PRESS PLAY"),
        TransportState::Paused => Some("PAUSED"),
        TransportState::Error => Some("CHECK SOURCE / CORS"),
        TransportState::Playing => None,
    }
}

fn browser_media_error_message(source: Option<&WebAudioSource>, error_code: Option<u16>) -> String {
    let is_hosted_url = source.is_some_and(WebAudioSource::is_hosted_url);
    error_code
        .map(BrowserMediaError::from_code)
        .unwrap_or(BrowserMediaError::Unknown)
        .user_message(is_hosted_url)
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransportState {
    Idle,
    Ready,
    Playing,
    Paused,
    Ended,
    Error,
}

impl TransportState {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Ready => "READY",
            Self::Playing => "PLAYING",
            Self::Paused => "PAUSED",
            Self::Ended => "ENDED",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone)]
struct WebAppState {
    source: Option<WebAudioSource>,
    transport: TransportState,
    focus: WebFocus,
    status: String,
    error: Option<String>,
    current_time: f64,
    duration: Option<f64>,
    bands: Vec<f32>,
    waveform: Vec<f32>,
    bpm: WebBpmState,
    last_bpm_sample_ms: Option<f64>,
    visual_mode: WebVisualMode,
    audio_controls: WebAudioControls,
    terminal_area: Rect,
    audio_control_hit_zones: Vec<WebAudioControlHitZone>,
    motion_enabled: bool,
    recent_sources: Vec<WebAudioSource>,
}

impl Default for WebAppState {
    fn default() -> Self {
        Self {
            source: None,
            transport: TransportState::Idle,
            focus: WebFocus::Transport,
            status: "Load a local audio file or a CORS-enabled hosted URL".to_string(),
            error: None,
            current_time: 0.0,
            duration: None,
            bands: vec![0.0; BAND_COUNT],
            waveform: vec![0.0; WAVEFORM_SAMPLE_COUNT],
            bpm: WebBpmState::unavailable(),
            last_bpm_sample_ms: None,
            visual_mode: WebVisualMode::Split,
            audio_controls: WebAudioControls::default(),
            terminal_area: Rect::new(0, 0, 0, 0),
            audio_control_hit_zones: Vec::new(),
            motion_enabled: true,
            recent_sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct WebPersistedSettings {
    visual_mode: String,
    motion_enabled: bool,
    volume_db: f64,
    eq_bands: [f64; WEB_EQ_BAND_COUNT],
    preset_index: Option<usize>,
    recent_hosted_urls: Vec<String>,
}

fn web_persisted_settings_from_state(state: &WebAppState) -> WebPersistedSettings {
    WebPersistedSettings {
        visual_mode: web_visual_mode_storage_value(state.visual_mode).to_string(),
        motion_enabled: state.motion_enabled,
        volume_db: state.audio_controls.volume_db,
        eq_bands: state.audio_controls.eq_bands,
        preset_index: state.audio_controls.preset_index,
        recent_hosted_urls: hosted_recent_urls(&state.recent_sources),
    }
}

fn web_persisted_settings_to_json(
    settings: &WebPersistedSettings,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(settings)
}

fn web_persisted_settings_from_json(json: &str) -> Option<WebPersistedSettings> {
    serde_json::from_str(json).ok()
}

fn web_restore_persisted_settings(state: &mut WebAppState, settings: WebPersistedSettings) {
    if let Some(mode) = web_visual_mode_from_storage_value(&settings.visual_mode) {
        state.visual_mode = mode;
    }
    state.motion_enabled = settings.motion_enabled;
    state.audio_controls.volume_db = settings
        .volume_db
        .clamp(WEB_VOLUME_MIN_DB, WEB_VOLUME_MAX_DB);
    state.audio_controls.eq_bands = settings
        .eq_bands
        .map(|band| band.clamp(WEB_EQ_MIN_DB, WEB_EQ_MAX_DB));
    state.audio_controls.preset_index = settings
        .preset_index
        .filter(|index| WEB_EQ_PRESETS.get(*index).is_some());
    if state.audio_controls.preset_index.is_none() {
        state.audio_controls.selected_control = 0;
    }
    state.audio_controls.selected_control = state
        .audio_controls
        .selected_control
        .min(WEB_EQ_CONTROL_COUNT - 1);
    state.audio_controls.control_revision = 0;
    state.recent_sources.clear();
    for url in settings
        .recent_hosted_urls
        .into_iter()
        .rev()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
    {
        remember_recent_source(&mut state.recent_sources, WebAudioSource::hosted_url(url));
    }
}

fn web_visual_mode_storage_value(mode: WebVisualMode) -> &'static str {
    match mode {
        WebVisualMode::Bars => "bars",
        WebVisualMode::Wave => "wave",
        WebVisualMode::Retro => "retro",
        WebVisualMode::Logo => "logo",
        WebVisualMode::Split => "split",
    }
}

fn web_visual_mode_from_storage_value(value: &str) -> Option<WebVisualMode> {
    match value {
        "bars" => Some(WebVisualMode::Bars),
        "wave" => Some(WebVisualMode::Wave),
        "retro" => Some(WebVisualMode::Retro),
        "logo" => Some(WebVisualMode::Logo),
        "split" => Some(WebVisualMode::Split),
        _ => None,
    }
}

fn load_web_settings_from_storage(state: &mut WebAppState) {
    let Some(storage) = web_local_storage() else {
        return;
    };
    let Ok(Some(json)) = storage.get_item(WEB_LOCAL_STORAGE_KEY) else {
        return;
    };
    if let Some(settings) = web_persisted_settings_from_json(&json) {
        web_restore_persisted_settings(state, settings);
    }
}

fn persist_web_settings_to_storage(state: &WebAppState) {
    let Some(storage) = web_local_storage() else {
        return;
    };
    let settings = web_persisted_settings_from_state(state);
    let Ok(json) = web_persisted_settings_to_json(&settings) else {
        return;
    };
    let _ = storage.set_item(WEB_LOCAL_STORAGE_KEY, &json);
}

fn web_local_storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

fn remember_recent_source(recent_sources: &mut Vec<WebAudioSource>, source: WebAudioSource) {
    recent_sources.retain(|existing| existing != &source);
    recent_sources.insert(0, source);
    recent_sources.truncate(RECENT_SOURCE_LIMIT);
}

fn hosted_recent_urls(recent_sources: &[WebAudioSource]) -> Vec<String> {
    recent_sources
        .iter()
        .filter_map(|source| match source {
            WebAudioSource::HostedUrl { url } => Some(url.clone()),
            WebAudioSource::LocalFile { .. } => None,
        })
        .collect()
}

fn web_audio_drop_error(file_name: &str, media_type: &str) -> Option<&'static str> {
    let media_type = media_type.trim().to_ascii_lowercase();
    if media_type.starts_with("audio/") {
        return None;
    }

    let file_name = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file_name)
        .to_ascii_lowercase();
    if WEB_AUDIO_DROP_EXTENSIONS
        .iter()
        .any(|extension| file_name.ends_with(extension))
    {
        None
    } else {
        Some(WEB_AUDIO_DROP_REJECTION)
    }
}

fn recent_source_display_label(source: &WebAudioSource, max_width: usize) -> String {
    let prefix = if source.is_hosted_url() { "U " } else { "F " };
    let max_width = max_width.max(prefix.len());
    let label_width = max_width.saturating_sub(prefix.len());
    let label = source.label();
    let mut clipped = label.chars().take(label_width).collect::<String>();
    if label.chars().count() > label_width && label_width >= 1 {
        clipped.pop();
        clipped.push('~');
    }
    format!("{prefix}{clipped}")
}

fn sync_recent_url_options(
    document: &Document,
    list: &HtmlElement,
    recent_sources: &[WebAudioSource],
) {
    while let Some(child) = list.first_child() {
        let _ = list.remove_child(&child);
    }

    for url in hosted_recent_urls(recent_sources) {
        if let Ok(option) = document.create_element("option") {
            let _ = option.set_attribute("value", &url);
            option.set_text_content(Some(&url));
            let _ = list.append_child(&option);
        }
    }
}

struct AudioGraph {
    audio: HtmlAudioElement,
    context: AudioContext,
    _source: MediaElementAudioSourceNode,
    eq_filters: Vec<BiquadFilterNode>,
    gain: GainNode,
    analyser: AnalyserNode,
}

#[derive(Clone)]
struct WebKeyboardControls {
    toggle_button: HtmlButtonElement,
    control_status: HtmlElement,
    file_input: HtmlInputElement,
    url_input: HtmlInputElement,
    motion_input: HtmlInputElement,
}

fn main() -> io::Result<()> {
    let backend = WebGl2Backend::new_with_options(WebGl2BackendOptions::new().grid_id("app"))?;
    let terminal = Terminal::new(backend)?;
    let mut initial_state = WebAppState::default();
    load_web_settings_from_storage(&mut initial_state);
    let state = Rc::new(RefCell::new(initial_state));
    let fx_runtime = Rc::new(RefCell::new(WebFxRuntime::default()));
    let graph = install_audio_graph(Rc::clone(&state)).map_err(js_to_io_error)?;

    terminal.draw_web(move |frame| {
        sample_analyser(&graph, &state);
        let mut snapshot = state.borrow().clone();
        let mut fx_runtime = fx_runtime.borrow_mut();
        render_web_808(frame, &mut snapshot, &mut fx_runtime);
        {
            let mut state = state.borrow_mut();
            state.terminal_area = snapshot.terminal_area;
            state.audio_control_hit_zones = snapshot.audio_control_hit_zones;
        }
    });

    Ok(())
}

fn install_audio_graph(state: Rc<RefCell<WebAppState>>) -> Result<Rc<AudioGraph>, JsValue> {
    let document = window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document is not available"))?;
    let audio = HtmlAudioElement::new()?;
    audio.set_preload("metadata");

    let context = AudioContext::new()?;
    let source = context.create_media_element_source(&audio)?;
    let analyser = context.create_analyser()?;
    analyser.set_fft_size(1024);
    analyser.set_smoothing_time_constant(0.78);

    let mut eq_filters: Vec<BiquadFilterNode> = Vec::with_capacity(WEB_EQ_BAND_COUNT);
    for (index, frequency) in WEB_EQ_FREQS.iter().enumerate() {
        let filter = context.create_biquad_filter()?;
        filter.set_type(BiquadFilterType::Peaking);
        filter.frequency().set_value(*frequency);
        filter.q().set_value(WEB_EQ_Q);
        filter.gain().set_value(0.0);

        if index == 0 {
            source.connect_with_audio_node(&filter)?;
        } else if let Some(previous) = eq_filters.get(index - 1) {
            previous.connect_with_audio_node(&filter)?;
        }

        eq_filters.push(filter);
    }

    let gain = context.create_gain()?;
    if let Some(last_filter) = eq_filters.last() {
        last_filter.connect_with_audio_node(&gain)?;
    }
    gain.connect_with_audio_node(&analyser)?;
    analyser.connect_with_audio_node(&context.destination())?;

    let graph = Rc::new(AudioGraph {
        audio,
        context,
        _source: source,
        eq_filters,
        gain,
        analyser,
    });

    apply_web_audio_controls(&graph, &state.borrow().audio_controls);
    wire_controls(&document, Rc::clone(&graph), state)?;
    Ok(graph)
}

fn apply_web_audio_controls(graph: &AudioGraph, controls: &WebAudioControls) {
    graph
        .gain
        .gain()
        .set_value(web_audio_gain_from_db(controls.volume_db));

    for (filter, gain_db) in graph.eq_filters.iter().zip(controls.eq_bands) {
        filter.gain().set_value(gain_db as f32);
    }
}

fn wire_controls(
    document: &Document,
    graph: Rc<AudioGraph>,
    state: Rc<RefCell<WebAppState>>,
) -> Result<(), JsValue> {
    let file_input: HtmlInputElement = element_by_id(document, "amp808-file")?;
    let url_input: HtmlInputElement = element_by_id(document, "amp808-url")?;
    let toggle_button: HtmlButtonElement = element_by_id(document, "amp808-toggle")?;
    let seek_back_button: HtmlButtonElement = element_by_id(document, "amp808-seek-back")?;
    let seek_forward_button: HtmlButtonElement = element_by_id(document, "amp808-seek-forward")?;
    let load_url_button: HtmlButtonElement = element_by_id(document, "amp808-load-url")?;
    let motion_input: HtmlInputElement = element_by_id(document, "amp808-motion")?;
    let control_status: HtmlElement = element_by_id(document, "amp808-control-status")?;
    let recent_url_list: HtmlElement = element_by_id(document, "amp808-recent-urls")?;
    let app_root: HtmlElement = element_by_id(document, "app")?;
    let object_url: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    {
        let state_ref = state.borrow();
        motion_input.set_checked(state_ref.motion_enabled);
        sync_recent_url_options(document, &recent_url_list, &state_ref.recent_sources);
        sync_controls(&toggle_button, &control_status, &state_ref);
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        let object_url = Rc::clone(&object_url);
        let file_input = file_input.clone();
        add_event_listener(file_input.clone().as_ref(), "change", move |_| {
            let Some(files) = file_input.files() else {
                return;
            };
            let Some(file) = files.get(0) else {
                return;
            };

            load_browser_local_file(
                file,
                &graph,
                &state,
                &object_url,
                &toggle_button,
                &control_status,
            );
        })?;
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        let object_url = Rc::clone(&object_url);
        let url_input = url_input.clone();
        let recent_url_list = recent_url_list.clone();
        let document = document.clone();
        add_event_listener(load_url_button.as_ref(), "click", move |_| {
            let url = url_input.value().trim().to_string();
            if url.is_empty() {
                set_error(
                    &state,
                    &toggle_button,
                    &control_status,
                    "Enter a hosted audio URL first.".to_string(),
                );
                return;
            }

            revoke_object_url(&object_url);
            graph.audio.set_cross_origin(Some("anonymous"));
            graph.audio.set_src(&url);
            graph.audio.load();

            {
                let mut state = state.borrow_mut();
                let source = WebAudioSource::hosted_url(url);
                state.source = Some(source.clone());
                remember_recent_source(&mut state.recent_sources, source);
                state.transport = TransportState::Ready;
                state.status =
                    "Hosted audio loaded; CORS must allow AMP808 web playback.".to_string();
                state.error = None;
                state.current_time = 0.0;
                state.duration = None;
                state.bands = vec![0.0; BAND_COUNT];
                state.waveform = vec![0.0; WAVEFORM_SAMPLE_COUNT];
                state.bpm = WebBpmState::estimating();
                state.last_bpm_sample_ms = None;
                sync_recent_url_options(&document, &recent_url_list, &state.recent_sources);
                persist_web_settings_to_storage(&state);
            }
            sync_controls(&toggle_button, &control_status, &state.borrow());
        })?;
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(toggle_button.clone().as_ref(), "click", move |_| {
            toggle_browser_playback(
                Rc::clone(&graph),
                Rc::clone(&state),
                toggle_button.clone(),
                control_status.clone(),
            );
        })?;
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(seek_back_button.as_ref(), "click", move |_| {
            seek_browser_audio(
                &graph,
                &state,
                -WEB_SEEK_STEP_SECONDS,
                &toggle_button,
                &control_status,
            );
        })?;
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(seek_forward_button.as_ref(), "click", move |_| {
            seek_browser_audio(
                &graph,
                &state,
                WEB_SEEK_STEP_SECONDS,
                &toggle_button,
                &control_status,
            );
        })?;
    }

    {
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        let motion_input = motion_input.clone();
        add_event_listener(motion_input.clone().as_ref(), "change", move |_| {
            {
                let mut state = state.borrow_mut();
                state.motion_enabled = motion_input.checked();
                state.focus = WebFocus::Motion;
                state.status = motion_status_text(state.motion_enabled).to_string();
                persist_web_settings_to_storage(&state);
            }
            let state_ref = state.borrow();
            sync_controls(&toggle_button, &control_status, &state_ref);
        })?;
    }

    wire_drop_events(
        app_root.clone(),
        Rc::clone(&graph),
        Rc::clone(&state),
        Rc::clone(&object_url),
        toggle_button.clone(),
        control_status.clone(),
    )?;

    wire_pointer_events(
        app_root,
        Rc::clone(&state),
        toggle_button.clone(),
        control_status.clone(),
    )?;

    wire_keyboard_events(
        document,
        Rc::clone(&graph),
        Rc::clone(&state),
        WebKeyboardControls {
            toggle_button: toggle_button.clone(),
            control_status: control_status.clone(),
            file_input,
            url_input,
            motion_input,
        },
    )?;

    wire_audio_events(&graph.audio, state, toggle_button, control_status)?;
    Ok(())
}

fn load_browser_local_file(
    file: web_sys::File,
    graph: &AudioGraph,
    state: &Rc<RefCell<WebAppState>>,
    object_url: &Rc<RefCell<Option<String>>>,
    toggle_button: &HtmlButtonElement,
    control_status: &HtmlElement,
) {
    let file_name = file.name();
    if let Some(message) = web_audio_drop_error(&file_name, &file.type_()) {
        set_error(state, toggle_button, control_status, message.to_string());
        return;
    }

    revoke_object_url(object_url);
    match Url::create_object_url_with_blob(&file) {
        Ok(url) => {
            graph.audio.set_cross_origin(None);
            graph.audio.set_src(&url);
            graph.audio.load();
            *object_url.borrow_mut() = Some(url);

            {
                let mut state = state.borrow_mut();
                let source = WebAudioSource::local_file(file_name);
                state.source = Some(source.clone());
                remember_recent_source(&mut state.recent_sources, source);
                state.transport = TransportState::Ready;
                state.focus = WebFocus::LocalFile;
                state.status = "Local audio loaded".to_string();
                state.error = None;
                state.current_time = 0.0;
                state.duration = None;
                state.bands = vec![0.0; BAND_COUNT];
                state.waveform = vec![0.0; WAVEFORM_SAMPLE_COUNT];
                state.bpm = WebBpmState::estimating();
                state.last_bpm_sample_ms = None;
            }
            sync_controls(toggle_button, control_status, &state.borrow());
        }
        Err(error) => set_error(
            state,
            toggle_button,
            control_status,
            format!("Could not create browser object URL: {error:?}"),
        ),
    }
}

fn wire_drop_events(
    app_root: HtmlElement,
    graph: Rc<AudioGraph>,
    state: Rc<RefCell<WebAppState>>,
    object_url: Rc<RefCell<Option<String>>>,
    toggle_button: HtmlButtonElement,
    control_status: HtmlElement,
) -> Result<(), JsValue> {
    {
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(app_root.as_ref(), "dragover", move |event| {
            let Ok(drag_event) = event.dyn_into::<DragEvent>() else {
                return;
            };
            drag_event.prevent_default();
            if let Some(data_transfer) = drag_event.data_transfer() {
                data_transfer.set_drop_effect("copy");
            }
            {
                let mut state = state.borrow_mut();
                state.focus = WebFocus::LocalFile;
                state.status = "Drop local audio to load".to_string();
                state.error = None;
            }
            sync_controls(&toggle_button, &control_status, &state.borrow());
        })?;
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let object_url = Rc::clone(&object_url);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(app_root.as_ref(), "drop", move |event| {
            let Ok(drag_event) = event.dyn_into::<DragEvent>() else {
                return;
            };
            drag_event.prevent_default();

            let Some(file) = drag_event
                .data_transfer()
                .and_then(|data_transfer| data_transfer.files())
                .and_then(|files| files.get(0))
            else {
                set_error(
                    &state,
                    &toggle_button,
                    &control_status,
                    WEB_AUDIO_DROP_REJECTION.to_string(),
                );
                return;
            };

            load_browser_local_file(
                file,
                &graph,
                &state,
                &object_url,
                &toggle_button,
                &control_status,
            );
        })?;
    }

    Ok(())
}

fn wire_pointer_events(
    app_root: HtmlElement,
    state: Rc<RefCell<WebAppState>>,
    toggle_button: HtmlButtonElement,
    control_status: HtmlElement,
) -> Result<(), JsValue> {
    let app_root_for_event = app_root.clone();
    add_event_listener(app_root.as_ref(), "click", move |event| {
        let Ok(mouse_event) = event.dyn_into::<MouseEvent>() else {
            return;
        };

        let bounds = app_root_for_event.get_bounding_client_rect();
        let target_index = {
            let state = state.borrow();
            let Some((x, y)) = web_pointer_cell_from_canvas_offset(
                f64::from(mouse_event.client_x()) - bounds.left(),
                f64::from(mouse_event.client_y()) - bounds.top(),
                bounds.width(),
                bounds.height(),
                state.terminal_area,
            ) else {
                return;
            };
            let Some(index) = web_audio_control_at_cell(&state.audio_control_hit_zones, x, y)
            else {
                return;
            };
            index
        };

        mouse_event.prevent_default();
        {
            let mut state = state.borrow_mut();
            state.audio_controls.selected_control = target_index.min(WEB_EQ_CONTROL_COUNT - 1);
            state.audio_controls.control_revision =
                state.audio_controls.control_revision.wrapping_add(1);
            state.focus = WebFocus::AudioControls;
            state.status = web_audio_control_status(&state.audio_controls);
            persist_web_settings_to_storage(&state);
            sync_controls(&toggle_button, &control_status, &state);
        }
    })
}

fn wire_keyboard_events(
    document: &Document,
    graph: Rc<AudioGraph>,
    state: Rc<RefCell<WebAppState>>,
    controls: WebKeyboardControls,
) -> Result<(), JsValue> {
    add_event_listener(document.as_ref(), "keydown", move |event| {
        let Ok(keyboard_event) = event.dyn_into::<KeyboardEvent>() else {
            return;
        };
        if keyboard_event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|element| {
                matches!(element.id().as_str(), "amp808-url" | "amp808-motion")
                    && keyboard_event.key() != "Escape"
            })
        {
            return;
        }
        let Some(action) = web_action_for_key(&keyboard_event.key()) else {
            return;
        };
        keyboard_event.prevent_default();

        {
            let focus = state.borrow().focus;
            state.borrow_mut().focus = web_focus_after_action(focus, action);
        }

        match action {
            WebAction::TogglePlayback => toggle_browser_playback(
                Rc::clone(&graph),
                Rc::clone(&state),
                controls.toggle_button.clone(),
                controls.control_status.clone(),
            ),
            WebAction::FocusLocalFile => {
                controls.file_input.click();
                let state_ref = state.borrow();
                sync_controls(
                    &controls.toggle_button,
                    &controls.control_status,
                    &state_ref,
                );
            }
            WebAction::FocusHostedUrl => {
                let _ = controls.url_input.focus();
                let state_ref = state.borrow();
                sync_controls(
                    &controls.toggle_button,
                    &controls.control_status,
                    &state_ref,
                );
            }
            WebAction::FocusAudioControls => {
                {
                    let mut state = state.borrow_mut();
                    state.status = web_audio_control_status(&state.audio_controls);
                }
                let state_ref = state.borrow();
                sync_controls(
                    &controls.toggle_button,
                    &controls.control_status,
                    &state_ref,
                );
            }
            WebAction::SelectPreviousAudioControl
            | WebAction::SelectNextAudioControl
            | WebAction::AdjustSelectedAudioControlUp
            | WebAction::AdjustSelectedAudioControlDown
            | WebAction::CycleSoundMode
            | WebAction::VolumeUp
            | WebAction::VolumeDown => {
                {
                    let mut state = state.borrow_mut();
                    web_audio_controls_after_action(&mut state.audio_controls, action);
                    state.status = web_audio_control_status(&state.audio_controls);
                    persist_web_settings_to_storage(&state);
                }
                let state_ref = state.borrow();
                apply_web_audio_controls(&graph, &state_ref.audio_controls);
                sync_controls(
                    &controls.toggle_button,
                    &controls.control_status,
                    &state_ref,
                );
            }
            WebAction::CycleVisualMode => {
                {
                    let mut state = state.borrow_mut();
                    state.visual_mode =
                        web_visual_mode_after_action(state.visual_mode, WebAction::CycleVisualMode);
                    state.status = format!(
                        "Visualizer mode {}",
                        web_visual_mode_label(state.visual_mode)
                    );
                    persist_web_settings_to_storage(&state);
                }
                let state_ref = state.borrow();
                sync_controls(
                    &controls.toggle_button,
                    &controls.control_status,
                    &state_ref,
                );
            }
            WebAction::ToggleMotion => {
                {
                    let mut state = state.borrow_mut();
                    state.motion_enabled =
                        web_motion_enabled_after_action(state.motion_enabled, action);
                    state.status = motion_status_text(state.motion_enabled).to_string();
                    controls.motion_input.set_checked(state.motion_enabled);
                    persist_web_settings_to_storage(&state);
                }
                let state_ref = state.borrow();
                sync_controls(
                    &controls.toggle_button,
                    &controls.control_status,
                    &state_ref,
                );
            }
            WebAction::SeekBack => seek_browser_audio(
                &graph,
                &state,
                -WEB_SEEK_STEP_SECONDS,
                &controls.toggle_button,
                &controls.control_status,
            ),
            WebAction::SeekForward => seek_browser_audio(
                &graph,
                &state,
                WEB_SEEK_STEP_SECONDS,
                &controls.toggle_button,
                &controls.control_status,
            ),
            WebAction::ClearFocus => {
                let state_ref = state.borrow();
                sync_controls(
                    &controls.toggle_button,
                    &controls.control_status,
                    &state_ref,
                );
            }
        }
    })
}

fn seek_browser_audio(
    graph: &AudioGraph,
    state: &Rc<RefCell<WebAppState>>,
    delta_seconds: f64,
    toggle_button: &HtmlButtonElement,
    control_status: &HtmlElement,
) {
    if state.borrow().source.is_none() {
        {
            let mut state = state.borrow_mut();
            state.status = "Load audio before seeking".to_string();
        }
        let state_ref = state.borrow();
        sync_controls(toggle_button, control_status, &state_ref);
        return;
    }

    let duration = finite_duration(&graph.audio);
    let target = web_seek_target_seconds(graph.audio.current_time(), duration, delta_seconds);
    graph.audio.set_current_time(target);
    {
        let mut state = state.borrow_mut();
        state.current_time = target;
        state.duration = duration;
        state.status = format!("Seeked to {}", format_seconds(target));
    }
    let state_ref = state.borrow();
    sync_controls(toggle_button, control_status, &state_ref);
}

fn toggle_browser_playback(
    graph: Rc<AudioGraph>,
    state: Rc<RefCell<WebAppState>>,
    toggle_button: HtmlButtonElement,
    control_status: HtmlElement,
) {
    if graph.audio.paused() {
        let resume = graph.context.resume().ok();
        match graph.audio.play() {
            Ok(play) => {
                spawn_local(async move {
                    if let Some(resume) = resume {
                        let _ = JsFuture::from(resume).await;
                    }
                    if let Err(error) = JsFuture::from(play).await {
                        set_error(
                            &state,
                            &toggle_button,
                            &control_status,
                            format!("Browser refused playback: {error:?}"),
                        );
                    }
                });
            }
            Err(error) => set_error(
                &state,
                &toggle_button,
                &control_status,
                format!("Browser refused playback: {error:?}"),
            ),
        }
    } else if let Err(error) = graph.audio.pause() {
        set_error(
            &state,
            &toggle_button,
            &control_status,
            format!("Could not pause playback: {error:?}"),
        );
    }
}

fn wire_audio_events(
    audio: &HtmlAudioElement,
    state: Rc<RefCell<WebAppState>>,
    toggle_button: HtmlButtonElement,
    control_status: HtmlElement,
) -> Result<(), JsValue> {
    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.clone().as_ref(), "loadedmetadata", move |_| {
            let mut state = state.borrow_mut();
            state.duration = finite_duration(&audio);
            state.transport = TransportState::Ready;
            state.status = "Audio metadata loaded".to_string();
            state.error = None;
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        add_event_listener(audio.clone().as_ref(), "timeupdate", move |_| {
            let mut state = state.borrow_mut();
            state.current_time = audio.current_time();
            state.duration = finite_duration(&audio);
        })?;
    }

    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.clone().as_ref(), "play", move |_| {
            let mut state = state.borrow_mut();
            state.current_time = audio.current_time();
            state.duration = finite_duration(&audio);
            state.transport = TransportState::Playing;
            state.status = "Playback running from browser audio".to_string();
            state.error = None;
            state.last_bpm_sample_ms = None;
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.clone().as_ref(), "pause", move |_| {
            let mut state = state.borrow_mut();
            if !audio.ended() && state.transport != TransportState::Error {
                state.transport = TransportState::Paused;
                state.status = "Playback paused".to_string();
            }
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.as_ref(), "ended", move |_| {
            let mut state = state.borrow_mut();
            state.transport = TransportState::Ended;
            state.status = "Playback ended".to_string();
            state.bands = vec![0.0; BAND_COUNT];
            state.waveform = vec![0.0; WAVEFORM_SAMPLE_COUNT];
            state.bpm = WebBpmState::unavailable();
            state.last_bpm_sample_ms = None;
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.clone().as_ref(), "error", move |_| {
            let source = state.borrow().source.clone();
            let message = browser_media_error_message(
                source.as_ref(),
                audio.error().as_ref().map(|error| error.code()),
            );
            set_error(&state, &toggle_button, &control_status, message);
        })?;
    }

    Ok(())
}

fn sample_analyser(graph: &AudioGraph, state: &Rc<RefCell<WebAppState>>) {
    if state.borrow().transport != TransportState::Playing {
        return;
    }

    let mut bins = vec![0; graph.analyser.frequency_bin_count() as usize];
    graph.analyser.get_byte_frequency_data(&mut bins);
    let bands = analyser_bins_to_bands(&bins, BAND_COUNT);
    let mut waveform_bins = vec![128; graph.analyser.frequency_bin_count() as usize];
    graph.analyser.get_byte_time_domain_data(&mut waveform_bins);
    let waveform = waveform_bytes_to_samples(&waveform_bins, WAVEFORM_SAMPLE_COUNT);
    let now_ms = js_sys::Date::now();
    let mut state = state.borrow_mut();
    let hop_seconds = state
        .last_bpm_sample_ms
        .map(|last_ms| ((now_ms - last_ms) / 1000.0).clamp(0.01, 0.12))
        .unwrap_or(WEB_BPM_DEFAULT_HOP_SECONDS);
    state.last_bpm_sample_ms = Some(now_ms);
    state
        .bpm
        .update_from_time_domain_bytes(&waveform_bins, hop_seconds, true);
    state.bands = bands;
    state.waveform = waveform;
}

fn render_web_808(frame: &mut Frame<'_>, state: &mut WebAppState, fx: &mut WebFxRuntime) {
    let tick = fx.next_tick(js_sys::Date::now());
    if !state.motion_enabled {
        fx.clear_effects();
    }
    let area = frame.area();
    state.terminal_area = area;
    state.audio_control_hit_zones.clear();
    let block = Block::default()
        .title(" AMP808 WEB ")
        .title_style(hardware_brand_style())
        .style(classic_hardware_body_style())
        .borders(Borders::ALL)
        .border_set(web_panel_border_set())
        .border_style(classic_body_border_style());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(machine_header_height()),
            Constraint::Min(14),
            Constraint::Length(2),
        ])
        .split(inner);

    render_machine_header(frame, rows[0], state);
    if state.motion_enabled {
        let source_label = state
            .source
            .as_ref()
            .map(WebAudioSource::label)
            .unwrap_or("No audio loaded");
        let header_identity_area = machine_header_identity_effect_area(rows[0], source_label);
        if let Some(effect) = fx.header_effect(state.transport) {
            frame.render_effect(effect, header_identity_area, tick);
        }
        if let Some(effect) = fx.header_identity_effect(state.transport) {
            frame.render_effect(effect, header_identity_area, tick);
        }
    }

    if rows[1].width < 90 {
        let deck_rows = web_compact_deck_layout(rows[1]);
        render_left_control_panel(frame, deck_rows[0], state, fx, tick);
        render_knob_bank(frame, deck_rows[1], state, fx, tick);
        render_visualizer(frame, deck_rows[2], state, fx, tick);
        render_step_strip(frame, deck_rows[3], state, fx, tick);
    } else {
        let body = web_desktop_body_layout(rows[1]);
        render_left_control_panel(frame, body[0], state, fx, tick);

        let deck_rows = web_desktop_deck_layout(body[1]);
        render_knob_bank(frame, deck_rows[0], state, fx, tick);
        render_visualizer(frame, deck_rows[1], state, fx, tick);
        render_step_strip(frame, deck_rows[2], state, fx, tick);
    }

    let compact_status = "Load audio or CORS URL";
    let footer_text = match state.error.as_deref() {
        Some(error) => error,
        None if rows[2].width < 70 => compact_status,
        None => &state.status,
    };
    let footer_color = if state.error.is_some() {
        Classic808Palette::RED_TEXT.ratatui()
    } else {
        Classic808Palette::IVORY.ratatui()
    };
    let footer = Paragraph::new(Text::from(vec![
        command_strip_line(state),
        Line::from(Span::styled(footer_text, Style::default().fg(footer_color))),
    ]))
    .style(classic_faceplate_style())
    .alignment(Alignment::Center);
    frame.render_widget(footer, rows[2]);
    if state.motion_enabled {
        if let Some(effect) = fx.transition_effect(state.transport) {
            frame.render_effect(effect, rows[2], tick);
        }
    }
}

fn web_compact_deck_layout(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .spacing(WEB_PANE_GAP)
        .constraints([
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Min(8),
            Constraint::Length(7),
        ])
        .split(area)
        .to_vec()
}

fn web_desktop_body_layout(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .spacing(WEB_PANE_GAP)
        .constraints([Constraint::Length(28), Constraint::Min(24)])
        .split(area)
        .to_vec()
}

fn web_desktop_deck_layout(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .spacing(WEB_PANE_GAP)
        .constraints([
            Constraint::Length(10),
            Constraint::Min(10),
            Constraint::Length(7),
        ])
        .split(area)
        .to_vec()
}

fn command_strip_line(state: &WebAppState) -> Line<'static> {
    Line::from(vec![
        Span::styled("SPC", command_key_style(state.focus == WebFocus::Transport)),
        Span::styled(" PLAY  ", classic_small_label_style()),
        Span::styled("<", command_key_style(state.focus == WebFocus::Transport)),
        Span::styled(" -15  ", classic_small_label_style()),
        Span::styled(">", command_key_style(state.focus == WebFocus::Transport)),
        Span::styled(" +15  ", classic_small_label_style()),
        Span::styled("L", command_key_style(state.focus == WebFocus::LocalFile)),
        Span::styled(" FILE/DROP  ", classic_small_label_style()),
        Span::styled("U", command_key_style(state.focus == WebFocus::HostedUrl)),
        Span::styled(" URL  ", classic_small_label_style()),
        Span::styled("V", command_key_style(state.focus == WebFocus::Analyser)),
        Span::styled(
            format!(" VIS:{}  ", web_visual_mode_label(state.visual_mode)),
            classic_small_label_style(),
        ),
        Span::styled(
            "I",
            command_key_style(state.focus == WebFocus::AudioControls),
        ),
        Span::styled(" EQ  ", classic_small_label_style()),
        Span::styled(
            "E",
            command_key_style(state.focus == WebFocus::AudioControls),
        ),
        Span::styled(
            format!(" MODE:{}  ", web_eq_preset_label(&state.audio_controls)),
            classic_small_label_style(),
        ),
        Span::styled(
            "[",
            command_key_style(state.focus == WebFocus::AudioControls),
        ),
        Span::styled(
            "]",
            command_key_style(state.focus == WebFocus::AudioControls),
        ),
        Span::styled(" CTRL  ", classic_small_label_style()),
        Span::styled(
            "+-",
            command_key_style(state.focus == WebFocus::AudioControls),
        ),
        Span::styled(" VOL  ", classic_small_label_style()),
        Span::styled("M", command_key_style(state.focus == WebFocus::Motion)),
        Span::styled(" MOT  ", classic_small_label_style()),
        Span::styled("ESC", command_key_style(false)),
        Span::styled(" HOME", classic_small_label_style()),
    ])
}

fn command_key_style(active: bool) -> Style {
    let color = if active {
        Classic808Palette::YELLOW.ratatui()
    } else {
        Classic808Palette::IVORY.ratatui()
    };
    Style::default()
        .fg(color)
        .bg(Classic808Palette::OLIVE.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn machine_brand_segments() -> [(&'static str, bool); 3] {
    [
        ("Machine Controlled ", true),
        ("Rhythm Composer ", false),
        ("TR-808 WEB", true),
    ]
}

fn machine_brand_label() -> String {
    machine_brand_segments()
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

fn machine_logo_mark() -> &'static str {
    "808"
}

fn machine_logo_style() -> Style {
    Style::default()
        .fg(Classic808Palette::BRAND_ORANGE.ratatui())
        .bg(Classic808Palette::BODY.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn machine_logo_pixel_size() -> PixelSize {
    PixelSize::HalfWidth
}

fn machine_header_height() -> u16 {
    MACHINE_HEADER_HEIGHT
}

fn machine_header_can_show_logo(width: u16, source_label: &str) -> bool {
    let source_line_width = "SOURCE ".len() + source_label.chars().count();
    let text_width = machine_brand_label().chars().count().max(source_line_width) as u16;
    let min_width = MACHINE_LOGO_WIDTH
        .saturating_add(MACHINE_LOGO_GUTTER)
        .saturating_add(text_width)
        .saturating_add(MACHINE_HEADER_TEXT_MARGIN);

    width >= min_width
}

fn machine_logo_area(area: Rect, source_label: &str) -> Option<Rect> {
    if area.height < 3 || !machine_header_can_show_logo(area.width, source_label) {
        return None;
    }

    Some(Rect::new(
        area.x,
        area.y,
        MACHINE_LOGO_WIDTH,
        area.height.min(MACHINE_HEADER_HEIGHT),
    ))
}

fn machine_header_text_area(area: Rect, source_label: &str) -> Rect {
    if machine_logo_area(area, source_label).is_some() {
        let x_offset = MACHINE_LOGO_WIDTH.saturating_add(MACHINE_LOGO_GUTTER);
        return Rect::new(
            area.x.saturating_add(x_offset),
            area.y,
            area.width.saturating_sub(x_offset),
            area.height,
        );
    }

    area
}

fn machine_header_identity_effect_area(area: Rect, _source_label: &str) -> Rect {
    area
}

fn render_machine_header(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let brand_label = machine_brand_label();
    let source_label = state
        .source
        .as_ref()
        .map(WebAudioSource::label)
        .unwrap_or("No audio loaded");

    let brand_line = if area.width < brand_label.len() as u16 {
        Line::from(Span::styled(brand_label, hardware_brand_style()))
    } else {
        Line::from(
            machine_brand_segments()
                .into_iter()
                .map(|(text, is_brand)| {
                    let style = if is_brand {
                        hardware_brand_style()
                    } else {
                        hardware_body_text_style().add_modifier(Modifier::BOLD)
                    };
                    Span::styled(text, style)
                })
                .collect::<Vec<_>>(),
        )
    };

    let lines = vec![
        brand_line,
        Line::from(vec![
            Span::styled(
                "SOURCE ",
                hardware_body_text_style().add_modifier(Modifier::BOLD),
            ),
            Span::styled(source_label, hardware_body_text_style()),
        ]),
    ];

    let text_area = machine_header_text_area(area, source_label);
    let alignment = if machine_logo_area(area, source_label).is_some() {
        Alignment::Left
    } else {
        Alignment::Center
    };

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(classic_hardware_body_style())
            .alignment(alignment),
        text_area,
    );

    if let Some(logo_area) = machine_logo_area(area, source_label) {
        let logo = BigText::builder()
            .pixel_size(machine_logo_pixel_size())
            .style(machine_logo_style())
            .left_aligned()
            .lines(vec![Line::from(machine_logo_mark())])
            .build();
        frame.render_widget(logo, logo_area);
    }
}

fn render_808_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    spec: WebPanelSpec,
    effect: Option<&mut tachyonfx::Effect>,
    tick: tachyonfx::Duration,
) -> Rect {
    let block = Block::default()
        .title(panel_title(spec))
        .style(classic_panel_inset_style())
        .borders(Borders::ALL)
        .border_set(web_panel_border_set())
        .border_style(panel_border_style(spec.state));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if let Some(effect) = effect {
        frame.render_effect(effect, area, tick);
    }
    inner
}

fn panel_title(spec: WebPanelSpec) -> Line<'static> {
    let mut spans = vec![Span::styled(spec.title, classic_label_style())];
    if let Some(lamp) = spec.lamp {
        spans.push(Span::styled(panel_lamp_label(lamp), panel_lamp_style(lamp)));
    }
    Line::from(spans)
}

fn panel_lamp_label(state: PanelState) -> &'static str {
    match state {
        PanelState::Idle => "[ ]",
        PanelState::Armed => "[R]",
        PanelState::Active => "[*]",
        PanelState::Error => "[!]",
    }
}

fn panel_lamp_style(state: PanelState) -> Style {
    Style::default()
        .fg(match state {
            PanelState::Idle => Classic808Palette::DIM.ratatui(),
            PanelState::Armed => Classic808Palette::AMBER.ratatui(),
            PanelState::Active => Classic808Palette::YELLOW.ratatui(),
            PanelState::Error => Classic808Palette::RED_TEXT.ratatui(),
        })
        .add_modifier(if matches!(state, PanelState::Active | PanelState::Error) {
            Modifier::BOLD
        } else {
            Modifier::empty()
        })
}

fn panel_border_style(state: PanelState) -> Style {
    Style::default().fg(match state {
        PanelState::Idle => Classic808Palette::ORANGE.ratatui(),
        PanelState::Armed => Classic808Palette::AMBER.ratatui(),
        PanelState::Active => Classic808Palette::YELLOW.ratatui(),
        PanelState::Error => Classic808Palette::RED_TEXT.ratatui(),
    })
}

fn web_panel_border_set() -> border::Set<'static> {
    border::Set {
        top_left: "▛",
        top_right: "▜",
        bottom_left: "▙",
        bottom_right: "▟",
        horizontal_top: "━",
        horizontal_bottom: "━",
        vertical_left: "┃",
        vertical_right: "┃",
    }
}

fn render_left_control_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &WebAppState,
    fx: &mut WebFxRuntime,
    tick: tachyonfx::Duration,
) {
    let spec = web_panel_spec(PanelRole::Transport, state);
    let effect = state
        .motion_enabled
        .then(|| fx.panel_effect(spec))
        .flatten();
    let inner = render_808_panel(frame, area, spec, effect, tick);

    if inner.width < 18 || inner.height < 8 {
        render_left_control_fallback(frame, inner, state);
        return;
    }

    let expanded_recent = inner.height >= 18;
    let rows = if expanded_recent {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Min(3),
                Constraint::Length(2),
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(2),
            ])
            .split(inner)
    };

    let status_lines = vec![
        Line::from(vec![
            Span::styled("MODE     ", classic_small_label_style()),
            Span::styled("WEB AUDIO", classic_value_style()),
        ]),
        Line::from(vec![
            Span::styled("STATE    ", classic_small_label_style()),
            Span::styled(
                state.transport.label(),
                Style::default()
                    .fg(transport_color(state.transport))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("TIME     ", classic_small_label_style()),
            Span::styled(
                format_time_status(state.current_time, state.duration),
                classic_value_style(),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(status_lines)), rows[0]);

    let (dial_area, controls_area) = if expanded_recent {
        render_recent_sources(frame, rows[1], &state.recent_sources);
        (rows[2], rows[3])
    } else {
        (rows[1], rows[2])
    };

    let tempo = web_tempo_display(&state.bpm);
    render_tempo_dial(frame, dial_area, tempo.normalized, &tempo.label);

    let control_lines = vec![
        Line::from(vec![
            Span::styled("MASTER VOL ", classic_small_label_style()),
            Span::styled("O", classic_knob_style()),
        ]),
        Line::from(vec![
            Span::styled("PATTERN A/B ", classic_small_label_style()),
            Span::styled("A ", active_lamp_style(0.0)),
            Span::styled("B", inactive_lamp_style()),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(control_lines)), controls_area);
}

fn render_recent_sources(frame: &mut Frame<'_>, area: Rect, recent_sources: &[WebAudioSource]) {
    if area.height == 0 || area.width < 8 {
        return;
    }

    let mut lines = vec![Line::from(Span::styled(
        "RECENT SOURCES",
        classic_small_label_style().add_modifier(Modifier::BOLD),
    ))];
    let entry_count = usize::from(area.height.saturating_sub(1)).min(recent_sources.len());

    if entry_count == 0 {
        lines.push(Line::from(Span::styled(
            "--",
            Style::default().fg(Classic808Palette::DIM.ratatui()),
        )));
    } else {
        let label_width = usize::from(area.width.saturating_sub(3)).max(2);
        for (index, source) in recent_sources.iter().take(entry_count).enumerate() {
            let style = if source.is_hosted_url() {
                Style::default().fg(Classic808Palette::AMBER.ratatui())
            } else {
                Style::default().fg(Classic808Palette::IVORY.ratatui())
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", index + 1),
                    Style::default().fg(Classic808Palette::YELLOW.ratatui()),
                ),
                Span::styled(recent_source_display_label(source, label_width), style),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_left_control_fallback(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let lines = vec![
        Line::from(vec![
            Span::styled("STATE ", classic_small_label_style()),
            Span::styled(
                state.transport.label(),
                Style::default().fg(transport_color(state.transport)),
            ),
        ]),
        Line::from(vec![
            Span::styled("TIME  ", classic_small_label_style()),
            Span::styled(
                format_time_status(state.current_time, state.duration),
                classic_value_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("TEMPO ", classic_small_label_style()),
            Span::styled(
                format!("{} BPM", web_tempo_display(&state.bpm).label),
                classic_value_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("PATTERN ", classic_small_label_style()),
            Span::styled("A ", active_lamp_style(0.0)),
            Span::styled("B", inactive_lamp_style()),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_tempo_dial(frame: &mut Frame<'_>, area: Rect, bpm_norm: f64, bpm_label: &str) {
    if area.width < 8 || area.height < 4 {
        let fallback = Line::from(vec![
            Span::styled("TEMPO ", classic_small_label_style()),
            Span::styled(format!("{bpm_label} BPM"), classic_value_style()),
        ]);
        frame.render_widget(Paragraph::new(fallback).alignment(Alignment::Center), area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(1)])
        .split(area);
    let (canvas_area, label_area) = (chunks[0], chunks[1]);
    let geometry = tempo_dial_geometry_808(canvas_area);
    let start_angle = 210.0_f64.to_radians();
    let end_angle = (-30.0_f64).to_radians();
    let sweep = start_angle - end_angle;
    let val_angle = start_angle - bpm_norm.clamp(0.0, 1.0) * sweep;
    let bpm_label = bpm_label.to_string();

    let canvas = Canvas::default()
        .x_bounds(geometry.x_bounds)
        .y_bounds(geometry.y_bounds)
        .marker(Marker::Braille)
        .paint(move |ctx| {
            for i in 0..60usize {
                let t1 = i as f64 / 60.0;
                let t2 = (i + 1) as f64 / 60.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: geometry.radius * a1.cos(),
                    y1: geometry.radius * a1.sin(),
                    x2: geometry.radius * a2.cos(),
                    y2: geometry.radius * a2.sin(),
                    color: Classic808Palette::IVORY.ratatui(),
                });
            }

            let active_segs = (bpm_norm.clamp(0.0, 1.0) * 60.0) as usize;
            for i in 0..active_segs {
                let t1 = i as f64 / 60.0;
                let t2 = (i + 1) as f64 / 60.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: geometry.radius * a1.cos(),
                    y1: geometry.radius * a1.sin(),
                    x2: geometry.radius * a2.cos(),
                    y2: geometry.radius * a2.sin(),
                    color: dial_arc_color((t1 + t2) / 2.0),
                });
            }

            ctx.draw(&CanvasLine {
                x1: 0.0,
                y1: 0.0,
                x2: geometry.radius * 0.72 * val_angle.cos(),
                y2: geometry.radius * 0.72 * val_angle.sin(),
                color: Classic808Palette::AMBER.ratatui(),
            });

            for i in 0u32..=10 {
                let t = i as f64 / 10.0;
                let angle = start_angle - t * sweep;
                ctx.draw(&CanvasLine {
                    x1: (geometry.radius + 0.35) * angle.cos(),
                    y1: (geometry.radius + 0.35) * angle.sin(),
                    x2: (geometry.radius + 0.9) * angle.cos(),
                    y2: (geometry.radius + 0.9) * angle.sin(),
                    color: tempo_tick_color_808(),
                });
            }

            ctx.print(
                -1.4,
                1.1,
                Span::styled(
                    "BPM",
                    Style::default().fg(Classic808Palette::IVORY.ratatui()),
                ),
            );
            ctx.print(
                -1.7,
                -0.5,
                Span::styled(
                    bpm_label.clone(),
                    Style::default()
                        .fg(Classic808Palette::AMBER.ratatui())
                        .add_modifier(Modifier::BOLD),
                ),
            );
        });

    frame.render_widget(canvas, canvas_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "TEMPO",
            classic_small_label_style(),
        )))
        .alignment(Alignment::Center),
        label_area,
    );
}

#[derive(Clone, Copy, Debug)]
struct TempoDialGeometry808 {
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    radius: f64,
}

fn tempo_dial_geometry_808(area: Rect) -> TempoDialGeometry808 {
    const Y_HALF: f64 = 9.0;
    const LABEL_PAD: f64 = 1.5;
    const ROUNDING_CORRECTION: f64 = 1.45;
    let visual_ratio = area.width as f64 / (area.height.max(1) as f64 * 2.0);
    let x_half = (Y_HALF * visual_ratio * ROUNDING_CORRECTION).clamp(6.0, 15.0);
    let radius = (x_half.min(Y_HALF) - LABEL_PAD - 0.2).clamp(2.2, 6.2);
    TempoDialGeometry808 {
        x_bounds: [-x_half, x_half],
        y_bounds: [-Y_HALF, Y_HALF],
        radius,
    }
}

fn dial_arc_color(position: f64) -> Color {
    if position > 0.72 {
        Classic808Palette::RED.ratatui()
    } else if position > 0.36 {
        Classic808Palette::ORANGE.ratatui()
    } else {
        Classic808Palette::AMBER.ratatui()
    }
}

fn tempo_tick_color_808() -> Color {
    Classic808Palette::GREY.ratatui()
}

fn render_knob_bank(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut WebAppState,
    fx: &mut WebFxRuntime,
    tick: tachyonfx::Duration,
) {
    let spec = web_panel_spec(PanelRole::Instrument, state);
    let effect = state
        .motion_enabled
        .then(|| fx.panel_effect(spec))
        .flatten();
    let inner = render_808_panel(frame, area, spec, effect, tick);
    let specs = instrument_control_specs();
    let visible = instrument_channel_visible_count(inner.width).min(specs.len());
    let specs = &specs[..visible];

    if inner.height < 4 {
        let knob_cells = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, visible as u32); visible])
            .split(inner);
        state.audio_control_hit_zones = web_audio_control_hit_zones(&knob_cells, visible);

        let mut knob_row = Vec::with_capacity(specs.len());
        let mut label_row = Vec::with_capacity(specs.len());
        for spec in specs {
            knob_row.push(Span::styled(" (@) ", classic_knob_style()));
            label_row.push(instrument_short_span(spec, 5));
        }
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(knob_row),
                Line::from(label_row),
            ]))
            .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    if inner.height < 5 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(inner);
        let knob_cells = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, visible as u32); visible])
            .split(rows[0]);
        state.audio_control_hit_zones = web_audio_control_hit_zones(&knob_cells, visible);

        for (index, (cell, spec)) in knob_cells.iter().zip(specs.iter()).enumerate() {
            render_canvas_knob(
                frame,
                *cell,
                web_knob_value(&state.audio_controls, index),
                spec,
                state.focus == WebFocus::AudioControls
                    && index == state.audio_controls.selected_control,
            );
        }

        frame.render_widget(
            Paragraph::new(Line::from(
                specs
                    .iter()
                    .map(|spec| {
                        Span::styled(
                            format!("{:^8}", instrument_label_cap_text(spec)),
                            instrument_parameter_style(spec.family),
                        )
                    })
                    .collect::<Vec<_>>(),
            ))
            .alignment(Alignment::Center),
            rows[1],
        );
        return;
    }

    if inner.height < INSTRUMENT_CHANNEL_FULL_HEIGHT {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);
        let knob_cells = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, visible as u32); visible])
            .split(rows[0]);
        state.audio_control_hit_zones = web_audio_control_hit_zones(&knob_cells, visible);

        for (index, (cell, spec)) in knob_cells.iter().zip(specs.iter()).enumerate() {
            render_canvas_knob(
                frame,
                *cell,
                web_knob_value(&state.audio_controls, index),
                spec,
                state.focus == WebFocus::AudioControls
                    && index == state.audio_controls.selected_control,
            );
        }

        render_audio_control_readout(frame, rows[1], &state.audio_controls);
        render_audio_control_pulse(frame, &knob_cells, state, fx, tick);

        frame.render_widget(
            Paragraph::new(Line::from(
                specs
                    .iter()
                    .map(|spec| {
                        Span::styled(
                            format!("{:^8}", instrument_label_cap_text(spec)),
                            instrument_parameter_style(spec.family),
                        )
                    })
                    .collect::<Vec<_>>(),
            ))
            .alignment(Alignment::Center),
            rows[2],
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(inner);
    let knob_cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, visible as u32); visible])
        .split(rows[0]);
    state.audio_control_hit_zones = web_audio_control_hit_zones(&knob_cells, visible);

    for (index, (cell, spec)) in knob_cells.iter().zip(specs.iter()).enumerate() {
        render_canvas_knob(
            frame,
            *cell,
            web_knob_value(&state.audio_controls, index),
            spec,
            state.focus == WebFocus::AudioControls
                && index == state.audio_controls.selected_control,
        );
    }

    render_audio_control_readout(frame, rows[1], &state.audio_controls);
    render_audio_control_pulse(frame, &knob_cells, state, fx, tick);

    let label_lines = instrument_label_cap_lines(specs, 8);
    frame.render_widget(
        Paragraph::new(label_lines).alignment(Alignment::Center),
        rows[2],
    );
}

fn render_audio_control_readout(frame: &mut Frame<'_>, area: Rect, controls: &WebAudioControls) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            web_audio_selected_control_readout(controls),
            Style::default()
                .fg(Classic808Palette::IVORY.ratatui())
                .bg(Classic808Palette::FACEPLATE.ratatui())
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        area,
    );
}

fn render_audio_control_pulse(
    frame: &mut Frame<'_>,
    knob_cells: &[Rect],
    state: &WebAppState,
    fx: &mut WebFxRuntime,
    tick: tachyonfx::Duration,
) {
    if !state.motion_enabled {
        return;
    }

    let Some(area) = knob_cells
        .get(state.audio_controls.selected_control)
        .copied()
    else {
        return;
    };
    let Some(effect) = fx.audio_control_effect(&state.audio_controls) else {
        return;
    };

    frame.render_effect(effect, area, tick);
}

fn render_canvas_knob(
    frame: &mut Frame<'_>,
    area: Rect,
    value: f64,
    spec: &InstrumentControlSpec,
    selected: bool,
) {
    if area.width < 5 || area.height < 3 {
        let fallback = vec![
            Line::from(Span::styled("(@)", classic_knob_style())),
            Line::from(instrument_short_span(spec, usize::from(area.width.max(1)))),
        ];
        frame.render_widget(
            Paragraph::new(Text::from(fallback)).alignment(Alignment::Center),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(1)])
        .split(area);
    let canvas_area = chunks[0];
    let label_area = chunks[1];
    let (x_bounds, y_bounds) = knob_canvas_bounds_808(canvas_area);
    let value = value.clamp(0.0, 1.0);
    let start_angle = 210.0_f64.to_radians();
    let end_angle = (-30.0_f64).to_radians();
    let sweep = start_angle - end_angle;
    let val_angle = start_angle - value * sweep;
    let radius = 3.85;
    let accent = if selected {
        Classic808Palette::YELLOW.ratatui()
    } else {
        Classic808Palette::GREY.ratatui()
    };

    let canvas = Canvas::default()
        .x_bounds(x_bounds)
        .y_bounds(y_bounds)
        .marker(Marker::Braille)
        .paint(move |ctx| {
            for i in 0..24usize {
                let t1 = i as f64 / 24.0;
                let t2 = (i + 1) as f64 / 24.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: radius * a1.cos(),
                    y1: radius * a1.sin(),
                    x2: radius * a2.cos(),
                    y2: radius * a2.sin(),
                    color: Classic808Palette::IVORY.ratatui(),
                });
            }

            let active_steps = (value * 24.0) as usize;
            for i in 0..active_steps {
                let t1 = i as f64 / 24.0;
                let t2 = (i + 1) as f64 / 24.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: radius * a1.cos(),
                    y1: radius * a1.sin(),
                    x2: radius * a2.cos(),
                    y2: radius * a2.sin(),
                    color: dial_arc_color((t1 + t2) / 2.0),
                });
            }

            ctx.draw(&CanvasLine {
                x1: 0.0,
                y1: 0.0,
                x2: radius * 0.68 * val_angle.cos(),
                y2: radius * 0.68 * val_angle.sin(),
                color: accent,
            });
        });

    frame.render_widget(canvas, canvas_area);
    let label_style = if selected {
        instrument_strip_style(spec.family)
            .fg(Classic808Palette::YELLOW.ratatui())
            .add_modifier(Modifier::BOLD)
    } else {
        instrument_strip_style(spec.family)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(spec.short_label, label_style)))
            .alignment(Alignment::Center),
        label_area,
    );
}

fn instrument_channel_visible_count(width: u16) -> usize {
    (usize::from(width) / 8).clamp(1, instrument_control_specs().len())
}

fn instrument_label_cap_text(spec: &InstrumentControlSpec) -> String {
    format!(
        "{} {}",
        spec.short_label,
        instrument_parameter_label(spec, 8)
    )
}

fn instrument_label_cap_lines(specs: &[InstrumentControlSpec], width: usize) -> Text<'static> {
    Text::from(vec![
        Line::from(
            specs
                .iter()
                .map(|spec| instrument_short_span(spec, width))
                .collect::<Vec<_>>(),
        ),
        Line::from(
            specs
                .iter()
                .map(|spec| instrument_parameter_span(spec, width))
                .collect::<Vec<_>>(),
        ),
    ])
}

fn instrument_short_span(spec: &InstrumentControlSpec, width: usize) -> Span<'static> {
    Span::styled(
        format!("{:^width$}", spec.short_label, width = width.max(2)),
        instrument_strip_style(spec.family).add_modifier(Modifier::BOLD),
    )
}

fn instrument_parameter_span(spec: &InstrumentControlSpec, width: usize) -> Span<'static> {
    let label = instrument_parameter_label(spec, width);
    Span::styled(
        format!("{label:^width$}", width = width.max(4)),
        instrument_parameter_style(spec.family),
    )
}

fn instrument_parameter_label(spec: &InstrumentControlSpec, width: usize) -> &'static str {
    if width <= 5 {
        abbreviate_parameter_label(spec.parameter_label)
    } else {
        spec.parameter_label
    }
}

fn abbreviate_parameter_label(label: &'static str) -> &'static str {
    match label {
        "LEVEL" => "LVL",
        "TUNE" => "TUNE",
        "DECAY" => "DEC",
        "SNAP" => "SNP",
        other => other,
    }
}

fn knob_canvas_bounds_808(area: Rect) -> ([f64; 2], [f64; 2]) {
    const Y_HALF: f64 = 4.0;
    const ROUNDING_CORRECTION: f64 = 1.30;
    let visual_ratio = area.width as f64 / (area.height.max(1) as f64 * 2.0);
    let x_half = (Y_HALF * visual_ratio * ROUNDING_CORRECTION).clamp(4.2, 12.0);
    ([-x_half, x_half], [-Y_HALF, Y_HALF])
}

fn web_knob_value(controls: &WebAudioControls, index: usize) -> f64 {
    web_audio_control_value(controls, index)
}

fn render_visualizer(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &WebAppState,
    fx: &mut WebFxRuntime,
    tick: tachyonfx::Duration,
) {
    let spec = web_panel_spec(PanelRole::Analyser, state);
    let effect = state
        .motion_enabled
        .then(|| fx.panel_effect(spec))
        .flatten();
    let inner = render_808_panel(frame, area, spec, effect, tick);
    let bands = &state.bands;

    if inner.height == 0 || inner.width < 2 || bands.is_empty() {
        return;
    }

    if let Some(presentation) = analyser_empty_state_presentation(state) {
        render_analyser_empty_state(frame, inner, presentation, state.transport);
        return;
    }

    match state.visual_mode {
        WebVisualMode::Bars => {
            render_spectrum_bars(frame, inner, bands);
        }
        WebVisualMode::Wave => {
            render_wave_visualizer(frame, inner, state);
        }
        WebVisualMode::Retro => {
            let lines = render_retro_visualizer_lines(
                bands,
                inner.width,
                inner.height,
                fx.visual_frame(),
                state.motion_enabled,
            );
            render_web_visualizer_lines(frame, inner, lines);
        }
        WebVisualMode::Logo => {
            let lines = render_logo_visualizer_lines(
                bands,
                inner.width,
                inner.height,
                fx.visual_frame(),
                state.motion_enabled,
            );
            render_web_visualizer_lines(frame, inner, lines);
        }
        WebVisualMode::Split => {
            render_split_visualizer(frame, inner, state);
        }
    }
}

fn render_split_visualizer(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let bands = &state.bands;
    if area.height >= 9 {
        let scope_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);
        render_spectrum_bars(frame, scope_rows[0], bands);
        render_waveform_trace(frame, scope_rows[1], &state.waveform);
        render_audio_progress_row(frame, scope_rows[2], state);
    } else if area.height >= 5 {
        let scope_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);
        render_spectrum_bars(frame, scope_rows[0], bands);
        render_audio_progress_row(frame, scope_rows[1], state);
    } else {
        render_spectrum_bars(frame, area, bands);
    }
}

fn render_wave_visualizer(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    if area.height >= 5 {
        let scope_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);
        render_waveform_trace(frame, scope_rows[0], &state.waveform);
        render_audio_progress_row(frame, scope_rows[1], state);
    } else {
        render_waveform_trace(frame, area, &state.waveform);
    }
}

fn render_web_visualizer_lines(
    frame: &mut Frame<'_>,
    area: Rect,
    visualizer_lines: Vec<WebVisualizerLine>,
) {
    if visualizer_lines.is_empty() {
        return;
    }

    let lines = visualizer_lines
        .into_iter()
        .map(|line| {
            Line::from(
                line.segments
                    .into_iter()
                    .map(|segment| {
                        Span::styled(
                            segment.text,
                            web_visualizer_segment_style(segment.kind, segment.row_bottom),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        area,
    );
}

fn web_visualizer_segment_style(kind: WebVisualizerSegmentKind, row_bottom: f64) -> Style {
    match kind {
        WebVisualizerSegmentKind::Gradient => spectrum_style(row_bottom),
        WebVisualizerSegmentKind::RetroGrid => {
            Style::default().fg(Classic808Palette::AMBER.ratatui())
        }
        WebVisualizerSegmentKind::RetroSun => Style::default().fg(mix_rgb(
            Classic808Palette::YELLOW.ratatui(),
            Classic808Palette::ORANGE.ratatui(),
            (1.0 - row_bottom) as f32,
        )),
        WebVisualizerSegmentKind::RetroWave => Style::default()
            .fg(Classic808Palette::RED_TEXT.ratatui())
            .add_modifier(Modifier::BOLD),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct WebVisualizerLine {
    segments: Vec<WebVisualizerSegment>,
}

impl WebVisualizerLine {
    #[cfg(test)]
    fn cell_width(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.text.chars().count())
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct WebVisualizerSegment {
    text: String,
    row_bottom: f64,
    kind: WebVisualizerSegmentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebVisualizerSegmentKind {
    Gradient,
    RetroGrid,
    RetroSun,
    RetroWave,
}

fn render_logo_visualizer_lines(
    bands: &[f32],
    width: u16,
    height: u16,
    frame: u64,
    animate: bool,
) -> Vec<WebVisualizerLine> {
    let width = usize::from(width);
    let height = usize::from(height);
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let bands = web_visualizer_bands_10(bands);
    let dot_rows = height * 4;
    let dot_cols = width * 2;
    let mut pixels = vec![vec![false; dot_cols]; dot_rows];

    let scale_x = (dot_cols / LOGO_TOTAL_W).max(1);
    let scale_y = ((dot_rows.saturating_mul(4) / 5) / LOGO_GLYPH_H).max(1);
    let rendered_w = LOGO_TOTAL_W * scale_x;
    let rendered_h = LOGO_GLYPH_H * scale_y;
    let offset_x = dot_cols.saturating_sub(rendered_w) / 2;
    let base_offset_y = dot_rows.saturating_sub(rendered_h) / 2;
    let animation_frame = if animate { frame } else { 0 };

    for (glyph_idx, glyph) in LOGO_GLYPHS.iter().enumerate() {
        let energy = bands[LOGO_BAND_MAP[glyph_idx]].clamp(0.0, 1.0);
        let wave = (animation_frame as f64 * 0.085 + glyph_idx as f64 * 0.78).sin() * 2.4;
        let bounce = (energy as f64 * base_offset_y as f64 * 0.5 + wave).round() as isize;
        let letter_x = offset_x + glyph_idx * (LOGO_GLYPH_W + LOGO_GLYPH_GAP) * scale_x;
        let letter_y = base_offset_y as isize - bounce;

        for (py, row_bits) in glyph.iter().enumerate() {
            for px in 0..LOGO_GLYPH_W {
                if row_bits & (1 << (LOGO_GLYPH_W - 1 - px)) == 0 {
                    continue;
                }

                let fill = (energy * energy * 0.58 + 0.32).clamp(0.0, 0.9);
                for sy in 0..scale_y {
                    for sx in 0..scale_x {
                        let dx = letter_x + px * scale_x + sx;
                        let dy = letter_y + (py * scale_y + sy) as isize;
                        if dx >= dot_cols || dy < 0 || dy as usize >= dot_rows {
                            continue;
                        }
                        if scatter_hash(
                            glyph_idx,
                            py * scale_y + sy,
                            px * scale_x + sx,
                            animation_frame,
                        ) > f64::from(fill)
                        {
                            continue;
                        }
                        pixels[dy as usize][dx] = true;
                    }
                }
            }
        }
    }

    braille_lines_from_pixels(width, height, &pixels, WebVisualizerSegmentKind::Gradient)
}

fn render_retro_visualizer_lines(
    bands: &[f32],
    width: u16,
    height: u16,
    frame: u64,
    animate: bool,
) -> Vec<WebVisualizerLine> {
    let width = usize::from(width);
    let height = usize::from(height);
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let bands = web_visualizer_bands_10(bands);
    let dot_rows = height * 4;
    let dot_cols = width * 2;
    let mut horizon_dot = (dot_rows * 2) / 5;
    horizon_dot = horizon_dot.max(2).min(dot_rows.saturating_sub(1));
    let floor_rows = dot_rows.saturating_sub(horizon_dot);
    let center_x = dot_cols.saturating_sub(1) as f64 / 2.0;
    let mut grid = vec![0u8; dot_rows * dot_cols];
    let animation_frame = if animate { frame } else { 0 };

    let sun_radius = horizon_dot as f64 * 0.85;
    for dy in 0..horizon_dot {
        let row_dist = (horizon_dot - dy) as f64;
        if row_dist > sun_radius {
            continue;
        }

        let half_width = (sun_radius * sun_radius - row_dist * row_dist).sqrt();
        if row_dist < sun_radius * 0.5 {
            let stripe_width = ((sun_radius * 0.15) as usize).max(1);
            if ((row_dist as usize) / stripe_width) % 2 == 1 {
                continue;
            }
        }

        let left = (center_x - half_width).max(0.0) as usize;
        let right = (center_x + half_width).min(dot_cols.saturating_sub(1) as f64) as usize;
        let offset = dy * dot_cols;
        for dx in left..=right {
            grid[offset + dx] = 3;
        }
    }

    if horizon_dot < dot_rows {
        let offset = horizon_dot * dot_cols;
        grid[offset..offset + dot_cols].fill(1);
    }

    const NUM_VERTICAL_LINES: usize = 18;
    for i in 0..=NUM_VERTICAL_LINES {
        let bottom_x = i as f64 * dot_cols.saturating_sub(1) as f64 / NUM_VERTICAL_LINES as f64;
        for dy in (horizon_dot + 1)..dot_rows {
            let t = (dy - horizon_dot) as f64 / floor_rows.saturating_sub(1).max(1) as f64;
            let screen_x = center_x + (bottom_x - center_x) * t;
            let ix = screen_x.round() as usize;
            if ix < dot_cols {
                grid[dy * dot_cols + ix] = 1;
            }
        }
    }

    let scroll = (animation_frame as f64 * 0.08).fract();
    const NUM_HORIZONTAL_LINES: usize = 10;
    for i in 0..NUM_HORIZONTAL_LINES {
        let mut z = (i as f64 + scroll) / NUM_HORIZONTAL_LINES as f64;
        if z > 1.0 {
            z -= 1.0;
        }
        let dy = horizon_dot + 1 + (z * z * floor_rows.saturating_sub(2).max(1) as f64) as usize;
        if dy > horizon_dot && dy < dot_rows {
            let offset = dy * dot_cols;
            grid[offset..offset + dot_cols].fill(1);
        }
    }

    let max_wave = horizon_dot as f64 * 0.85;
    let mut wave_y = vec![horizon_dot.min(dot_rows.saturating_sub(1)); dot_cols];
    for (dx, y_slot) in wave_y.iter_mut().enumerate() {
        let band_f = dx as f64 / dot_cols.saturating_sub(1).max(1) as f64
            * (WEB_VISUALIZER_BANDS - 1) as f64;
        let band_idx = band_f.floor() as usize;
        let frac = band_f - band_idx as f64;
        let interp = (1.0 - (frac * std::f64::consts::PI).cos()) / 2.0;
        let level = if band_idx >= WEB_VISUALIZER_BANDS - 1 {
            bands[WEB_VISUALIZER_BANDS - 1]
        } else {
            bands[band_idx] * (1.0 - interp as f32) + bands[band_idx + 1] * interp as f32
        }
        .max(0.03);
        let wave_dot = horizon_dot.saturating_sub((f64::from(level) * max_wave) as usize);
        *y_slot = wave_dot.min(dot_rows.saturating_sub(1));
    }

    for (dx, &y) in wave_y.iter().enumerate() {
        grid[y * dot_cols + dx] = 2;
        if dx > 0 {
            let previous = wave_y[dx - 1];
            for fy in previous.min(y)..=previous.max(y) {
                grid[fy * dot_cols + dx] = 2;
            }
        }
    }

    let mut lines = Vec::with_capacity(height);
    for row in 0..height {
        let row_bottom = (height - 1 - row) as f64 / height as f64;
        let base = row * 4;
        let mut segments = Vec::new();
        let mut current_kind: Option<WebVisualizerSegmentKind> = None;
        let mut run = String::with_capacity(width);

        for ch in 0..width {
            let mut bits = 0u8;
            let mut has_wave = false;
            let mut has_sun = false;
            let col_base = ch * 2;

            for dr in 0..4 {
                for dc in 0..2 {
                    let tag = grid[(base + dr) * dot_cols + (col_base + dc)];
                    if tag == 0 {
                        continue;
                    }
                    bits |= braille_bit(dr, dc);
                    if tag == 2 {
                        has_wave = true;
                    } else if tag == 3 {
                        has_sun = true;
                    }
                }
            }

            let kind = if has_wave {
                WebVisualizerSegmentKind::RetroWave
            } else if has_sun {
                WebVisualizerSegmentKind::RetroSun
            } else {
                WebVisualizerSegmentKind::RetroGrid
            };

            if current_kind != Some(kind) && !run.is_empty() {
                segments.push(WebVisualizerSegment {
                    text: std::mem::take(&mut run),
                    row_bottom,
                    kind: current_kind.unwrap_or(WebVisualizerSegmentKind::RetroGrid),
                });
            }
            current_kind = Some(kind);
            run.push(char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' '));
        }

        if !run.is_empty() {
            segments.push(WebVisualizerSegment {
                text: run,
                row_bottom,
                kind: current_kind.unwrap_or(WebVisualizerSegmentKind::RetroGrid),
            });
        }

        lines.push(WebVisualizerLine { segments });
    }

    lines
}

fn web_visualizer_bands_10(bands: &[f32]) -> [f32; WEB_VISUALIZER_BANDS] {
    let mut output = [0.0; WEB_VISUALIZER_BANDS];
    let sampled = resample_f32(bands, WEB_VISUALIZER_BANDS);
    for (slot, level) in output.iter_mut().zip(sampled) {
        *slot = level.clamp(0.0, 1.0);
    }
    output
}

fn scatter_hash(band: usize, row: usize, col: usize, frame: u64) -> f64 {
    let stagger = (row * 3 + col) as u64;
    let frame_bucket = (frame + stagger) / 3;
    let mut hash = (band as u64) * 7_919
        + (row as u64) * 6_271
        + (col as u64) * 3_037
        + frame_bucket * 104_729;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x45d9_f3b3_7197_344b);
    hash ^= hash >> 16;
    (hash % 10_000) as f64 / 10_000.0
}

fn braille_bit(dot_row: usize, dot_col: usize) -> u8 {
    match (dot_row, dot_col) {
        (0, 0) => 0x01,
        (1, 0) => 0x02,
        (2, 0) => 0x04,
        (3, 0) => 0x40,
        (0, 1) => 0x08,
        (1, 1) => 0x10,
        (2, 1) => 0x20,
        (3, 1) => 0x80,
        _ => 0,
    }
}

fn braille_lines_from_pixels(
    width: usize,
    height: usize,
    pixels: &[Vec<bool>],
    kind: WebVisualizerSegmentKind,
) -> Vec<WebVisualizerLine> {
    let mut lines = Vec::with_capacity(height);
    for cell_y in 0..height {
        let mut line = String::with_capacity(width);
        for cell_x in 0..width {
            let x = cell_x * 2;
            let y = cell_y * 4;
            let mut bits = 0u8;

            for dot_row in 0..4 {
                for dot_col in 0..2 {
                    if pixels[y + dot_row][x + dot_col] {
                        bits |= braille_bit(dot_row, dot_col);
                    }
                }
            }

            if bits == 0 {
                line.push(' ');
            } else {
                line.push(char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' '));
            }
        }

        lines.push(WebVisualizerLine {
            segments: vec![WebVisualizerSegment {
                text: line,
                row_bottom: (height - 1 - cell_y) as f64 / height as f64,
                kind,
            }],
        });
    }
    lines
}

fn render_spectrum_bars(frame: &mut Frame<'_>, area: Rect, bands: &[f32]) {
    if area.height == 0 || area.width < 2 {
        return;
    }

    let bands = analyser_bands_for_scope_width(bands, area.width);
    if bands.is_empty() {
        return;
    }

    let heights = analyser_bands_to_heights(&bands, area.height);
    let mut lines = Vec::with_capacity(usize::from(area.height));

    for row in (1..=area.height).rev() {
        let row_ratio = f64::from(row) / f64::from(area.height.max(1));
        let style = spectrum_style(row_ratio);
        let mut spans = Vec::with_capacity(heights.len());
        for height in &heights {
            let cell = if *height >= row { "##" } else { "  " };
            spans.push(Span::styled(cell, style));
        }
        lines.push(Line::from(spans));
    }

    let visualizer = Paragraph::new(Text::from(lines)).alignment(Alignment::Center);
    frame.render_widget(visualizer, area);
}

fn render_waveform_trace(frame: &mut Frame<'_>, area: Rect, waveform: &[f32]) {
    if area.height < 2 || area.width < 4 || waveform.is_empty() {
        return;
    }

    let x_half = (area.width as f64 / 2.0).max(4.0);
    let y_half = 1.25;
    let samples = resample_f32(waveform, usize::from(area.width.max(1)));
    let canvas = Canvas::default()
        .x_bounds([-x_half, x_half])
        .y_bounds([-y_half, y_half])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            if samples.len() < 2 {
                return;
            }

            let last = samples.len() - 1;
            for index in 0..last {
                let x1 = -x_half + (index as f64 / last as f64) * x_half * 2.0;
                let x2 = -x_half + ((index + 1) as f64 / last as f64) * x_half * 2.0;
                let y1 = f64::from(samples[index]).clamp(-1.0, 1.0);
                let y2 = f64::from(samples[index + 1]).clamp(-1.0, 1.0);
                let energy =
                    ((samples[index].abs() + samples[index + 1].abs()) / 2.0).clamp(0.0, 1.0);
                ctx.draw(&CanvasLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: mix_rgb(
                        Classic808Palette::AMBER.ratatui(),
                        Classic808Palette::YELLOW.ratatui(),
                        energy,
                    ),
                });
            }
        });

    frame.render_widget(canvas, area);
}

fn render_audio_progress_row(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    if area.width < 12 || area.height == 0 {
        return;
    }

    let fraction = playback_progress_fraction(state.current_time, state.duration);
    let bar_width = usize::from(area.width.saturating_sub(18)).clamp(4, 42);
    let filled = (fraction * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);
    let line = Line::from(vec![
        Span::styled("TIME ", classic_small_label_style()),
        Span::styled(
            format!(
                "{:<13}",
                format_time_status(state.current_time, state.duration)
            ),
            classic_value_style(),
        ),
        Span::styled(
            " ".repeat(filled),
            Style::default().bg(Classic808Palette::YELLOW.ratatui()),
        ),
        Span::styled(
            " ".repeat(empty),
            Style::default().bg(Classic808Palette::OLIVE.ratatui()),
        ),
    ]);

    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn playback_progress_fraction(current_time: f64, duration: Option<f64>) -> f64 {
    let Some(duration) = duration.filter(|duration| duration.is_finite() && *duration > 0.0) else {
        return 0.0;
    };
    if !current_time.is_finite() {
        return 0.0;
    }
    (current_time.max(0.0) / duration).clamp(0.0, 1.0)
}

fn web_step_chase_index(state: &WebAppState) -> Option<usize> {
    if state.transport != TransportState::Playing || !state.current_time.is_finite() {
        return None;
    }

    let current_time = state.current_time.max(0.0);
    if let Some(bpm) = state.bpm.provisional_bpm().filter(|bpm| *bpm > 0) {
        let step_seconds = 60.0 / f64::from(bpm) / 4.0;
        if step_seconds.is_finite() && step_seconds > 0.0 {
            return Some(((current_time / step_seconds).floor() as usize) % WEB_808_STEP_COUNT);
        }
    }

    state
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| {
            let progress = playback_progress_fraction(current_time, Some(duration));
            ((progress * WEB_808_STEP_COUNT as f64).floor() as usize).min(WEB_808_STEP_COUNT - 1)
        })
}

fn render_analyser_empty_state(
    frame: &mut Frame<'_>,
    area: Rect,
    presentation: AnalyserEmptyPresentation,
    transport: TransportState,
) {
    let spacer_count = area.height.saturating_sub(3) / 2;
    let mut lines = Vec::with_capacity(usize::from(spacer_count) + 3);
    for _ in 0..spacer_count {
        lines.push(Line::from(""));
    }

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

    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        area,
    );
}

fn render_step_strip(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &WebAppState,
    fx: &mut WebFxRuntime,
    tick: tachyonfx::Duration,
) {
    let spec = web_panel_spec(PanelRole::Steps, state);
    let effect = state
        .motion_enabled
        .then(|| fx.panel_effect(spec))
        .flatten();
    let inner = render_808_panel(frame, area, spec, effect, tick);
    let bands = &state.bands;

    if inner.width < 32 {
        return;
    }

    let step_count = (usize::from(inner.width) / 6).clamp(1, 16);
    let active_chase_step = web_step_chase_index(state);
    let mut numbers = Vec::with_capacity(step_count);
    let mut pads = Vec::with_capacity(step_count);
    for step in 0..step_count {
        numbers.push(Span::styled(
            format!("{:^6}", step + 1),
            classic_small_label_style(),
        ));
        let energy = bands.get(step).copied().unwrap_or_default();
        let glow =
            step_chase_glow_intensity(state.transport, energy, active_chase_step == Some(step));
        pads.push(Span::styled(
            format!("{:^6}", step + 1),
            classic_step_keycap_style(classic_pad_family(step), glow),
        ));
    }

    let lamp_glow = bands
        .iter()
        .copied()
        .map(|energy| step_glow_intensity(state.transport, energy))
        .fold(0.0, f32::max);

    let lines = vec![
        Line::from(numbers),
        Line::from(pads),
        Line::from(vec![
            Span::styled("START / STOP  ", classic_small_label_style()),
            Span::styled("TAP  ", classic_pad_style(ClassicPadFamily::Ivory)),
            Span::styled("A", active_lamp_style(lamp_glow)),
            Span::styled(" B", inactive_lamp_style()),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        inner,
    );
}

fn transport_color(transport: TransportState) -> Color {
    match transport {
        TransportState::Playing => Classic808Palette::YELLOW.ratatui(),
        TransportState::Ready | TransportState::Paused => Classic808Palette::AMBER.ratatui(),
        TransportState::Error => Classic808Palette::RED_TEXT.ratatui(),
        TransportState::Idle | TransportState::Ended => Classic808Palette::GREY.ratatui(),
    }
}

fn spectrum_style(row_ratio: f64) -> Style {
    let color = if row_ratio > 0.66 {
        Classic808Palette::RED.ratatui()
    } else if row_ratio > 0.33 {
        Classic808Palette::AMBER.ratatui()
    } else {
        Classic808Palette::YELLOW.ratatui()
    };
    Style::default().fg(color)
}

fn step_glow_intensity(transport: TransportState, energy: f32) -> f32 {
    if transport != TransportState::Playing {
        return 0.0;
    }
    ((energy - 0.08) / 0.84).clamp(0.0, 1.0)
}

fn step_chase_glow_intensity(transport: TransportState, energy: f32, is_chase_active: bool) -> f32 {
    let analyser_glow = step_glow_intensity(transport, energy);
    if transport != TransportState::Playing || !is_chase_active {
        return analyser_glow;
    }
    analyser_glow.max(0.72)
}

fn classic_faceplate_style() -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .bg(Classic808Palette::FACEPLATE.ratatui())
}

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

fn classic_label_style() -> Style {
    Style::default()
        .fg(Classic808Palette::YELLOW.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn classic_small_label_style() -> Style {
    Style::default().fg(Classic808Palette::LABEL.ratatui())
}

fn classic_value_style() -> Style {
    Style::default().fg(Classic808Palette::IVORY.ratatui())
}

fn classic_knob_style() -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn instrument_strip_style(family: ClassicPadFamily) -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .bg(instrument_family_bg(family))
}

fn instrument_parameter_style(family: ClassicPadFamily) -> Style {
    Style::default()
        .fg(instrument_family_fg(family))
        .add_modifier(Modifier::BOLD)
}

fn instrument_family_fg(family: ClassicPadFamily) -> Color {
    match family {
        ClassicPadFamily::Red => Classic808Palette::RED_TEXT.ratatui(),
        ClassicPadFamily::Orange => Classic808Palette::ORANGE.ratatui(),
        ClassicPadFamily::Yellow => Classic808Palette::YELLOW.ratatui(),
        ClassicPadFamily::Ivory => Classic808Palette::IVORY.ratatui(),
    }
}

fn instrument_family_bg(family: ClassicPadFamily) -> Color {
    match family {
        ClassicPadFamily::Red => Color::Rgb(0x4c, 0x15, 0x12),
        ClassicPadFamily::Orange => Color::Rgb(0x4a, 0x2a, 0x0e),
        ClassicPadFamily::Yellow => Color::Rgb(0x4b, 0x42, 0x10),
        ClassicPadFamily::Ivory => Classic808Palette::OLIVE.ratatui(),
    }
}

fn active_lamp_style(glow: f32) -> Style {
    let glow = glow.clamp(0.0, 1.0);
    Style::default()
        .fg(mix_rgb(
            Classic808Palette::RED_TEXT.ratatui(),
            Classic808Palette::YELLOW.ratatui(),
            glow * 0.45,
        ))
        .add_modifier(Modifier::BOLD)
}

fn inactive_lamp_style() -> Style {
    Style::default().fg(Classic808Palette::DIM.ratatui())
}

fn classic_pad_style(family: ClassicPadFamily) -> Style {
    Style::default()
        .fg(classic_pad_color(family, true))
        .add_modifier(Modifier::BOLD)
}

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

fn classic_pad_color(family: ClassicPadFamily, active: bool) -> Color {
    match (family, active) {
        (ClassicPadFamily::Red, true) => Classic808Palette::RED.ratatui(),
        (ClassicPadFamily::Orange, true) => Classic808Palette::ORANGE.ratatui(),
        (ClassicPadFamily::Yellow, true) => Classic808Palette::YELLOW.ratatui(),
        (ClassicPadFamily::Ivory, true) => Classic808Palette::IVORY.ratatui(),
        (ClassicPadFamily::Red, false) => Color::Rgb(0xa4, 0x21, 0x1a),
        (ClassicPadFamily::Orange, false) => Color::Rgb(0xb8, 0x56, 0x19),
        (ClassicPadFamily::Yellow, false) => Color::Rgb(0xa4, 0xaa, 0x24),
        (ClassicPadFamily::Ivory, false) => Color::Rgb(0xb4, 0xae, 0x92),
    }
}

fn format_time_status(current_time: f64, duration: Option<f64>) -> String {
    match duration {
        Some(duration) => format!(
            "{} / {}",
            format_seconds(current_time),
            format_seconds(duration)
        ),
        None => format!("{} / --:--", format_seconds(current_time)),
    }
}

fn format_seconds(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--".to_string();
    }

    let total_seconds = seconds.round() as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn finite_duration(audio: &HtmlAudioElement) -> Option<f64> {
    let duration = audio.duration();
    duration.is_finite().then_some(duration)
}

fn motion_status_text(enabled: bool) -> &'static str {
    if enabled {
        "Motion effects enabled"
    } else {
        "Reduced motion enabled"
    }
}

fn sync_controls(button: &HtmlButtonElement, status: &HtmlElement, state: &WebAppState) {
    button.set_disabled(state.source.is_none());
    let label = if state.transport == TransportState::Playing {
        "Pause"
    } else {
        "Play"
    };
    button.set_text_content(Some(label));
    status.set_text_content(Some(state.error.as_deref().unwrap_or(&state.status)));
}

fn set_error(
    state: &Rc<RefCell<WebAppState>>,
    button: &HtmlButtonElement,
    status: &HtmlElement,
    message: String,
) {
    let mut state = state.borrow_mut();
    state.transport = TransportState::Error;
    state.error = Some(message);
    state.bands = vec![0.0; BAND_COUNT];
    state.waveform = vec![0.0; WAVEFORM_SAMPLE_COUNT];
    state.bpm = WebBpmState::unavailable();
    state.last_bpm_sample_ms = None;
    sync_controls(button, status, &state);
}

fn revoke_object_url(object_url: &Rc<RefCell<Option<String>>>) {
    if let Some(url) = object_url.borrow_mut().take() {
        let _ = Url::revoke_object_url(&url);
    }
}

fn element_by_id<T>(document: &Document, id: &str) -> Result<T, JsValue>
where
    T: JsCast,
{
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("missing #{id}")))?
        .dyn_into()
        .map_err(|_| JsValue::from_str(&format!("invalid element type for #{id}")))
}

fn add_event_listener(
    target: &EventTarget,
    name: &'static str,
    handler: impl FnMut(Event) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::<dyn FnMut(Event)>::wrap(Box::new(handler));
    target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

fn js_to_io_error(error: JsValue) -> io::Error {
    io::Error::other(format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::{
        analyser_bands_for_scope_width, analyser_empty_state_presentation,
        analyser_empty_state_text, browser_media_error_message, classic_body_border_style,
        classic_hardware_body_style, classic_pad_family, classic_panel_inset_style,
        classic_step_keycap_style, classic_step_keycap_text_color, contrast_ratio,
        hardware_body_text_style, hardware_brand_style, hosted_recent_urls,
        instrument_channel_visible_count, instrument_control_specs, instrument_family_bg,
        instrument_family_fg, instrument_label_cap_lines, instrument_label_cap_text,
        knob_canvas_bounds_808, machine_brand_label, machine_header_can_show_logo,
        machine_header_height, machine_header_identity_effect_area, machine_logo_area,
        machine_logo_mark, machine_logo_pixel_size, machine_logo_style, playback_progress_fraction,
        recent_source_display_label, remember_recent_source, render_logo_visualizer_lines,
        render_retro_visualizer_lines, step_chase_glow_intensity, step_glow_intensity,
        tempo_dial_geometry_808, tempo_tick_color_808, waveform_bytes_to_samples,
        web_action_for_key, web_audio_control_at_cell, web_audio_control_fx_signature,
        web_audio_control_hit_zones, web_audio_control_value, web_audio_controls_after_action,
        web_audio_drop_error, web_audio_gain_from_db, web_audio_selected_control_readout,
        web_compact_deck_layout, web_desktop_body_layout, web_desktop_deck_layout,
        web_eq_band_to_normalized, web_eq_preset_label, web_focus_after_action, web_fx_tick_ms,
        web_header_fx_signature, web_header_identity_fx_signature, web_motion_enabled_after_action,
        web_panel_border_set, web_panel_fx_signature, web_panel_spec,
        web_persisted_settings_from_json, web_persisted_settings_from_state,
        web_persisted_settings_to_json, web_pointer_cell_from_canvas_offset,
        web_restore_persisted_settings, web_seek_target_seconds, web_step_chase_index,
        web_tempo_display, web_transition_fx_signature, web_visual_mode_after_action,
        web_visual_mode_label, web_volume_to_normalized, Classic808Palette, ClassicColor,
        ClassicPadFamily, PanelRole, PanelState, TransportState, WebAction, WebAppState,
        WebAudioControlHitZone, WebAudioControls, WebAudioSource, WebFocus, WebPersistedSettings,
        WebVisualMode, INSTRUMENT_CHANNEL_FULL_HEIGHT, WEB_EQ_MAX_DB, WEB_EQ_MIN_DB,
        WEB_EQ_PRESETS, WEB_VOLUME_MAX_DB,
    };
    use amp808_core::web_audio::WebBpmState;
    use ratzilla::ratatui::{
        layout::Rect,
        style::{Color, Modifier},
    };
    use tui_big_text::PixelSize;

    #[test]
    fn classic_pad_family_matches_tr_808_step_groups() {
        let families = (0..16).map(classic_pad_family).collect::<Vec<_>>();

        assert_eq!(
            families,
            vec![
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Ivory,
                ClassicPadFamily::Ivory,
                ClassicPadFamily::Ivory,
                ClassicPadFamily::Ivory,
            ]
        );
    }

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

    #[test]
    fn classic_palette_keeps_normal_text_at_aa_contrast() {
        let faceplate = Classic808Palette::FACEPLATE;
        let normal_text = [
            ("ivory", Classic808Palette::IVORY),
            ("orange", Classic808Palette::ORANGE),
            ("amber", Classic808Palette::AMBER),
            ("yellow", Classic808Palette::YELLOW),
            ("grey", Classic808Palette::GREY),
            ("label", Classic808Palette::LABEL),
        ];

        for (name, color) in normal_text {
            assert!(
                contrast_ratio(color, faceplate) >= 4.5,
                "{name} should pass AA contrast on the 808 faceplate"
            );
        }

        assert!(
            contrast_ratio(Classic808Palette::RED, faceplate) < 4.5,
            "hardware red should stay reserved for lamps/buttons, not normal text"
        );
    }

    #[test]
    fn classic_palette_keeps_controls_and_borders_at_aa_contrast() {
        let faceplate = Classic808Palette::FACEPLATE;
        let pairs = [
            ("orange border", Classic808Palette::ORANGE, faceplate),
            ("error text", Classic808Palette::RED_TEXT, faceplate),
            ("orange button text", faceplate, Classic808Palette::ORANGE),
            ("yellow button text", faceplate, Classic808Palette::YELLOW),
            ("ivory button text", faceplate, Classic808Palette::IVORY),
        ];

        for (name, foreground, background) in pairs {
            assert!(
                contrast_ratio(foreground, background) >= 4.5,
                "{name} should pass AA contrast"
            );
        }
    }

    #[test]
    fn hardware_body_palette_keeps_text_and_brand_readable() {
        assert_ne!(
            Classic808Palette::BODY,
            Classic808Palette::IVORY,
            "large hardware body fill should not use the bright ivory label color"
        );
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

    #[test]
    fn instrument_family_colors_keep_strip_labels_readable() {
        for family in [
            ClassicPadFamily::Red,
            ClassicPadFamily::Orange,
            ClassicPadFamily::Yellow,
            ClassicPadFamily::Ivory,
        ] {
            assert!(
                contrast_ratio(
                    Classic808Palette::IVORY,
                    color_to_classic(instrument_family_bg(family))
                ) >= 4.5,
                "{family:?} strip label should pass AA contrast"
            );
            assert!(
                contrast_ratio(
                    color_to_classic(instrument_family_fg(family)),
                    Classic808Palette::FACEPLATE
                ) >= 4.5,
                "{family:?} parameter label should pass AA contrast"
            );
        }
    }

    #[test]
    fn instrument_knob_canvas_bounds_tighten_wide_slots_toward_round_dials() {
        let area = Rect::new(0, 0, 12, 4);
        let (x_bounds, y_bounds) = knob_canvas_bounds_808(area);
        let x_span = x_bounds[1] - x_bounds[0];
        let y_span = y_bounds[1] - y_bounds[0];
        let rendered_aspect = area.width as f64 * y_span / (area.height as f64 * x_span);

        assert!(
            (1.15..=1.55).contains(&rendered_aspect),
            "instrument knobs should render rounder than a wide arch; got aspect {rendered_aspect:.2}"
        );
    }

    #[test]
    fn tempo_dial_geometry_tightens_left_panel_gauge_toward_rounder_footprint() {
        let area = Rect::new(0, 0, 26, 18);
        let geometry = tempo_dial_geometry_808(area);
        let x_span = geometry.x_bounds[1] - geometry.x_bounds[0];
        let y_span = geometry.y_bounds[1] - geometry.y_bounds[0];
        let rendered_aspect = area.width as f64 * y_span / (area.height as f64 * x_span);

        assert!(
            (1.15..=1.55).contains(&rendered_aspect),
            "tempo gauge should render rounder than a wide arch; got aspect {rendered_aspect:.2}"
        );
        assert!(
            geometry.radius >= 5.0,
            "tempo gauge should stay bold enough to fill its panel; got radius {:.2}",
            geometry.radius
        );
    }

    #[test]
    fn tempo_tick_marks_use_readable_grey_on_faceplate() {
        assert!(
            contrast_ratio(
                color_to_classic(tempo_tick_color_808()),
                Classic808Palette::FACEPLATE,
            ) >= 4.5,
            "tempo gauge tick marks should be visible on the black faceplate"
        );
    }

    #[test]
    fn machine_brand_label_uses_tr_808_model_identity() {
        assert_eq!(
            machine_brand_label(),
            "Machine Controlled Rhythm Composer TR-808 WEB"
        );
    }

    #[test]
    fn machine_logo_mark_uses_large_808_identity() {
        assert_eq!(machine_logo_mark(), "808");
        assert_eq!(
            machine_logo_style().fg,
            Some(Classic808Palette::BRAND_ORANGE.ratatui())
        );
        assert!(machine_logo_style().add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn machine_header_shows_left_logo_only_when_brand_and_source_have_room() {
        let source = "01. Valentino Kanzyani - Nueva York.mp3";

        assert!(machine_header_can_show_logo(160, source));
        assert!(!machine_header_can_show_logo(56, source));
    }

    #[test]
    fn machine_logo_area_sits_in_top_left_corner_of_main_header() {
        let header = Rect::new(0, 0, 160, machine_header_height());
        let logo = machine_logo_area(header, "01. Valentino Kanzyani - Nueva York.mp3")
            .expect("wide header should expose logo area");

        assert_eq!(logo.x, header.x);
        assert_eq!(logo.y, header.y);
        assert!(logo.x + logo.width < header.x + header.width);
        assert_eq!(machine_logo_pixel_size(), PixelSize::HalfWidth);
        assert!(machine_header_height() >= 8);
        assert!(logo.width <= 18);
        assert_eq!(logo.height, machine_header_height());
    }

    #[test]
    fn machine_header_identity_effect_area_includes_big_logo_mark() {
        let header = Rect::new(0, 0, 160, machine_header_height());
        let logo = machine_logo_area(header, "01. Valentino Kanzyani - Nueva York.mp3")
            .expect("wide header should expose logo area");
        let identity =
            machine_header_identity_effect_area(header, "01. Valentino Kanzyani - Nueva York.mp3");

        assert_eq!(identity.x, logo.x);
        assert_eq!(identity.y, header.y);
        assert!(identity.width >= logo.width);
        assert_eq!(identity.height, header.height);
    }

    #[test]
    fn web_tempo_display_maps_bpm_state_to_dial_label_and_position() {
        let mut locked_state = WebBpmState::estimating();
        feed_synthetic_bpm_frames(&mut locked_state, 180);
        let locked = web_tempo_display(&locked_state);
        assert_eq!(locked.label, "120");
        assert!((locked.normalized - 0.4167).abs() < 0.01);

        let mut estimating_state = WebBpmState::estimating();
        feed_synthetic_bpm_frames(&mut estimating_state, 48);
        let estimating = web_tempo_display(&estimating_state);
        assert_eq!(estimating.label, "~120");
        assert!((estimating.normalized - 0.4167).abs() < 0.01);

        let unavailable_state = WebBpmState::unavailable();
        let unavailable = web_tempo_display(&unavailable_state);
        assert_eq!(unavailable.label, "--");
        assert_eq!(unavailable.normalized, 0.5);
    }

    fn feed_synthetic_bpm_frames(bpm: &mut WebBpmState, frame_count: usize) {
        for frame in 0..frame_count {
            let on_beat = frame % 10 == 0;
            let byte = if on_beat { 255 } else { 128 };
            bpm.update_from_time_domain_bytes(&vec![byte; 512], 0.05, true);
        }
    }

    fn color_to_classic(color: Color) -> ClassicColor {
        match color {
            Color::Rgb(red, green, blue) => ClassicColor { red, green, blue },
            other => panic!("expected RGB color, got {other:?}"),
        }
    }

    #[test]
    fn web_panel_specs_make_transport_state_visible() {
        let mut state = WebAppState {
            transport: TransportState::Playing,
            ..WebAppState::default()
        };

        assert_eq!(
            web_panel_spec(PanelRole::Transport, &state).state,
            PanelState::Active
        );
        assert_eq!(
            web_panel_spec(PanelRole::Analyser, &state).state,
            PanelState::Active
        );
        assert_eq!(
            web_panel_spec(PanelRole::Instrument, &state).state,
            PanelState::Armed
        );

        state.transport = TransportState::Error;
        state.error = Some("CORS blocked".to_string());

        assert_eq!(
            web_panel_spec(PanelRole::Transport, &state).state,
            PanelState::Error
        );
        assert_eq!(
            web_panel_spec(PanelRole::Analyser, &state).state,
            PanelState::Error
        );
    }

    #[test]
    fn web_panel_specs_reflect_terminal_focus() {
        let mut state = WebAppState {
            focus: WebFocus::HostedUrl,
            ..WebAppState::default()
        };

        assert_eq!(
            web_panel_spec(PanelRole::Transport, &state).state,
            PanelState::Active
        );
        assert_eq!(
            web_panel_spec(PanelRole::Analyser, &state).state,
            PanelState::Idle
        );

        state.focus = WebFocus::Analyser;
        assert_eq!(
            web_panel_spec(PanelRole::Analyser, &state).state,
            PanelState::Active
        );
    }

    #[test]
    fn recent_sources_are_newest_first_deduped_and_capped() {
        let mut recent = Vec::new();

        for label in ["one.mp3", "two.mp3", "three.mp3", "four.mp3"] {
            remember_recent_source(&mut recent, WebAudioSource::local_file(label));
        }
        remember_recent_source(
            &mut recent,
            WebAudioSource::hosted_url("https://example.com/a.mp3"),
        );
        remember_recent_source(&mut recent, WebAudioSource::local_file("two.mp3"));

        let labels = recent.iter().map(WebAudioSource::label).collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec![
                "two.mp3",
                "https://example.com/a.mp3",
                "four.mp3",
                "three.mp3"
            ]
        );
    }

    #[test]
    fn hosted_recent_urls_exclude_browser_selected_local_files() {
        let recent = vec![
            WebAudioSource::local_file("break.wav"),
            WebAudioSource::hosted_url("https://example.com/a.mp3"),
            WebAudioSource::local_file("hat.wav"),
            WebAudioSource::hosted_url("https://cdn.example.com/b.ogg"),
        ];

        assert_eq!(
            hosted_recent_urls(&recent),
            vec![
                "https://example.com/a.mp3".to_string(),
                "https://cdn.example.com/b.ogg".to_string()
            ]
        );
    }

    #[test]
    fn recent_source_display_labels_keep_source_kind_visible() {
        assert_eq!(
            recent_source_display_label(&WebAudioSource::local_file("break.wav"), 12),
            "F break.wav"
        );
        assert_eq!(
            recent_source_display_label(
                &WebAudioSource::hosted_url("https://example.com/a.mp3"),
                12
            ),
            "U https://e~"
        );
    }

    #[test]
    fn web_audio_drop_accepts_browser_audio_files_and_known_audio_extensions() {
        assert_eq!(web_audio_drop_error("break.mp3", "audio/mpeg"), None);
        assert_eq!(web_audio_drop_error("909 Kick.WAV", ""), None);
        assert_eq!(
            web_audio_drop_error("cover.png", "image/png"),
            Some("Drop an audio file to load it into AMP808 web.")
        );
    }

    #[test]
    fn web_desktop_layout_leaves_gutters_between_heavy_borders() {
        let body = web_desktop_body_layout(Rect::new(0, 0, 120, 40));
        assert_eq!(body.len(), 2);
        assert_eq!(body[1].x, body[0].x + body[0].width + 1);

        let deck = web_desktop_deck_layout(Rect::new(30, 0, 90, 40));
        assert_eq!(deck.len(), 3);
        assert_eq!(deck[1].y, deck[0].y + deck[0].height + 1);
        assert_eq!(deck[2].y, deck[1].y + deck[1].height + 1);
    }

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
    fn web_compact_layout_leaves_vertical_gutters_between_panels() {
        let deck = web_compact_deck_layout(Rect::new(0, 0, 80, 40));

        assert_eq!(deck.len(), 4);
        assert_eq!(deck[1].y, deck[0].y + deck[0].height + 1);
        assert_eq!(deck[2].y, deck[1].y + deck[1].height + 1);
        assert_eq!(deck[3].y, deck[2].y + deck[2].height + 1);
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

    #[test]
    fn web_panel_fx_signature_only_traces_active_or_error_panels() {
        assert_eq!(web_panel_fx_signature(PanelState::Idle), None);
        assert_eq!(web_panel_fx_signature(PanelState::Armed), None);
        assert_eq!(web_panel_fx_signature(PanelState::Active), Some(1800));
        assert_eq!(web_panel_fx_signature(PanelState::Error), Some(900));
    }

    #[test]
    fn web_panel_border_set_uses_heavy_exabind_style_edges() {
        let set = web_panel_border_set();

        assert_eq!(set.horizontal_top, "━");
        assert_eq!(set.vertical_left, "┃");
        assert_eq!(set.top_left, "▛");
        assert_eq!(set.top_right, "▜");
        assert_eq!(set.bottom_left, "▙");
        assert_eq!(set.bottom_right, "▟");
    }

    #[test]
    fn web_fx_tick_ms_uses_clamped_browser_frame_delta() {
        assert_eq!(web_fx_tick_ms(f64::NAN), 16);
        assert_eq!(web_fx_tick_ms(0.0), 12);
        assert_eq!(web_fx_tick_ms(16.4), 16);
        assert_eq!(web_fx_tick_ms(16.6), 17);
        assert_eq!(web_fx_tick_ms(250.0), 80);
    }

    #[test]
    fn web_header_fx_signature_tracks_motion_worthy_transport_states() {
        assert_eq!(web_header_fx_signature(TransportState::Idle), None);
        assert_eq!(web_header_fx_signature(TransportState::Ready), None);
        assert_eq!(web_header_fx_signature(TransportState::Paused), None);
        assert_eq!(web_header_fx_signature(TransportState::Ended), None);
        assert_eq!(web_header_fx_signature(TransportState::Playing), Some(1400));
        assert_eq!(web_header_fx_signature(TransportState::Error), Some(600));
    }

    #[test]
    fn web_header_identity_fx_signature_tracks_playback_only() {
        assert_eq!(web_header_identity_fx_signature(TransportState::Idle), None);
        assert_eq!(
            web_header_identity_fx_signature(TransportState::Ready),
            None
        );
        assert_eq!(
            web_header_identity_fx_signature(TransportState::Paused),
            None
        );
        assert_eq!(
            web_header_identity_fx_signature(TransportState::Ended),
            None
        );
        assert_eq!(
            web_header_identity_fx_signature(TransportState::Error),
            None
        );
        assert_eq!(
            web_header_identity_fx_signature(TransportState::Playing),
            Some(2200)
        );
    }

    #[test]
    fn web_transition_fx_signature_covers_load_play_and_error_changes() {
        assert_eq!(web_transition_fx_signature(TransportState::Idle), None);
        assert_eq!(
            web_transition_fx_signature(TransportState::Ready),
            Some(360)
        );
        assert_eq!(
            web_transition_fx_signature(TransportState::Playing),
            Some(320)
        );
        assert_eq!(web_transition_fx_signature(TransportState::Paused), None);
        assert_eq!(web_transition_fx_signature(TransportState::Ended), None);
        assert_eq!(
            web_transition_fx_signature(TransportState::Error),
            Some(520)
        );
    }

    #[test]
    fn step_glow_intensity_uses_analyser_energy_without_faking_idle_motion() {
        assert_eq!(step_glow_intensity(TransportState::Idle, 0.9), 0.0);
        assert_eq!(step_glow_intensity(TransportState::Paused, 0.9), 0.0);
        assert_eq!(step_glow_intensity(TransportState::Error, 0.9), 0.0);
        assert_eq!(step_glow_intensity(TransportState::Playing, 0.0), 0.0);
        assert_eq!(step_glow_intensity(TransportState::Playing, 0.07), 0.0);
        assert!(step_glow_intensity(TransportState::Playing, 0.5) > 0.45);
        assert_eq!(step_glow_intensity(TransportState::Playing, 2.0), 1.0);
    }

    #[test]
    fn step_chase_glow_highlights_active_playing_step_without_fake_idle_motion() {
        assert_eq!(
            step_chase_glow_intensity(TransportState::Playing, 0.0, false),
            0.0
        );
        assert!(step_chase_glow_intensity(TransportState::Playing, 0.0, true) >= 0.68);
        assert_eq!(
            step_chase_glow_intensity(TransportState::Paused, 0.9, true),
            0.0
        );
        assert!(
            step_chase_glow_intensity(TransportState::Playing, 0.5, true)
                > step_glow_intensity(TransportState::Playing, 0.5)
        );
    }

    #[test]
    fn web_keyboard_shortcuts_map_to_terminal_actions() {
        assert_eq!(web_action_for_key(" "), Some(WebAction::TogglePlayback));
        assert_eq!(
            web_action_for_key("Spacebar"),
            Some(WebAction::TogglePlayback)
        );
        assert_eq!(web_action_for_key("l"), Some(WebAction::FocusLocalFile));
        assert_eq!(web_action_for_key("L"), Some(WebAction::FocusLocalFile));
        assert_eq!(web_action_for_key("u"), Some(WebAction::FocusHostedUrl));
        assert_eq!(web_action_for_key("v"), Some(WebAction::CycleVisualMode));
        assert_eq!(web_action_for_key("m"), Some(WebAction::ToggleMotion));
        assert_eq!(web_action_for_key("i"), Some(WebAction::FocusAudioControls));
        assert_eq!(web_action_for_key("e"), Some(WebAction::CycleSoundMode));
        assert_eq!(
            web_action_for_key("["),
            Some(WebAction::SelectPreviousAudioControl)
        );
        assert_eq!(
            web_action_for_key("]"),
            Some(WebAction::SelectNextAudioControl)
        );
        assert_eq!(
            web_action_for_key("ArrowUp"),
            Some(WebAction::AdjustSelectedAudioControlUp)
        );
        assert_eq!(
            web_action_for_key("k"),
            Some(WebAction::AdjustSelectedAudioControlUp)
        );
        assert_eq!(
            web_action_for_key("ArrowDown"),
            Some(WebAction::AdjustSelectedAudioControlDown)
        );
        assert_eq!(
            web_action_for_key("j"),
            Some(WebAction::AdjustSelectedAudioControlDown)
        );
        assert_eq!(web_action_for_key("="), Some(WebAction::VolumeUp));
        assert_eq!(web_action_for_key("+"), Some(WebAction::VolumeUp));
        assert_eq!(web_action_for_key("-"), Some(WebAction::VolumeDown));
        assert_eq!(web_action_for_key("ArrowLeft"), Some(WebAction::SeekBack));
        assert_eq!(
            web_action_for_key("ArrowRight"),
            Some(WebAction::SeekForward)
        );
        assert_eq!(web_action_for_key("Escape"), Some(WebAction::ClearFocus));
        assert_eq!(web_action_for_key("x"), None);
    }

    #[test]
    fn web_keyboard_actions_update_terminal_focus() {
        assert_eq!(
            web_focus_after_action(WebFocus::Transport, WebAction::FocusLocalFile),
            WebFocus::LocalFile
        );
        assert_eq!(
            web_focus_after_action(WebFocus::Transport, WebAction::FocusHostedUrl),
            WebFocus::HostedUrl
        );
        assert_eq!(
            web_focus_after_action(WebFocus::HostedUrl, WebAction::CycleVisualMode),
            WebFocus::Analyser
        );
        assert_eq!(
            web_focus_after_action(WebFocus::Transport, WebAction::FocusAudioControls),
            WebFocus::AudioControls
        );
        assert_eq!(
            web_focus_after_action(WebFocus::Transport, WebAction::CycleSoundMode),
            WebFocus::AudioControls
        );
        assert_eq!(
            web_focus_after_action(WebFocus::Transport, WebAction::AdjustSelectedAudioControlUp),
            WebFocus::AudioControls
        );
        assert_eq!(
            web_focus_after_action(WebFocus::LocalFile, WebAction::TogglePlayback),
            WebFocus::Transport
        );
        assert_eq!(
            web_focus_after_action(WebFocus::Analyser, WebAction::ClearFocus),
            WebFocus::Transport
        );
    }

    #[test]
    fn web_visual_mode_cycles_through_bars_wave_retro_logo_and_split() {
        assert_eq!(
            web_visual_mode_after_action(WebVisualMode::Bars, WebAction::CycleVisualMode),
            WebVisualMode::Wave
        );
        assert_eq!(
            web_visual_mode_after_action(WebVisualMode::Wave, WebAction::CycleVisualMode),
            WebVisualMode::Retro
        );
        assert_eq!(
            web_visual_mode_after_action(WebVisualMode::Retro, WebAction::CycleVisualMode),
            WebVisualMode::Logo
        );
        assert_eq!(
            web_visual_mode_after_action(WebVisualMode::Logo, WebAction::CycleVisualMode),
            WebVisualMode::Split
        );
        assert_eq!(
            web_visual_mode_after_action(WebVisualMode::Split, WebAction::CycleVisualMode),
            WebVisualMode::Bars
        );
        assert_eq!(
            web_visual_mode_after_action(WebVisualMode::Wave, WebAction::TogglePlayback),
            WebVisualMode::Wave
        );
    }

    #[test]
    fn web_audio_controls_default_to_native_tui_volume_and_flat_eq_shape() {
        let controls = WebAudioControls::default();

        assert_eq!(controls.volume_db, 0.0);
        assert_eq!(controls.eq_bands, [0.0; 10]);
        assert_eq!(web_eq_preset_label(&controls), "Flat");
        assert_eq!(controls.selected_control, 0);
        assert_eq!(controls.control_revision, 0);
        assert_eq!(web_volume_to_normalized(0.0), 30.0 / 36.0);
        assert_eq!(web_eq_band_to_normalized(0.0), 0.5);
        assert_eq!(web_audio_control_value(&controls, 0), 30.0 / 36.0);
        assert_eq!(web_audio_control_value(&controls, 1), 0.5);
        assert_eq!(web_audio_gain_from_db(0.0), 1.0);
    }

    #[test]
    fn web_audio_sound_modes_cycle_through_native_eq_presets() {
        let mut controls = WebAudioControls::default();

        web_audio_controls_after_action(&mut controls, WebAction::CycleSoundMode);

        assert_eq!(web_eq_preset_label(&controls), WEB_EQ_PRESETS[1].name);
        assert_eq!(controls.eq_bands, WEB_EQ_PRESETS[1].bands);
        assert_eq!(
            web_audio_control_value(&controls, 1),
            web_eq_band_to_normalized(WEB_EQ_PRESETS[1].bands[0])
        );
    }

    #[test]
    fn web_audio_control_selection_and_adjustment_are_clamped() {
        let mut controls = WebAudioControls::default();

        web_audio_controls_after_action(&mut controls, WebAction::SelectNextAudioControl);
        assert_eq!(controls.selected_control, 1);
        assert_eq!(controls.control_revision, 1);

        web_audio_controls_after_action(&mut controls, WebAction::AdjustSelectedAudioControlUp);
        assert_eq!(controls.eq_bands[0], 1.0);
        assert_eq!(web_eq_preset_label(&controls), "CUSTOM");
        assert_eq!(controls.control_revision, 2);

        controls.eq_bands[0] = 12.0;
        web_audio_controls_after_action(&mut controls, WebAction::AdjustSelectedAudioControlUp);
        assert_eq!(controls.eq_bands[0], 12.0);
        assert_eq!(controls.control_revision, 3);

        controls.selected_control = 0;
        controls.volume_db = 6.0;
        web_audio_controls_after_action(&mut controls, WebAction::AdjustSelectedAudioControlUp);
        assert_eq!(controls.volume_db, 6.0);
        assert_eq!(controls.control_revision, 4);

        web_audio_controls_after_action(&mut controls, WebAction::VolumeDown);
        assert_eq!(controls.volume_db, 5.0);
        assert_eq!(controls.control_revision, 5);
    }

    #[test]
    fn web_audio_selected_control_readout_names_volume_eq_and_sound_mode() {
        let mut controls = WebAudioControls::default();

        assert_eq!(
            web_audio_selected_control_readout(&controls),
            "AC / MASTER VOL / +0 dB / Flat"
        );

        controls.selected_control = 1;
        controls.eq_bands[0] = 4.0;
        controls.preset_index = Some(1);
        assert_eq!(
            web_audio_selected_control_readout(&controls),
            "BD / 70Hz / +4 dB / Rock"
        );

        controls.selected_control = 11;
        assert_eq!(
            web_audio_selected_control_readout(&controls),
            "OH / SOUND MODE / Rock"
        );
    }

    #[test]
    fn web_audio_control_hit_test_returns_selected_knob_index() {
        let zones = vec![
            WebAudioControlHitZone {
                index: 0,
                area: Rect::new(4, 2, 8, 5),
            },
            WebAudioControlHitZone {
                index: 1,
                area: Rect::new(14, 2, 8, 5),
            },
        ];

        assert_eq!(web_audio_control_at_cell(&zones, 4, 2), Some(0));
        assert_eq!(web_audio_control_at_cell(&zones, 21, 6), Some(1));
        assert_eq!(web_audio_control_at_cell(&zones, 12, 4), None);
    }

    #[test]
    fn web_audio_control_hit_test_uses_exclusive_bottom_and_right_edges() {
        let zones = vec![WebAudioControlHitZone {
            index: 3,
            area: Rect::new(10, 5, 6, 4),
        }];

        assert_eq!(web_audio_control_at_cell(&zones, 15, 8), Some(3));
        assert_eq!(web_audio_control_at_cell(&zones, 16, 8), None);
        assert_eq!(web_audio_control_at_cell(&zones, 15, 9), None);
    }

    #[test]
    fn web_audio_control_hit_zones_ignore_empty_or_hidden_cells() {
        let cells = vec![
            Rect::new(0, 0, 8, 5),
            Rect::new(8, 0, 0, 5),
            Rect::new(16, 0, 8, 0),
            Rect::new(24, 0, 8, 5),
        ];

        assert_eq!(
            web_audio_control_hit_zones(&cells, 3),
            vec![WebAudioControlHitZone {
                index: 0,
                area: cells[0],
            }]
        );
    }

    #[test]
    fn web_pointer_cell_from_canvas_offset_maps_browser_pixels_to_terminal_cells() {
        let terminal = Rect::new(2, 3, 160, 80);

        assert_eq!(
            web_pointer_cell_from_canvas_offset(400.0, 200.0, 800.0, 400.0, terminal),
            Some((82, 43))
        );
        assert_eq!(
            web_pointer_cell_from_canvas_offset(799.0, 399.0, 800.0, 400.0, terminal),
            Some((161, 82))
        );
        assert_eq!(
            web_pointer_cell_from_canvas_offset(800.0, 399.0, 800.0, 400.0, terminal),
            None
        );
        assert_eq!(
            web_pointer_cell_from_canvas_offset(0.0, -1.0, 800.0, 400.0, terminal),
            None
        );
    }

    #[test]
    fn web_audio_control_fx_signature_only_runs_after_user_interaction() {
        assert_eq!(web_audio_control_fx_signature(0), None);
        assert_eq!(web_audio_control_fx_signature(1), Some(420));
    }

    #[test]
    fn web_visual_mode_labels_match_808_command_surface() {
        assert_eq!(web_visual_mode_label(WebVisualMode::Bars), "BARS");
        assert_eq!(web_visual_mode_label(WebVisualMode::Wave), "WAVE");
        assert_eq!(web_visual_mode_label(WebVisualMode::Retro), "RETRO");
        assert_eq!(web_visual_mode_label(WebVisualMode::Logo), "LOGO");
        assert_eq!(web_visual_mode_label(WebVisualMode::Split), "SPLIT");
    }

    #[test]
    fn web_app_defaults_to_existing_split_visualizer_mode() {
        let state = WebAppState::default();

        assert_eq!(state.visual_mode, WebVisualMode::Split);
    }

    #[test]
    fn persisted_settings_capture_restorable_browser_preferences_only() {
        let mut state = WebAppState {
            visual_mode: WebVisualMode::Logo,
            motion_enabled: false,
            ..WebAppState::default()
        };
        state.audio_controls.volume_db = -8.0;
        state.audio_controls.eq_bands = WEB_EQ_PRESETS[2].bands;
        state.audio_controls.preset_index = Some(2);
        state.audio_controls.selected_control = 4;
        state.recent_sources = vec![
            WebAudioSource::local_file("private-break.wav"),
            WebAudioSource::hosted_url("https://example.com/a.mp3"),
            WebAudioSource::hosted_url("https://cdn.example.com/b.ogg"),
        ];

        let settings = web_persisted_settings_from_state(&state);

        assert_eq!(settings.visual_mode, "logo");
        assert!(!settings.motion_enabled);
        assert_eq!(settings.volume_db, -8.0);
        assert_eq!(settings.eq_bands, WEB_EQ_PRESETS[2].bands);
        assert_eq!(settings.preset_index, Some(2));
        assert_eq!(
            settings.recent_hosted_urls,
            vec![
                "https://example.com/a.mp3".to_string(),
                "https://cdn.example.com/b.ogg".to_string()
            ]
        );

        let json = web_persisted_settings_to_json(&settings).expect("settings should serialize");
        let restored =
            web_persisted_settings_from_json(&json).expect("settings should deserialize");

        assert_eq!(restored, settings);
    }

    #[test]
    fn restored_settings_apply_clamped_state_without_transport_side_effects() {
        let settings = WebPersistedSettings {
            visual_mode: "retro".to_string(),
            motion_enabled: false,
            volume_db: 99.0,
            eq_bands: [20.0, -20.0, 2.0, 1.0, 0.0, -1.0, -2.0, 3.0, 4.0, 5.0],
            preset_index: Some(999),
            recent_hosted_urls: vec![
                "https://example.com/a.mp3".to_string(),
                "https://example.com/a.mp3".to_string(),
                "".to_string(),
                "https://cdn.example.com/b.ogg".to_string(),
            ],
        };
        let mut state = WebAppState {
            transport: TransportState::Playing,
            source: Some(WebAudioSource::local_file("current.wav")),
            ..WebAppState::default()
        };

        web_restore_persisted_settings(&mut state, settings);

        assert_eq!(state.visual_mode, WebVisualMode::Retro);
        assert!(!state.motion_enabled);
        assert_eq!(state.audio_controls.volume_db, WEB_VOLUME_MAX_DB);
        assert_eq!(state.audio_controls.eq_bands[0], WEB_EQ_MAX_DB);
        assert_eq!(state.audio_controls.eq_bands[1], WEB_EQ_MIN_DB);
        assert_eq!(state.audio_controls.preset_index, None);
        assert_eq!(state.audio_controls.selected_control, 0);
        assert_eq!(state.audio_controls.control_revision, 0);
        assert_eq!(state.transport, TransportState::Playing);
        assert_eq!(
            hosted_recent_urls(&state.recent_sources),
            vec![
                "https://example.com/a.mp3".to_string(),
                "https://cdn.example.com/b.ogg".to_string()
            ]
        );
    }

    #[test]
    fn logo_visualizer_renders_centered_braille_logo_from_audio_bands() {
        let bands = vec![1.0; 24];
        let lines = render_logo_visualizer_lines(&bands, 48, 8, 0, false);

        assert_eq!(lines.len(), 8);
        assert!(lines.iter().all(|line| line.cell_width() == 48));
        assert!(
            lines
                .iter()
                .flat_map(|line| line.segments.iter())
                .any(|segment| segment
                    .text
                    .chars()
                    .any(|ch| ('\u{2801}'..='\u{28ff}').contains(&ch))),
            "logo mode should render a braille TR-808 mark"
        );
    }

    #[test]
    fn logo_visualizer_fills_more_cells_for_louder_bands() {
        let quiet = render_logo_visualizer_lines(&vec![0.0; 24], 48, 8, 0, false);
        let loud = render_logo_visualizer_lines(&vec![1.0; 24], 48, 8, 0, false);

        assert!(
            painted_braille_count(&loud) > painted_braille_count(&quiet),
            "loud logo should resolve more of the TR-808 mark than quiet logo"
        );
    }

    fn painted_braille_count(lines: &[super::WebVisualizerLine]) -> usize {
        lines
            .iter()
            .flat_map(|line| &line.segments)
            .flat_map(|segment| segment.text.chars())
            .filter(|ch| ('\u{2801}'..='\u{28ff}').contains(ch))
            .count()
    }

    #[test]
    fn retro_visualizer_draws_grid_sun_and_audio_wave_layers() {
        let bands = (0..24)
            .map(|index| ((index % 8) as f32 + 1.0) / 8.0)
            .collect::<Vec<_>>();
        let lines = render_retro_visualizer_lines(&bands, 48, 10, 3, true);

        assert_eq!(lines.len(), 10);
        assert!(lines.iter().all(|line| line.cell_width() == 48));

        let kinds = lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .map(|segment| segment.kind)
            .collect::<Vec<_>>();

        assert!(kinds.contains(&super::WebVisualizerSegmentKind::RetroGrid));
        assert!(kinds.contains(&super::WebVisualizerSegmentKind::RetroSun));
        assert!(kinds.contains(&super::WebVisualizerSegmentKind::RetroWave));
    }

    #[test]
    fn web_motion_toggle_is_runtime_state_not_a_build_flag() {
        assert!(WebAppState::default().motion_enabled);
        assert!(!web_motion_enabled_after_action(
            true,
            WebAction::ToggleMotion
        ));
        assert!(web_motion_enabled_after_action(
            false,
            WebAction::ToggleMotion
        ));
        assert!(web_motion_enabled_after_action(
            true,
            WebAction::TogglePlayback
        ));
    }

    #[test]
    fn web_seek_target_clamps_to_known_duration() {
        assert_eq!(web_seek_target_seconds(42.0, Some(120.0), -15.0), 27.0);
        assert_eq!(web_seek_target_seconds(5.0, Some(120.0), -15.0), 0.0);
        assert_eq!(web_seek_target_seconds(118.0, Some(120.0), 15.0), 120.0);
        assert_eq!(web_seek_target_seconds(118.0, None, 15.0), 133.0);
        assert_eq!(web_seek_target_seconds(f64::NAN, Some(120.0), 15.0), 15.0);
    }

    #[test]
    fn analyser_bands_expand_to_fill_wide_scope_width() {
        let bands = analyser_bands_for_scope_width(&[0.0, 0.5, 1.0], 20);

        assert_eq!(bands.len(), 10);
        assert_eq!(bands.first().copied(), Some(0.0));
        assert_eq!(bands.last().copied(), Some(1.0));
        assert!(
            bands.iter().filter(|sample| **sample == 0.5).count() >= 2,
            "middle band should be visibly repeated across a wide scope"
        );
    }

    #[test]
    fn waveform_bytes_are_normalized_and_resampled_for_scope_trace() {
        let samples = waveform_bytes_to_samples(&[0, 128, 255], 5);

        assert_eq!(samples.len(), 5);
        assert_eq!(samples.first().copied(), Some(-1.0));
        assert!(samples.iter().any(|sample| sample.abs() < 0.01));
        assert!(samples.last().copied().unwrap() > 0.98);
    }

    #[test]
    fn playback_progress_fraction_uses_known_duration_only() {
        assert_eq!(playback_progress_fraction(30.0, Some(120.0)), 0.25);
        assert_eq!(playback_progress_fraction(150.0, Some(120.0)), 1.0);
        assert_eq!(playback_progress_fraction(-10.0, Some(120.0)), 0.0);
        assert_eq!(playback_progress_fraction(30.0, None), 0.0);
        assert_eq!(playback_progress_fraction(30.0, Some(0.0)), 0.0);
        assert_eq!(playback_progress_fraction(f64::NAN, Some(120.0)), 0.0);
    }

    #[test]
    fn web_step_chase_uses_locked_bpm_for_sixteenth_note_steps() {
        let mut state = WebAppState {
            transport: TransportState::Playing,
            ..WebAppState::default()
        };
        feed_synthetic_bpm_frames(&mut state.bpm, 180);

        state.current_time = 0.0;
        assert_eq!(web_step_chase_index(&state), Some(0));

        state.current_time = 0.124;
        assert_eq!(web_step_chase_index(&state), Some(0));

        state.current_time = 0.125;
        assert_eq!(web_step_chase_index(&state), Some(1));

        state.current_time = 1.875;
        assert_eq!(web_step_chase_index(&state), Some(15));

        state.current_time = 2.0;
        assert_eq!(web_step_chase_index(&state), Some(0));
    }

    #[test]
    fn web_step_chase_falls_back_to_track_progress_without_bpm() {
        let mut state = WebAppState {
            transport: TransportState::Playing,
            current_time: 30.0,
            duration: Some(120.0),
            ..WebAppState::default()
        };

        assert_eq!(web_step_chase_index(&state), Some(4));

        state.current_time = 119.9;
        assert_eq!(web_step_chase_index(&state), Some(15));
    }

    #[test]
    fn web_step_chase_stays_dark_when_transport_has_no_motion() {
        let mut state = WebAppState {
            transport: TransportState::Paused,
            current_time: 30.0,
            duration: Some(120.0),
            ..WebAppState::default()
        };
        feed_synthetic_bpm_frames(&mut state.bpm, 180);

        assert_eq!(web_step_chase_index(&state), None);
    }

    #[test]
    fn analyser_empty_state_text_tracks_browser_audio_state() {
        let mut state = WebAppState::default();
        assert_eq!(
            analyser_empty_state_text(&state),
            Some("LOAD AUDIO OR CORS URL")
        );

        state.transport = TransportState::Ready;
        assert_eq!(
            analyser_empty_state_text(&state),
            Some("READY - PRESS PLAY")
        );

        state.transport = TransportState::Paused;
        assert_eq!(analyser_empty_state_text(&state), Some("PAUSED"));

        state.transport = TransportState::Error;
        state.error = Some("CORS blocked".to_string());
        assert_eq!(
            analyser_empty_state_text(&state),
            Some("CHECK SOURCE / CORS")
        );

        state.transport = TransportState::Playing;
        state.error = None;
        assert_eq!(analyser_empty_state_text(&state), None);
    }

    #[test]
    fn analyser_empty_state_presentation_names_idle_paused_and_blocked_hardware_states() {
        let mut state = WebAppState::default();
        let idle = analyser_empty_state_presentation(&state).expect("idle presentation");
        assert_eq!(idle.title, "LOAD AUDIO");
        assert_eq!(idle.subtitle, "WEB AUDIO ANALYSER");
        assert_eq!(idle.hint, "LOCAL FILE, DROP, OR CORS URL");

        state.transport = TransportState::Paused;
        let paused = analyser_empty_state_presentation(&state).expect("paused presentation");
        assert_eq!(paused.title, "PAUSED");
        assert_eq!(paused.subtitle, "FROZEN ANALYSER");
        assert_eq!(paused.hint, "PRESS PLAY TO RESUME");

        state.transport = TransportState::Error;
        state.error = Some("CORS blocked".to_string());
        let blocked = analyser_empty_state_presentation(&state).expect("error presentation");
        assert_eq!(blocked.title, "CHECK SOURCE");
        assert_eq!(blocked.subtitle, "CORS OR MEDIA ERROR");
        assert_eq!(blocked.hint, "NO FAKE ANALYSER MOTION");
    }

    #[test]
    fn browser_media_error_messages_keep_hosted_and_local_failures_clear() {
        let hosted = WebAudioSource::hosted_url("https://example.com/audio.mp3");
        assert_eq!(
            browser_media_error_message(Some(&hosted), Some(2)),
            "Hosted audio network load failed. Check the URL and server availability."
        );
        assert_eq!(
            browser_media_error_message(Some(&hosted), Some(4)),
            "Hosted audio must be a browser-supported media file and allow CORS for AMP808 web playback."
        );

        let local = WebAudioSource::local_file("bad.flac");
        assert_eq!(
            browser_media_error_message(Some(&local), Some(4)),
            "This local audio codec or container is not supported by the browser."
        );
        assert_eq!(
            browser_media_error_message(None, None),
            "Browser could not load this audio file."
        );
    }

    #[test]
    fn instrument_control_specs_match_808_web_strip() {
        let specs = instrument_control_specs();

        assert_eq!(specs.len(), 12);
        assert_eq!(specs[0].short_label, "AC");
        assert_eq!(specs[0].parameter_label, "LEVEL");
        assert_eq!(specs[1].short_label, "BD");
        assert_eq!(specs[1].instrument_label, "BASS");
        assert_eq!(specs[2].instrument_label, "SNARE");
        assert_eq!(specs[9].short_label, "CB");
        assert_eq!(specs[9].parameter_label, "TUNE");
        assert_eq!(specs[10].short_label, "CY");
        assert_eq!(specs[10].parameter_label, "DECAY");
        assert_eq!(specs[11].short_label, "OH");
        assert_eq!(specs[11].parameter_label, "DECAY");

        let families = specs
            .iter()
            .map(|spec| spec.family)
            .collect::<Vec<ClassicPadFamily>>();
        assert_eq!(
            families,
            vec![
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Ivory,
            ]
        );
    }

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

    #[test]
    fn instrument_label_cap_lines_match_hardware_channel_rows() {
        let specs = &instrument_control_specs()[9..=10];
        let lines = instrument_label_cap_lines(specs, 8);

        assert_eq!(lines.lines.len(), 2);
        assert_eq!(lines.lines[0].spans.len(), 2);
        assert_eq!(lines.lines[1].spans.len(), 2);
        assert_eq!(instrument_label_cap_text(&specs[0]), "CB TUNE");
        assert_eq!(instrument_label_cap_text(&specs[1]), "CY DECAY");
    }

    #[test]
    fn instrument_channel_full_height_documents_fallback_threshold() {
        assert_eq!(INSTRUMENT_CHANNEL_FULL_HEIGHT, 7);
    }
}
