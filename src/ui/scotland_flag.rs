use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

const FIELD_BLUE: Color = Color::Rgb(0x00, 0x5E, 0xB8);
const FIELD_MIDNIGHT: Color = Color::Rgb(0x05, 0x10, 0x2D);
const FIELD_DEEP_BLUE: Color = Color::Rgb(0x0C, 0x2A, 0x66);
const FIELD_INDIGO: Color = Color::Rgb(0x21, 0x1A, 0x57);
const FIELD_PLUM: Color = Color::Rgb(0x43, 0x27, 0x6B);
const SALTIRE_WHITE: Color = Color::Rgb(0xF8, 0xF8, 0xF8);
const SALTIRE_CYAN: Color = Color::Rgb(0x97, 0xF8, 0xFF);
const SALTIRE_LAVENDER: Color = Color::Rgb(0xD6, 0xB0, 0xFF);
const SALTIRE_PHOSPHOR: Color = Color::Rgb(0xE6, 0xFF, 0xFF);
const FULL_BLOCK: &str = "█";
const MIN_BRAILLE_WIDTH: u16 = 6;
const MIN_BRAILLE_HEIGHT: u16 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScotlandFlagConfig {
    pub thickness: f64,
    pub thickness_audio_weight: f64,
    pub ripple: f64,
    pub ripple_secondary: f64,
    pub ripple_frequency: f64,
    pub gain: f64,
    pub warp: f64,
    pub triangle_size: f64,
    pub triangle_pulse: f64,
    pub static_amount: f64,
    pub scanline_amount: f64,
    pub edge_falloff_amount: f64,
    pub bloom_amount: f64,
}

impl Default for ScotlandFlagConfig {
    fn default() -> Self {
        Self {
            thickness: 0.072,
            thickness_audio_weight: 0.056,
            ripple: 0.032,
            ripple_secondary: 0.013,
            ripple_frequency: 0.58,
            gain: 1.24,
            warp: 0.135,
            triangle_size: 0.4,
            triangle_pulse: 0.19,
            static_amount: 0.26,
            scanline_amount: 0.16,
            edge_falloff_amount: 0.21,
            bloom_amount: 0.24,
        }
    }
}

pub struct ScotlandFlagWidget<'a> {
    bands: &'a [f64],
    phase: f64,
    config: ScotlandFlagConfig,
}

impl<'a> ScotlandFlagWidget<'a> {
    pub fn new(bands: &'a [f64], phase: f64) -> Self {
        Self {
            bands,
            phase,
            config: ScotlandFlagConfig::default(),
        }
    }
}

impl Widget for ScotlandFlagWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if should_use_small_fallback(area.width, area.height) {
            render_small_fallback(area, buf);
            return;
        }

        render_flag_braille(area, buf, self.bands, self.phase, self.config);
    }
}

fn render_flag_braille(
    area: Rect,
    buf: &mut Buffer,
    bands: &[f64],
    phase: f64,
    config: ScotlandFlagConfig,
) {
    let bass = low_band_energy(bands, config.gain);
    for x in 0..area.width {
        for y in 0..area.height {
            paint_braille_cell(
                area,
                buf,
                area.x + x,
                area.y + y,
                bands,
                phase,
                bass,
                config,
            );
        }
    }
}

