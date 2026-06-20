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
use tachyonfx::{fx, EffectRenderer, Interpolation};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    window, AnalyserNode, AudioContext, Document, Event, EventTarget, HtmlAudioElement,
    HtmlButtonElement, HtmlElement, HtmlInputElement, KeyboardEvent, MediaElementAudioSourceNode,
    Url,
};

const BAND_COUNT: usize = 24;
const WEB_SEEK_STEP_SECONDS: f64 = 15.0;
const WAVEFORM_SAMPLE_COUNT: usize = 96;
const WEB_PANE_GAP: u16 = 1;
const INSTRUMENT_CHANNEL_FULL_HEIGHT: u16 = 7;
const RECENT_SOURCE_LIMIT: usize = 4;
const WEB_BPM_DEFAULT_HOP_SECONDS: f64 = 1.0 / 60.0;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebFocus {
    Transport,
    LocalFile,
    HostedUrl,
    Analyser,
    Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebAction {
    TogglePlayback,
    FocusLocalFile,
    FocusHostedUrl,
    CycleVisualMode,
    ToggleMotion,
    SeekBack,
    SeekForward,
    ClearFocus,
}

fn web_action_for_key(key: &str) -> Option<WebAction> {
    match key {
        " " | "Spacebar" | "Space" => Some(WebAction::TogglePlayback),
        "l" | "L" => Some(WebAction::FocusLocalFile),
        "u" | "U" => Some(WebAction::FocusHostedUrl),
        "v" | "V" => Some(WebAction::CycleVisualMode),
        "m" | "M" => Some(WebAction::ToggleMotion),
        "ArrowLeft" => Some(WebAction::SeekBack),
        "ArrowRight" => Some(WebAction::SeekForward),
        "Escape" => Some(WebAction::ClearFocus),
        _ => None,
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
        ) | (PanelRole::Analyser, WebFocus::Analyser)
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
    last_transition_transport: Option<TransportState>,
    transition_panel: Option<WebPanelFx>,
    header_panel: Option<WebPanelFx>,
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
        tachyonfx::Duration::from_millis(tick_ms)
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

    fn clear_panel(&mut self, role: PanelRole) {
        *self.panel_slot(role) = None;
    }

    fn clear_effects(&mut self) {
        self.last_transition_transport = None;
        self.transition_panel = None;
        self.header_panel = None;
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
            motion_enabled: true,
            recent_sources: Vec::new(),
        }
    }
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
    let state = Rc::new(RefCell::new(WebAppState::default()));
    let fx_runtime = Rc::new(RefCell::new(WebFxRuntime::default()));
    let graph = install_audio_graph(Rc::clone(&state)).map_err(js_to_io_error)?;

    terminal.draw_web(move |frame| {
        sample_analyser(&graph, &state);
        let snapshot = state.borrow().clone();
        let mut fx_runtime = fx_runtime.borrow_mut();
        render_web_808(frame, &snapshot, &mut fx_runtime);
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
    source.connect_with_audio_node(&analyser)?;
    analyser.connect_with_audio_node(&context.destination())?;

    let graph = Rc::new(AudioGraph {
        audio,
        context,
        _source: source,
        analyser,
    });

    wire_controls(&document, Rc::clone(&graph), state)?;
    Ok(graph)
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
    let object_url: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

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

            revoke_object_url(&object_url);
            match Url::create_object_url_with_blob(&file) {
                Ok(url) => {
                    graph.audio.set_cross_origin(None);
                    graph.audio.set_src(&url);
                    graph.audio.load();
                    *object_url.borrow_mut() = Some(url);

                    {
                        let mut state = state.borrow_mut();
                        let source = WebAudioSource::local_file(file.name());
                        state.source = Some(source.clone());
                        remember_recent_source(&mut state.recent_sources, source);
                        state.transport = TransportState::Ready;
                        state.status = "Local audio loaded".to_string();
                        state.error = None;
                        state.current_time = 0.0;
                        state.duration = None;
                        state.bands = vec![0.0; BAND_COUNT];
                        state.waveform = vec![0.0; WAVEFORM_SAMPLE_COUNT];
                        state.bpm = WebBpmState::estimating();
                        state.last_bpm_sample_ms = None;
                    }
                    sync_controls(&toggle_button, &control_status, &state.borrow());
                }
                Err(error) => set_error(
                    &state,
                    &toggle_button,
                    &control_status,
                    format!("Could not create browser object URL: {error:?}"),
                ),
            }
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
            }
            let state_ref = state.borrow();
            sync_controls(&toggle_button, &control_status, &state_ref);
        })?;
    }

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
            WebAction::CycleVisualMode => {
                {
                    let mut state = state.borrow_mut();
                    state.status = "Visualizer focus selected".to_string();
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

fn render_web_808(frame: &mut Frame<'_>, state: &WebAppState, fx: &mut WebFxRuntime) {
    let tick = fx.next_tick(js_sys::Date::now());
    if !state.motion_enabled {
        fx.clear_effects();
    }
    let area = frame.area();
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
            Constraint::Length(3),
            Constraint::Min(14),
            Constraint::Length(2),
        ])
        .split(inner);

    render_machine_header(frame, rows[0], state);
    if state.motion_enabled {
        if let Some(effect) = fx.header_effect(state.transport) {
            frame.render_effect(effect, rows[0], tick);
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
        Span::styled(" FILE  ", classic_small_label_style()),
        Span::styled("U", command_key_style(state.focus == WebFocus::HostedUrl)),
        Span::styled(" URL  ", classic_small_label_style()),
        Span::styled("V", command_key_style(state.focus == WebFocus::Analyser)),
        Span::styled(" VIS  ", classic_small_label_style()),
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
        ("Roland ", true),
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

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(classic_hardware_body_style())
            .alignment(Alignment::Center),
        area,
    );
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
    state: &WebAppState,
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

    if inner.height < INSTRUMENT_CHANNEL_FULL_HEIGHT {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(inner);
        let knob_cells = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, visible as u32); visible])
            .split(rows[0]);

        for (index, (cell, spec)) in knob_cells.iter().zip(specs.iter()).enumerate() {
            render_canvas_knob(frame, *cell, web_knob_value(index), spec, index == 1);
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

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(2)])
        .split(inner);
    let knob_cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, visible as u32); visible])
        .split(rows[0]);

    for (index, (cell, spec)) in knob_cells.iter().zip(specs.iter()).enumerate() {
        render_canvas_knob(frame, *cell, web_knob_value(index), spec, index == 1);
    }

    let label_lines = instrument_label_cap_lines(specs, 8);
    frame.render_widget(
        Paragraph::new(label_lines).alignment(Alignment::Center),
        rows[1],
    );
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

fn web_knob_value(index: usize) -> f64 {
    const VALUES: [f64; 12] = [
        0.72, 0.62, 0.55, 0.48, 0.58, 0.66, 0.35, 0.44, 0.7, 0.5, 0.77, 0.68,
    ];
    VALUES[index % VALUES.len()]
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

    if let Some(message) = analyser_empty_state_text(state) {
        render_analyser_empty_state(frame, inner, message, state.transport);
        return;
    }

    if inner.height >= 9 {
        let scope_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(inner);
        render_spectrum_bars(frame, scope_rows[0], bands);
        render_waveform_trace(frame, scope_rows[1], &state.waveform);
        render_audio_progress_row(frame, scope_rows[2], state);
    } else if inner.height >= 5 {
        let scope_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(inner);
        render_spectrum_bars(frame, scope_rows[0], bands);
        render_audio_progress_row(frame, scope_rows[1], state);
    } else {
        render_spectrum_bars(frame, inner, bands);
    }
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

fn render_analyser_empty_state(
    frame: &mut Frame<'_>,
    area: Rect,
    message: &'static str,
    transport: TransportState,
) {
    let spacer_count = area.height.saturating_sub(3) / 2;
    let mut lines = Vec::with_capacity(usize::from(spacer_count) + 3);
    for _ in 0..spacer_count {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        message,
        Style::default()
            .fg(transport_color(transport))
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "WEB AUDIO ANALYSER",
        classic_small_label_style(),
    )));
    lines.push(Line::from(Span::styled(
        "REAL BANDS ONLY",
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
    let mut numbers = Vec::with_capacity(step_count);
    let mut pads = Vec::with_capacity(step_count);
    for step in 0..step_count {
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
        analyser_bands_for_scope_width, analyser_empty_state_text, browser_media_error_message,
        classic_body_border_style, classic_hardware_body_style, classic_pad_family,
        classic_panel_inset_style, classic_step_keycap_style, classic_step_keycap_text_color,
        contrast_ratio, hardware_body_text_style, hardware_brand_style, hosted_recent_urls,
        instrument_channel_visible_count, instrument_control_specs, instrument_family_bg,
        instrument_family_fg, instrument_label_cap_lines, instrument_label_cap_text,
        knob_canvas_bounds_808, machine_brand_label, playback_progress_fraction,
        recent_source_display_label, remember_recent_source, step_glow_intensity,
        tempo_dial_geometry_808, tempo_tick_color_808, waveform_bytes_to_samples,
        web_action_for_key, web_compact_deck_layout, web_desktop_body_layout,
        web_desktop_deck_layout, web_focus_after_action, web_fx_tick_ms, web_header_fx_signature,
        web_motion_enabled_after_action, web_panel_border_set, web_panel_fx_signature,
        web_panel_spec, web_seek_target_seconds, web_tempo_display, web_transition_fx_signature,
        Classic808Palette, ClassicColor, ClassicPadFamily, PanelRole, PanelState, TransportState,
        WebAction, WebAppState, WebAudioSource, WebFocus, INSTRUMENT_CHANNEL_FULL_HEIGHT,
    };
    use amp808_core::web_audio::WebBpmState;
    use ratzilla::ratatui::{layout::Rect, style::Color};

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
        assert_eq!(machine_brand_label(), "Roland Rhythm Composer TR-808 WEB");
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
            web_focus_after_action(WebFocus::LocalFile, WebAction::TogglePlayback),
            WebFocus::Transport
        );
        assert_eq!(
            web_focus_after_action(WebFocus::Analyser, WebAction::ClearFocus),
            WebFocus::Transport
        );
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