fn paint_braille_cell(
    area: Rect,
    buf: &mut Buffer,
    x: u16,
    y: u16,
    bands: &[f64],
    phase: f64,
    bass: f64,
    config: ScotlandFlagConfig,
) {
    let mut bits = 0u8;
    let cell_x = x - area.x;
    let cell_y = y - area.y;
    let pixel_width = area.width as usize * 2;
    let pixel_height = area.height as usize * 4;
    let cell_x_norm = normalized_cell_center(cell_x, area.width);
    let cell_y_norm = normalized_cell_center(cell_y, area.height);
    let cell_audio = column_level(bands, cell_x_norm, config.gain);
    let background = field_color(cell_x_norm, cell_y_norm, phase, cell_audio, bass, config);
    let saltire_color = saltire_color(cell_x_norm, cell_y_norm, phase, cell_audio, bass, config);

    for sub_row in 0..4 {
        for sub_col in 0..2 {
            let px = cell_x as usize * 2 + sub_col;
            let py = cell_y as usize * 4 + sub_row;
            let x_norm = normalized_position(px, pixel_width);
            let y_norm = normalized_position(py, pixel_height);
            let audio = column_level(bands, x_norm, config.gain);
            let ripple = ripple_offset(x_norm, phase, audio, bass, config);
            let warp = column_audio_warp(x_norm, phase, audio, config);
            let thickness = saltire_thickness(audio, config);
            let warped_y = y_norm + ripple + warp;

            if is_on_saltire(x_norm, warped_y, thickness) {
                bits |= braille_bit(sub_row, sub_col);
            }
        }
    }

    let cell = &mut buf[(x, y)];
    cell.set_style(Style::default().fg(saltire_color).bg(background));
    if bits == 0 {
        cell.set_symbol(" ");
    } else {
        let glyph = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
        let mut symbol = [0u8; 4];
        cell.set_symbol(glyph.encode_utf8(&mut symbol));
    }
}

fn render_small_fallback(area: Rect, buf: &mut Buffer) {
    let threshold = fallback_threshold(area.width, area.height);
    for x in 0..area.width {
        let x_norm = normalized_cell_center(x, area.width);
        for y in 0..area.height {
            let y_norm = normalized_cell_center(y, area.height);
            let color = if is_on_saltire(x_norm, y_norm, threshold) {
                SALTIRE_WHITE
            } else {
                FIELD_BLUE
            };
            paint_fallback_cell(buf, area.x + x, area.y + y, color);
        }
    }
}

fn paint_fallback_cell(buf: &mut Buffer, x: u16, y: u16, color: Color) {
    let cell = &mut buf[(x, y)];
    cell.set_symbol(FULL_BLOCK);
    cell.set_style(Style::default().fg(color).bg(color));
}

fn should_use_small_fallback(width: u16, height: u16) -> bool {
    width < MIN_BRAILLE_WIDTH || height < MIN_BRAILLE_HEIGHT
}

fn fallback_threshold(width: u16, height: u16) -> f64 {
    let tightest_axis = width.min(height).max(1) as f64;
    (0.7 / tightest_axis).max(0.12)
}

fn normalized_cell_center(index: u16, len: u16) -> f64 {
    if len <= 1 {
        return 0.5;
    }

    (index as f64 + 0.5) / len as f64
}

fn normalized_position(index: usize, len: usize) -> f64 {
    if len <= 1 {
        return 0.5;
    }

    (index as f64 + 0.5) / len as f64
}

fn column_level(bands: &[f64], x_norm: f64, gain: f64) -> f64 {
    if bands.is_empty() {
        return 0.0;
    }
    if bands.len() == 1 {
        return (bands[0] * gain).clamp(0.0, 1.0);
    }

    let pos = x_norm.clamp(0.0, 1.0) * (bands.len() - 1) as f64;
    let idx = pos.floor() as usize;
    let next = (idx + 1).min(bands.len() - 1);
    let frac = pos - idx as f64;
    let interpolated = bands[idx] * (1.0 - frac) + bands[next] * frac;
    (interpolated * gain).clamp(0.0, 1.0)
}

fn low_band_energy(bands: &[f64], gain: f64) -> f64 {
    const LOW_BAND_WEIGHTS: [f64; 4] = [0.42, 0.30, 0.18, 0.10];

    let mut weighted = 0.0;
    let mut total_weight = 0.0;
    for (idx, weight) in LOW_BAND_WEIGHTS.iter().enumerate() {
        if let Some(band) = bands.get(idx) {
            weighted += band * weight;
            total_weight += weight;
        }
    }

    if total_weight == 0.0 {
        return 0.0;
    }

    ((weighted / total_weight) * gain).clamp(0.0, 1.0)
}

fn field_color(
    x_norm: f64,
    y_norm: f64,
    phase: f64,
    audio: f64,
    bass: f64,
    config: ScotlandFlagConfig,
) -> Color {
    let vertical = y_norm.clamp(0.0, 1.0).powf(0.82);
    let horizontal = (x_norm - 0.5).abs();
    let tide = ((phase * 0.24) + x_norm * TAU * 0.72 - y_norm * TAU * 0.16).sin() * 0.5 + 0.5;
    let shimmer = ((phase * 0.11) + x_norm * TAU * 1.4 + y_norm * TAU * 0.33).cos() * 0.5 + 0.5;
    let triangle = quadrant_triangle_pulse(x_norm, y_norm, phase, audio, bass, config);
    let scanline = scanline_darkening(y_norm, phase, config.scanline_amount);
    let vignette = edge_falloff(x_norm, y_norm, phase, config.edge_falloff_amount);

    let mut base = mix_rgb(FIELD_MIDNIGHT, FIELD_DEEP_BLUE, 0.22 + vertical * 0.52);
    base = mix_rgb(base, FIELD_BLUE, 0.03 + audio * 0.16 + bass * 0.11);

    let purple_mix = (0.06 + tide * 0.11 + bass * 0.12 + audio * 0.05).clamp(0.0, 0.28);
    let indigo_mix = (0.08 + shimmer * 0.12 + (1.0 - horizontal) * 0.06).clamp(0.0, 0.26);

    let with_indigo = mix_rgb(base, FIELD_INDIGO, indigo_mix);
    let with_plum = mix_rgb(with_indigo, FIELD_PLUM, purple_mix);
    let pulse_blue = mix_rgb(FIELD_BLUE, FIELD_PLUM, 0.24 + bass * 0.24 + audio * 0.06);
    let pulsed = mix_rgb(with_plum, pulse_blue, triangle);
    let scanlined = mix_rgb(pulsed, FIELD_MIDNIGHT, scanline);
    mix_rgb(scanlined, FIELD_MIDNIGHT, vignette)
}

fn corner_local_coords(x_norm: f64, y_norm: f64) -> (f64, f64) {
    let local_x = if x_norm <= 0.5 {
        x_norm * 2.0
    } else {
        (1.0 - x_norm) * 2.0
    };
    let local_y = if y_norm <= 0.5 {
        y_norm * 2.0
    } else {
        (1.0 - y_norm) * 2.0
    };

    (local_x.clamp(0.0, 1.0), local_y.clamp(0.0, 1.0))
}

fn quadrant_triangle_mask(x_norm: f64, y_norm: f64, size: f64) -> f64 {
    let size = size.clamp(0.01, 0.95);
    let (local_x, local_y) = corner_local_coords(x_norm, y_norm);
    let edge = local_x + local_y;
    if edge >= size {
        return 0.0;
    }

    let depth = 1.0 - edge / size;
    depth * depth
}

fn quadrant_triangle_pulse(
    x_norm: f64,
    y_norm: f64,
    phase: f64,
    audio: f64,
    bass: f64,
    config: ScotlandFlagConfig,
) -> f64 {
    let pulse = ((phase * 0.42) + (x_norm + y_norm) * TAU * 0.25).sin() * 0.5 + 0.5;
    let size = config.triangle_size + config.triangle_pulse * (0.3 + bass * 0.7) * pulse;
    let triangle = quadrant_triangle_mask(x_norm, y_norm, size);
    (triangle * (0.16 + bass * 0.22 + audio * 0.09)).clamp(0.0, 0.34)
}

fn static_noise(x_norm: f64, y_norm: f64, phase: f64) -> f64 {
    let x_bucket = (x_norm.clamp(0.0, 1.0) * 255.0) as u64;
    let y_bucket = (y_norm.clamp(0.0, 1.0) * 255.0) as u64;
    let phase_bucket = (phase * 40.0).round().max(0.0) as u64;

    let mut hash = x_bucket.wrapping_mul(7_919)
        ^ y_bucket.wrapping_mul(104_729)
        ^ phase_bucket.wrapping_mul(3_037);
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x45d9_f3b3_7197_344b);
    hash ^= hash >> 16;
    (hash % 10_000) as f64 / 10_000.0
}

fn scanline_darkening(y_norm: f64, phase: f64, amount: f64) -> f64 {
    let stripe = ((y_norm * TAU * 22.0) + phase * 0.28).sin() * 0.5 + 0.5;
    let fine = ((y_norm * TAU * 44.0) - phase * 0.15).cos() * 0.5 + 0.5;
    (amount * (0.28 + stripe * 0.47 + fine * 0.25)).clamp(0.0, amount)
}

fn edge_falloff(x_norm: f64, y_norm: f64, phase: f64, amount: f64) -> f64 {
    let dx = ((x_norm - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    let dy = ((y_norm - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    let radial = ((dx * dx + dy * dy).sqrt() / 2.0_f64.sqrt()).clamp(0.0, 1.0);
    let soft_edge = radial.powf(1.7);
    let drift = ((phase * 0.18) + x_norm * TAU * 0.22 - y_norm * TAU * 0.18).sin() * 0.5 + 0.5;
    (amount * soft_edge * (0.82 + drift * 0.18)).clamp(0.0, amount)
}

fn phosphor_bloom(
    x_norm: f64,
    y_norm: f64,
    phase: f64,
    audio: f64,
    bass: f64,
    config: ScotlandFlagConfig,
) -> f64 {
    let beat = ((phase * 1.55) + x_norm * TAU * 0.38 - y_norm * TAU * 0.24).sin() * 0.5 + 0.5;
    let sweep = ((phase * 0.82) - x_norm * TAU * 0.17 + y_norm * TAU * 0.29).cos() * 0.5 + 0.5;
    let energy = (bass * 0.8 + audio * 0.2).clamp(0.0, 1.0).powf(1.18);

    (config.bloom_amount * energy * (0.34 + beat * 0.46 + sweep * 0.20))
        .clamp(0.0, config.bloom_amount)
}

fn saltire_color(
    x_norm: f64,
    y_norm: f64,
    phase: f64,
    audio: f64,
    bass: f64,
    config: ScotlandFlagConfig,
) -> Color {
    let scan = ((y_norm * TAU * 14.0) + phase * 1.2).sin() * 0.5 + 0.5;
    let shimmer = ((x_norm * TAU * 9.0) - phase * 0.85).cos() * 0.5 + 0.5;
    let static_grain = static_noise(x_norm, y_norm, phase);
    let scanline = scanline_darkening(y_norm, phase, config.scanline_amount * 0.84);
    let vignette = edge_falloff(x_norm, y_norm, phase, config.edge_falloff_amount * 0.68);
    let bloom = phosphor_bloom(x_norm, y_norm, phase, audio, bass, config);

    let cyan_mix = (0.05 + scan * 0.1 + bass * 0.06 + static_grain * config.static_amount * 0.46)
        .clamp(0.0, 0.26);
    let purple_mix =
        (0.04 + shimmer * 0.09 + audio * 0.05 + static_grain * config.static_amount * 0.58)
            .clamp(0.0, 0.24);

    let with_cyan = mix_rgb(SALTIRE_WHITE, SALTIRE_CYAN, cyan_mix);
    let tinted = mix_rgb(with_cyan, SALTIRE_LAVENDER, purple_mix);
    let scanlined = mix_rgb(tinted, FIELD_DEEP_BLUE, scanline);
    let vignetted = mix_rgb(scanlined, FIELD_DEEP_BLUE, vignette);
    mix_rgb(vignetted, SALTIRE_PHOSPHOR, bloom)
}

fn harmonic_wobble_gain(bass: f64, config: ScotlandFlagConfig) -> f64 {
    config.ripple_secondary * (0.18 + bass * 0.82)
}

fn ripple_offset(
    x_norm: f64,
    phase: f64,
    audio: f64,
    bass: f64,
    config: ScotlandFlagConfig,
) -> f64 {
    let base_angle = phase + x_norm * config.ripple_frequency * TAU;
    let harmonic_angle = phase * 0.53 + x_norm * config.ripple_frequency * TAU * 2.0 + 0.35;
    let wave = base_angle.sin() * config.ripple
        + harmonic_angle.sin() * harmonic_wobble_gain(bass, config);
    wave * (0.62 + audio * 1.08 + bass * 0.12)
}

fn column_audio_warp(x_norm: f64, phase: f64, audio: f64, config: ScotlandFlagConfig) -> f64 {
    let sway = (phase * 0.46 + x_norm * TAU * 0.72).sin() * 0.42;
    let edge_bias = ((x_norm - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    (((x_norm - 0.5) * 2.0) + sway * edge_bias) * audio * config.warp
}

fn saltire_thickness(audio: f64, config: ScotlandFlagConfig) -> f64 {
    config.thickness + audio * config.thickness_audio_weight
}

fn is_on_saltire(x_norm: f64, y_norm: f64, threshold: f64) -> bool {
    diagonal_distance(x_norm, y_norm) <= threshold
}

fn diagonal_distance(x_norm: f64, y_norm: f64) -> f64 {
    let rising = (y_norm - x_norm).abs() * FRAC_1_SQRT_2;
    let falling = (x_norm + y_norm - 1.0).abs() * FRAC_1_SQRT_2;
    rising.min(falling)
}

fn braille_bit(sub_row: usize, sub_col: usize) -> u8 {
    match (sub_row, sub_col) {
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

fn mix_rgb(a: Color, b: Color, ratio_to_b: f64) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let t = ratio_to_b.clamp(0.0, 1.0);
            let lerp = |lhs: u8, rhs: u8| -> u8 {
                ((lhs as f64 * (1.0 - t)) + (rhs as f64 * t)).round() as u8
            };
            Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
        }
        _ => a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    fn rgb(color: Color) -> (u8, u8, u8) {
        match color {
            Color::Rgb(r, g, b) => (r, g, b),
            other => panic!("expected rgb color, got {other:?}"),
        }
    }

    fn brightness(color: Color) -> u16 {
        let (r, g, b) = rgb(color);
        r as u16 + g as u16 + b as u16
    }

    #[test]
    fn default_config_keeps_effects_cranked_up() {
        let config = ScotlandFlagConfig::default();

        assert!(config.ripple >= 0.03);
        assert!(config.ripple_secondary >= 0.01);
        assert!(config.warp >= 0.13);
        assert!(config.triangle_pulse >= 0.18);
        assert!(config.static_amount >= 0.25);
        assert!(config.scanline_amount >= 0.15);
        assert!(config.edge_falloff_amount >= 0.2);
        assert!(config.bloom_amount >= 0.2);
    }

    #[test]
    fn normalized_cell_center_handles_single_cell() {
        assert_eq!(normalized_cell_center(0, 0), 0.5);
        assert_eq!(normalized_cell_center(0, 1), 0.5);
    }

    #[test]
    fn normalized_position_handles_single_pixel() {
        assert_eq!(normalized_position(0, 0), 0.5);
        assert_eq!(normalized_position(0, 1), 0.5);
    }

    #[test]
    fn normalized_cell_center_spreads_across_axis() {
        assert!(normalized_cell_center(0, 4) < 0.2);
        assert!(normalized_cell_center(3, 4) > 0.8);
    }

    #[test]
    fn column_level_interpolates_between_band_values() {
        let bands = [0.0, 1.0, 0.0];
        assert_eq!(column_level(&bands, 0.0, 1.0), 0.0);
        assert_eq!(column_level(&bands, 0.5, 1.0), 1.0);
        assert!((column_level(&bands, 0.25, 1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn low_band_energy_prefers_bass_over_high_treble() {
        let bass_heavy = [1.0, 0.8, 0.5, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let treble_heavy = [0.0, 0.0, 0.0, 0.0, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0];

        assert!(low_band_energy(&bass_heavy, 1.0) > low_band_energy(&treble_heavy, 1.0));
    }

    #[test]
    fn corner_local_coords_mirror_each_quadrant() {
        for (x, y) in [(0.1, 0.2), (0.9, 0.2), (0.1, 0.8), (0.9, 0.8)] {
            let (local_x, local_y) = corner_local_coords(x, y);
            assert!((local_x - 0.2).abs() < 1e-9);
            assert!((local_y - 0.4).abs() < 1e-9);
        }
    }

    #[test]
    fn quadrant_triangle_mask_is_strongest_near_corners() {
        let corner = quadrant_triangle_mask(0.02, 0.03, 0.4);
        let edge = quadrant_triangle_mask(0.18, 0.18, 0.4);
        let center = quadrant_triangle_mask(0.5, 0.5, 0.4);

        assert!(corner > edge);
        assert_eq!(center, 0.0);
    }

    #[test]
    fn quadrant_triangle_pulse_tracks_phase_and_bass() {
        let config = ScotlandFlagConfig::default();
        let quiet = quadrant_triangle_pulse(0.08, 0.1, 0.1, 0.2, 0.0, config);
        let loud = quadrant_triangle_pulse(0.08, 0.1, 0.1, 0.6, 1.0, config);
        let later = quadrant_triangle_pulse(0.08, 0.1, 1.2, 0.6, 1.0, config);

        assert!(loud > quiet);
        assert_ne!(loud, later);
    }

    #[test]
    fn quadrant_triangle_pulse_is_noticeable_when_loud() {
        let config = ScotlandFlagConfig::default();
        let loud = quadrant_triangle_pulse(0.04, 0.05, 0.1, 0.8, 1.0, config);

        assert!(loud > 0.08);
    }

    #[test]
    fn field_color_stays_in_dark_blue_purple_family() {
        let (r, g, b) = rgb(field_color(
            0.4,
            0.6,
            0.3,
            0.5,
            0.7,
            ScotlandFlagConfig::default(),
        ));
        assert!(b > g, "expected blue channel to dominate: ({r}, {g}, {b})");
        assert!(b > r, "expected blue channel to dominate: ({r}, {g}, {b})");
        assert!(r > 0, "expected a little purple warmth in the palette");
    }

    #[test]
    fn field_color_varies_with_phase_and_bass() {
        let config = ScotlandFlagConfig::default();
        let quiet = field_color(0.35, 0.55, 0.1, 0.2, 0.0, config);
        let loud = field_color(0.35, 0.55, 1.2, 0.8, 1.0, config);
        assert_ne!(quiet, loud);
    }

    #[test]
    fn field_color_varies_across_flag_surface() {
        let config = ScotlandFlagConfig::default();
        let top = field_color(0.5, 0.1, 0.2, 0.4, 0.3, config);
        let bottom = field_color(0.5, 0.9, 0.2, 0.4, 0.3, config);
        let edge = field_color(0.05, 0.5, 0.2, 0.4, 0.3, config);
        assert_ne!(top, bottom);
        assert_ne!(bottom, edge);
    }

    #[test]
    fn field_color_shows_corner_triangle_accent() {
        let config = ScotlandFlagConfig::default();
        let corner = field_color(0.04, 0.05, 0.6, 0.5, 1.0, config);
        let center = field_color(0.5, 0.5, 0.6, 0.5, 1.0, config);
        assert_ne!(corner, center);
    }

    #[test]
    fn field_color_has_subtle_edge_vignette() {
        let config = ScotlandFlagConfig::default();
        let center = field_color(0.5, 0.5, 0.6, 0.4, 0.5, config);
        let edge = field_color(0.96, 0.5, 0.6, 0.4, 0.5, config);
        let (center_r, center_g, center_b) = rgb(center);
        let (edge_r, edge_g, edge_b) = rgb(edge);

        assert!(edge_b < center_b || edge_g < center_g || edge_r < center_r);
    }

    #[test]
    fn static_noise_varies_with_position_and_phase() {
        let a = static_noise(0.1, 0.2, 0.3);
        let b = static_noise(0.11, 0.2, 0.3);
        let c = static_noise(0.1, 0.2, 0.9);
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn scanline_darkening_varies_by_row_and_stays_subtle() {
        let top = scanline_darkening(0.1, 0.3, 0.08);
        let mid = scanline_darkening(0.5, 0.3, 0.08);
        let bottom = scanline_darkening(0.9, 0.3, 0.08);
        assert_ne!(top, mid);
        assert_ne!(mid, bottom);
        assert!(top <= 0.08 && mid <= 0.08 && bottom <= 0.08);
    }

    #[test]
    fn edge_falloff_darkens_edges_more_than_center() {
        let center = edge_falloff(0.5, 0.5, 0.4, 0.12);
        let edge = edge_falloff(0.95, 0.5, 0.4, 0.12);
        let corner = edge_falloff(0.95, 0.95, 0.4, 0.12);

        assert!(center < 0.001);
        assert!(edge > center);
        assert!(corner > edge);
        assert!(corner <= 0.12);
    }

    #[test]
    fn phosphor_bloom_tracks_bass_hits() {
        let config = ScotlandFlagConfig::default();
        let quiet = phosphor_bloom(0.45, 0.4, 0.7, 0.2, 0.0, config);
        let loud = phosphor_bloom(0.45, 0.4, 0.7, 0.8, 1.0, config);

        assert!(loud > quiet);
        assert!(loud > 0.1);
    }

    #[test]
    fn saltire_color_stays_bright_but_picks_up_crt_tint() {
        let config = ScotlandFlagConfig::default();
        let color = saltire_color(0.3, 0.4, 0.7, 0.6, 0.8, config);
        let (r, g, b) = rgb(color);
        assert!(r > 140 && g > 140 && b > 180);
        assert_ne!(color, SALTIRE_WHITE);
    }

    #[test]
    fn saltire_color_varies_with_scan_static() {
        let config = ScotlandFlagConfig::default();
        let first = saltire_color(0.2, 0.3, 0.1, 0.4, 0.5, config);
        let second = saltire_color(0.2, 0.3, 1.4, 0.4, 0.5, config);
        assert_ne!(first, second);
    }

    #[test]
    fn saltire_color_varies_with_scanline_row() {
        let config = ScotlandFlagConfig::default();
        let top = saltire_color(0.3, 0.15, 0.7, 0.6, 0.8, config);
        let bottom = saltire_color(0.3, 0.85, 0.7, 0.6, 0.8, config);
        assert_ne!(top, bottom);
    }

    #[test]
    fn saltire_color_is_slightly_dimmer_at_edges() {
        let config = ScotlandFlagConfig::default();
        let center = saltire_color(0.5, 0.5, 0.7, 0.6, 0.8, config);
        let edge = saltire_color(0.96, 0.5, 0.7, 0.6, 0.8, config);
        let (center_r, center_g, center_b) = rgb(center);
        let (edge_r, edge_g, edge_b) = rgb(edge);

        assert!(edge_b < center_b || edge_g < center_g || edge_r < center_r);
    }

    #[test]
    fn saltire_color_blooms_brighter_on_bass_hits() {
        let config = ScotlandFlagConfig::default();
        let quiet = saltire_color(0.45, 0.4, 0.7, 0.2, 0.0, config);
        let loud = saltire_color(0.45, 0.4, 0.7, 0.8, 1.0, config);

        assert!(brightness(loud) > brightness(quiet));
    }

    #[test]
    fn diagonal_distance_is_zero_on_both_saltire_lines() {
        assert_eq!(diagonal_distance(0.25, 0.25), 0.0);
        assert_eq!(diagonal_distance(0.25, 0.75), 0.0);
    }

    #[test]
    fn saltire_membership_uses_threshold_distance() {
        assert!(is_on_saltire(0.1, 0.1, 0.02));
        assert!(is_on_saltire(0.1, 0.9, 0.02));
        assert!(!is_on_saltire(0.5, 0.2, 0.02));
    }

    #[test]
    fn ripple_and_thickness_scale_with_audio() {
        let config = ScotlandFlagConfig::default();
        let quiet_ripple = ripple_offset(0.3, 0.8, 0.0, 0.3, config).abs();
        let loud_ripple = ripple_offset(0.3, 0.8, 1.0, 0.3, config).abs();
        assert!(loud_ripple > quiet_ripple);
        assert!(saltire_thickness(1.0, config) > saltire_thickness(0.0, config));
    }

    #[test]
    fn second_harmonic_wobble_is_subtle_but_present() {
        let config = ScotlandFlagConfig::default();
        let mut without_harmonic = config;
        without_harmonic.ripple_secondary = 0.0;

        let base = ripple_offset(0.31, 0.47, 0.45, 0.6, without_harmonic);
        let wobble = ripple_offset(0.31, 0.47, 0.45, 0.6, config);

        assert_ne!(base, wobble);
        assert!((wobble - base).abs() < 0.01);
    }

    #[test]
    fn second_harmonic_wobble_tracks_bass_energy() {
        let config = ScotlandFlagConfig::default();
        let quiet_bass = ripple_offset(0.31, 0.47, 0.45, 0.0, config);
        let loud_bass = ripple_offset(0.31, 0.47, 0.45, 1.0, config);

        assert_ne!(quiet_bass, loud_bass);
        assert!(harmonic_wobble_gain(1.0, config) > harmonic_wobble_gain(0.0, config));
        assert!((loud_bass - quiet_bass).abs() < 0.01);
    }

    #[test]
    fn audio_warp_is_zero_in_center_and_stronger_toward_edges() {
        let config = ScotlandFlagConfig::default();
        assert!(column_audio_warp(0.5, 0.0, 1.0, config).abs() < 0.04);
        assert!(column_audio_warp(0.0, 0.0, 1.0, config).abs() > 0.0);
        assert!(column_audio_warp(1.0, 0.0, 1.0, config).abs() > 0.0);
    }

    #[test]
    fn tiny_widgets_use_fallback_threshold() {
        assert!(should_use_small_fallback(4, 3));
        assert!(!should_use_small_fallback(12, 5));
        assert!(fallback_threshold(3, 2) > fallback_threshold(10, 6));
    }

    #[test]
    fn braille_bit_matches_unicode_layout() {
        assert_eq!(braille_bit(0, 0), 0x01);
        assert_eq!(braille_bit(3, 0), 0x40);
        assert_eq!(braille_bit(2, 1), 0x20);
    }

    #[test]
    fn braille_widget_renders_blue_background_and_fine_glyphs() {
        let area = Rect::new(0, 0, 10, 4);
        let mut buffer = Buffer::empty(area);
        ScotlandFlagWidget::new(&[0.6; 10], 0.0).render(area, &mut buffer);

        let mut saw_braille = false;
        let mut saw_background_variation = false;
        let mut saw_saltire_tint = false;
        let mut last_bg = None;
        for y in 0..area.height {
            for x in 0..area.width {
                let cell = &buffer[(x, y)];
                let (r, g, b) = rgb(cell.bg);
                assert!(
                    b > g && b > r,
                    "expected blue-family background, got ({r}, {g}, {b})"
                );
                if let Some(prev) = last_bg
                    && prev != cell.bg
                {
                    saw_background_variation = true;
                }
                last_bg = Some(cell.bg);
                if let Some(ch) = cell.symbol().chars().next()
                    && ('\u{2801}'..='\u{28ff}').contains(&ch)
                {
                    saw_braille = true;
                    if cell.fg != SALTIRE_WHITE {
                        saw_saltire_tint = true;
                    }
                }
            }
        }

        assert!(saw_braille, "expected braille glyphs for the saltire");
        assert!(
            saw_saltire_tint,
            "expected cyan/purple CRT tint in the saltire"
        );
        assert!(
            saw_background_variation,
            "expected the field blues to vary across the widget"
        );
    }

    #[test]
    fn small_fallback_keeps_block_rendering() {
        let area = Rect::new(0, 0, 4, 2);
        let mut buffer = Buffer::empty(area);
        ScotlandFlagWidget::new(&[0.6; 10], 0.0).render(area, &mut buffer);

        let mut symbols = Vec::new();
        for y in 0..area.height {
            for x in 0..area.width {
                symbols.push(buffer[(x, y)].symbol().to_string());
            }
        }

        assert!(symbols.iter().all(|symbol| symbol == FULL_BLOCK));
    }
}
