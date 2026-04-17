use std::f64::consts::PI;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

const NUM_BANDS: usize = 10;
const FFT_SIZE: usize = 2048;
const BAND_FILL_WIDTH: usize = 4;
const BAND_GAP_WIDTH: usize = 1;
const SOLID_BAR_WIDTH: usize = NUM_BANDS * BAND_FILL_WIDTH + (NUM_BANDS - 1) * BAND_GAP_WIDTH;
const LOGO_GLYPH_W: usize = 5;
const LOGO_GLYPH_H: usize = 7;
const LOGO_GLYPH_GAP: usize = 2;
const LOGO_GLYPH_COUNT: usize = 6;
const LOGO_TOTAL_W: usize =
    LOGO_GLYPH_COUNT * LOGO_GLYPH_W + (LOGO_GLYPH_COUNT - 1) * LOGO_GLYPH_GAP;
const LOGO_BAND_MAP: [usize; LOGO_GLYPH_COUNT] = [0, 2, 4, 5, 7, 9];

/// Thin glyph for horizontal bars so adjacent rows keep a visible gap.
const HBAR_GLYPH: &str = "▀";
/// Block levels for vertical bar rendering (empty through full).
const VBAR_BLOCKS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Frequency edges for 10 spectrum bands (Hz).
const BAND_EDGES: [f64; 11] = [
    20.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0, 12800.0, 16000.0, 20000.0,
];

/// 5x7 pixel glyphs for `TR-808`.
/// Each row uses 5 bits, with bit 4 as the leftmost pixel.
const LOGO_GLYPHS: [[u8; LOGO_GLYPH_H]; LOGO_GLYPH_COUNT] = [
    [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04], // T
    [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11], // R
    [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00], // -
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E], // 8
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E], // 0
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E], // 8
];

/// Visualizer mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisMode {
    Bars,
    BarsGap,
    VBars,
    Bricks,
    Retro,
    Logo,
    ScotlandFlag,
    Scope,
}

/// Trigger edge mode for oscilloscope alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeTriggerEdge {
    Rising,
    Falling,
}

/// FFT spectrum analyzer for the TUI.
pub struct Visualizer {
    prev: [f64; NUM_BANDS],
    sr: f64,
    buf: Vec<Complex<f64>>,
    pub mode: VisMode,
    frame: u64,
    scope_trigger_enabled: bool,
    scope_trigger_edge: ScopeTriggerEdge,
    scope_trigger_level: f64,
    scope_trigger_debounce: usize,
    planner: FftPlanner<f64>,
}

impl Visualizer {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            prev: [0.0; NUM_BANDS],
            sr: sample_rate,
            buf: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            mode: VisMode::Bars,
            frame: 0,
            scope_trigger_enabled: true,
            scope_trigger_edge: ScopeTriggerEdge::Rising,
            scope_trigger_level: 0.0,
            scope_trigger_debounce: 2,
            planner: FftPlanner::new(),
        }
    }

    pub fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            VisMode::Bars => VisMode::BarsGap,
            VisMode::BarsGap => VisMode::VBars,
            VisMode::VBars => VisMode::Bricks,
            VisMode::Bricks => VisMode::Retro,
            VisMode::Retro => VisMode::Logo,
            VisMode::Logo => VisMode::ScotlandFlag,
            VisMode::ScotlandFlag => VisMode::Scope,
            VisMode::Scope => VisMode::Bars,
        };
    }

    pub fn cycle_music_app_mode(&mut self) {
        self.mode = match self.mode {
            VisMode::Bars => VisMode::BarsGap,
            VisMode::BarsGap => VisMode::VBars,
            VisMode::VBars => VisMode::Bricks,
            VisMode::Bricks => VisMode::Logo,
            VisMode::Logo => VisMode::ScotlandFlag,
            VisMode::ScotlandFlag | VisMode::Retro | VisMode::Scope => VisMode::Bars,
        };
    }

    /// Run FFT on raw audio samples and return 10 normalized band levels (0-1).
    pub fn analyze(&mut self, samples: &[f64]) -> [f64; NUM_BANDS] {
        let mut bands = [0.0f64; NUM_BANDS];

        if samples.is_empty() {
            // Decay previous values when no audio
            for (band, prev) in bands.iter_mut().zip(self.prev.iter_mut()) {
                *band = *prev * 0.8;
                *prev = *band;
            }
            return bands;
        }

        // Zero-fill and copy into buffer with Hann window
        for i in 0..FFT_SIZE {
            let sample = if i < samples.len() { samples[i] } else { 0.0 };
            let w = 0.5 * (1.0 - (2.0 * PI * i as f64 / (FFT_SIZE - 1) as f64).cos());
            self.buf[i] = Complex::new(sample * w, 0.0);
        }

        // Compute FFT
        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        fft.process(&mut self.buf);

        let bin_hz = self.sr / FFT_SIZE as f64;
        let half_len = FFT_SIZE / 2;

        // Sum magnitudes per frequency band
        #[allow(
            clippy::needless_range_loop,
            reason = "Band-edge lookups and output writes are clearest when indexed together."
        )]
        for b in 0..NUM_BANDS {
            let lo_idx = (BAND_EDGES[b] / bin_hz) as usize;
            let hi_idx = (BAND_EDGES[b + 1] / bin_hz) as usize;
            let lo_idx = lo_idx.max(1);
            let hi_idx = hi_idx.min(half_len - 1);

            let mut sum = 0.0;
            let mut count = 0;
            for i in lo_idx..=hi_idx {
                sum += self.buf[i].norm();
                count += 1;
            }
            if count > 0 {
                sum /= count as f64;
            }

            // Convert to dB-like scale and normalize to 0-1
            if sum > 0.0 {
                bands[b] = (20.0 * sum.log10() + 10.0) / 50.0;
            }
            bands[b] = bands[b].clamp(0.0, 1.0);

            // Temporal smoothing: fast attack, slow decay
            if bands[b] > self.prev[b] {
                bands[b] = bands[b] * 0.6 + self.prev[b] * 0.4;
            } else {
                bands[b] = bands[b] * 0.25 + self.prev[b] * 0.75;
            }
            self.prev[b] = bands[b];
        }

        bands
    }

    pub fn synthetic_bands(
        &mut self,
        phase_secs: f64,
        is_playing: bool,
        is_paused: bool,
    ) -> [f64; NUM_BANDS] {
        if !is_playing {
            let mut bands = [0.0; NUM_BANDS];
            for (band, prev) in bands.iter_mut().zip(self.prev.iter_mut()) {
                *band = *prev * 0.72;
                *prev = *band;
            }
            return bands;
        }

        if is_paused && self.prev.iter().any(|&band| band > 0.0) {
            return self.prev;
        }

        let mut bands = [0.0; NUM_BANDS];
        for (idx, band) in bands.iter_mut().enumerate() {
            let idxf = idx as f64;
            let primary = ((phase_secs * 1.15) + idxf * 0.72).sin() * 0.5 + 0.5;
            let secondary = ((phase_secs * 0.63) + idxf * 1.11).cos() * 0.5 + 0.5;
            let accent = ((phase_secs * 1.87) + idxf * 0.41).sin() * 0.5 + 0.5;
            let tilt = 1.0 - (idxf / (NUM_BANDS.saturating_sub(1)) as f64) * 0.22;
            let target = ((0.18 + primary * 0.42 + secondary * 0.18 + accent * 0.12) * tilt)
                .clamp(0.08, 0.95);

            let prev = self.prev[idx];
            *band = if target > prev {
                target * 0.7 + prev * 0.3
            } else {
                target * 0.35 + prev * 0.65
            };
            self.prev[idx] = *band;
        }

        bands
    }

    /// Render spectrum bands as styled text lines.
    /// Returns a Vec of (line_string, Vec<(start_col, width, row_bottom)>) for styling.
    pub fn render(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        match self.mode {
            VisMode::Bars => self.render_vertical_bars_gapped(bands),
            VisMode::BarsGap => self.render_bars_solid(bands),
            VisMode::VBars => self.render_vertical_bars_gapped(bands),
            VisMode::Bricks => self.render_bricks(bands),
            VisMode::Retro | VisMode::Logo | VisMode::ScotlandFlag | VisMode::Scope => {
                self.render_vertical_bars_gapped(&[0.0; NUM_BANDS])
            }
        }
    }

    pub fn render_synthetic(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        match self.mode {
            VisMode::Retro | VisMode::Logo | VisMode::ScotlandFlag | VisMode::Scope => {
                self.render_vertical_bars_gapped(bands)
            }
            _ => self.render(bands),
        }
    }

    /// Render horizontal bars for 808 mode, independent of standard-view mode mappings.
    pub fn render_808_horizontal(
        &self,
        bands: &[f64; NUM_BANDS],
        solid: bool,
    ) -> Vec<SpectrumLine> {
        if solid {
            self.render_bars_solid(bands)
        } else {
            self.render_bars_gapped(bands)
        }
    }

    /// Render a retro synthwave scene using spectrum bands for the horizon wave.
    pub fn render_retro(
        &mut self,
        bands: &[f64; NUM_BANDS],
        width: usize,
        height: usize,
        animate: bool,
    ) -> Vec<SpectrumLine> {
        if width == 0 || height == 0 {
            return Vec::new();
        }

        let dot_rows = height * 4;
        let dot_cols = width * 2;
        if dot_rows == 0 || dot_cols == 0 {
            return Vec::new();
        }

        let mut horizon_dot = (dot_rows * 2) / 5;
        if horizon_dot < 2 {
            horizon_dot = 2;
        }
        horizon_dot = horizon_dot.min(dot_rows.saturating_sub(1));

        let floor_rows = dot_rows.saturating_sub(horizon_dot);
        let center_x = dot_cols.saturating_sub(1) as f64 / 2.0;
        let mut grid = vec![0u8; dot_rows * dot_cols];

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

        let scroll = (self.frame as f64 * 0.08).fract();
        const NUM_HORIZONTAL_LINES: usize = 10;
        for i in 0..NUM_HORIZONTAL_LINES {
            let mut z = (i as f64 + scroll) / NUM_HORIZONTAL_LINES as f64;
            if z > 1.0 {
                z -= 1.0;
            }
            let dy =
                horizon_dot + 1 + (z * z * floor_rows.saturating_sub(2).max(1) as f64) as usize;
            if dy > horizon_dot && dy < dot_rows {
                let offset = dy * dot_cols;
                grid[offset..offset + dot_cols].fill(1);
            }
        }

        let max_wave = horizon_dot as f64 * 0.85;
        let mut wave_y = vec![horizon_dot.min(dot_rows.saturating_sub(1)); dot_cols];
        for (dx, y_slot) in wave_y.iter_mut().enumerate() {
            let band_f =
                dx as f64 / dot_cols.saturating_sub(1).max(1) as f64 * (NUM_BANDS - 1) as f64;
            let band_idx = band_f.floor() as usize;
            let frac = band_f - band_idx as f64;
            let interp = (1.0 - (frac * PI).cos()) / 2.0;

            let level = if band_idx >= NUM_BANDS - 1 {
                bands[NUM_BANDS - 1]
            } else {
                bands[band_idx] * (1.0 - interp) + bands[band_idx + 1] * interp
            }
            .max(0.03);

            let wave_dot = horizon_dot.saturating_sub((level * max_wave) as usize);
            *y_slot = wave_dot.min(dot_rows.saturating_sub(1));
        }

        for (dx, &y) in wave_y.iter().enumerate() {
            grid[y * dot_cols + dx] = 2;
            if dx > 0 {
                let prev = wave_y[dx - 1];
                let y_min = prev.min(y);
                let y_max = prev.max(y);
                for fy in y_min..=y_max {
                    grid[fy * dot_cols + dx] = 2;
                }
            }
        }

        if animate {
            self.frame = self.frame.wrapping_add(1);
        }

        let mut lines = Vec::with_capacity(height);
        for row in 0..height {
            let row_bottom = (height - 1 - row) as f64 / height as f64;
            let base = row * 4;
            let mut segments = Vec::new();
            let mut current_kind: Option<SpectrumSegmentKind> = None;
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
                    SpectrumSegmentKind::RetroWave
                } else if has_sun {
                    SpectrumSegmentKind::RetroSun
                } else {
                    SpectrumSegmentKind::RetroGrid
                };

                if current_kind != Some(kind) && !run.is_empty() {
                    segments.push(SpectrumSegment {
                        text: std::mem::take(&mut run),
                        row_bottom,
                        kind: current_kind.unwrap_or(SpectrumSegmentKind::RetroGrid),
                    });
                }
                current_kind = Some(kind);

                run.push(char::from_u32(0x2800 + bits as u32).unwrap_or(' '));
            }

            if !run.is_empty() {
                segments.push(SpectrumSegment {
                    text: run,
                    row_bottom,
                    kind: current_kind.unwrap_or(SpectrumSegmentKind::RetroGrid),
                });
            }

            lines.push(SpectrumLine { segments });
        }

        lines
    }

    /// Render a dissolving `TR-808` Braille logo driven by spectrum energy.
    pub fn render_logo(
        &mut self,
        bands: &[f64; NUM_BANDS],
        width: usize,
        height: usize,
        animate: bool,
    ) -> Vec<SpectrumLine> {
        if width == 0 || height == 0 {
            return Vec::new();
        }

        let dot_rows = height * 4;
        let dot_cols = width * 2;
        if dot_rows == 0 || dot_cols == 0 {
            return Vec::new();
        }

        let mut pixels = vec![vec![false; dot_cols]; dot_rows];

        let scale_x = (dot_cols / LOGO_TOTAL_W).max(1);
        let scale_y = ((dot_rows.saturating_mul(4) / 5) / LOGO_GLYPH_H).max(1);
        let rendered_w = LOGO_TOTAL_W * scale_x;
        let rendered_h = LOGO_GLYPH_H * scale_y;
        let offset_x = dot_cols.saturating_sub(rendered_w) / 2;
        let base_offset_y = dot_rows.saturating_sub(rendered_h) / 2;
        let frame = self.frame;

        for (glyph_idx, glyph) in LOGO_GLYPHS.iter().enumerate() {
            let energy = bands[LOGO_BAND_MAP[glyph_idx]].clamp(0.0, 1.0);
            let wave = ((frame as f64) * 0.085 + glyph_idx as f64 * 0.78).sin() * 2.4;
            let bounce = (energy * base_offset_y as f64 * 0.5 + wave).round() as isize;

            let letter_x = offset_x + glyph_idx * (LOGO_GLYPH_W + LOGO_GLYPH_GAP) * scale_x;
            let letter_y = base_offset_y as isize - bounce;

            for (py, row_bits) in glyph.iter().enumerate() {
                for px in 0..LOGO_GLYPH_W {
                    if row_bits & (1 << (LOGO_GLYPH_W - 1 - px)) == 0 {
                        continue;
                    }

                    // Loud passages fill the logo solid; silence dissolves it to sparse dots.
                    let fill = (energy * energy * 0.58 + 0.32).clamp(0.0, 0.9);
                    for sy in 0..scale_y {
                        for sx in 0..scale_x {
                            let dx = letter_x + px * scale_x + sx;
                            let dy = letter_y + (py * scale_y + sy) as isize;
                            if dx >= dot_cols || dy < 0 || dy as usize >= dot_rows {
                                continue;
                            }

                            if scatter_hash(glyph_idx, py * scale_y + sy, px * scale_x + sx, frame)
                                > fill
                            {
                                continue;
                            }

                            pixels[dy as usize][dx] = true;
                        }
                    }
                }
            }
        }

        if animate {
            self.frame = self.frame.wrapping_add(1);
        }

        braille_lines_from_pixels(&pixels, width, height, SpectrumSegmentKind::Gradient)
    }

    /// Render a basic oscilloscope trace from recent time-domain samples.
    pub fn render_scope(&self, samples: &[f64], width: usize, height: usize) -> Vec<SpectrumLine> {
        if width == 0 || height == 0 {
            return Vec::new();
        }

        let view = if self.scope_trigger_enabled {
            let trigger_idx = self.find_trigger_index(samples).or_else(|| {
                let opposite = match self.scope_trigger_edge {
                    ScopeTriggerEdge::Rising => ScopeTriggerEdge::Falling,
                    ScopeTriggerEdge::Falling => ScopeTriggerEdge::Rising,
                };
                self.find_trigger_index_with_edge(samples, opposite)
            });
            if let Some(idx) = trigger_idx {
                &samples[idx..]
            } else {
                samples
            }
        } else {
            samples
        };

        let px_w = width * 2;
        let px_h = height * 4;
        let mut pixels = vec![vec![false; px_w]; px_h];

        if view.is_empty() {
            // Draw a center reference line when there's no signal.
            let mid = px_h / 2;
            for cell in pixels[mid].iter_mut().take(px_w) {
                *cell = true;
            }
        } else {
            let mut prev: Option<(i32, i32)> = None;
            let point_count = px_w.max(2);
            for px in 0..point_count {
                let t = px as f64 / (point_count - 1) as f64;
                let s = sample_linear(view, t).clamp(-1.0, 1.0);
                let norm = (s + 1.0) * 0.5; // [-1,1] -> [0,1]
                let y = ((1.0 - norm) * (px_h.saturating_sub(1)) as f64).round() as i32;
                let cur = (px as i32, y.clamp(0, px_h.saturating_sub(1) as i32));

                if let Some(prev_pt) = prev {
                    plot_line(&mut pixels, prev_pt, cur);
                } else {
                    set_pixel(&mut pixels, cur.0, cur.1);
                }

                prev = Some(cur);
            }
        }

        braille_lines_from_pixels(&pixels, width, height, SpectrumSegmentKind::Gradient)
    }

    /// Find the first trigger crossing index for the current scope trigger settings.
    fn find_trigger_index(&self, samples: &[f64]) -> Option<usize> {
        self.find_trigger_index_with_edge(samples, self.scope_trigger_edge)
    }

    fn find_trigger_index_with_edge(
        &self,
        samples: &[f64],
        edge: ScopeTriggerEdge,
    ) -> Option<usize> {
        if samples.len() < 2 {
            return None;
        }

        let level = self.scope_trigger_level.clamp(-1.0, 1.0);
        let debounce = self.scope_trigger_debounce.max(1);

        for i in 1..samples.len() {
            let prev = samples[i - 1];
            let cur = samples[i];

            let crossed = match edge {
                ScopeTriggerEdge::Rising => prev < level && cur >= level,
                ScopeTriggerEdge::Falling => prev > level && cur <= level,
            };
            if !crossed {
                continue;
            }

            let end = i.saturating_add(debounce);
            if end > samples.len() {
                continue;
            }

            let stable = match edge {
                ScopeTriggerEdge::Rising => samples[i..end].iter().all(|&v| v >= level),
                ScopeTriggerEdge::Falling => samples[i..end].iter().all(|&v| v <= level),
            };
            if stable {
                return Some(i);
            }
        }

        None
    }

    fn render_bars_solid(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        const HEIGHT: usize = 5;

        let mut lines = Vec::with_capacity(HEIGHT);

        for row in 0..HEIGHT {
            let row_bottom = (HEIGHT - 1 - row) as f64 / HEIGHT as f64;
            let row_top = (HEIGHT - row) as f64 / HEIGHT as f64;
            let filled = horizontal_row_fill_width(bands, row_bottom, row_top, SOLID_BAR_WIDTH)
                .min(SOLID_BAR_WIDTH);
            let text = format!(
                "{}{}",
                HBAR_GLYPH.repeat(filled),
                " ".repeat(SOLID_BAR_WIDTH - filled)
            );

            lines.push(SpectrumLine {
                segments: vec![SpectrumSegment {
                    text,
                    row_bottom,
                    kind: SpectrumSegmentKind::Gradient,
                }],
            });
        }

        lines
    }

    fn render_bars_gapped(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        const HEIGHT: usize = 5;

        let mut lines = Vec::with_capacity(HEIGHT);

        for row in 0..HEIGHT {
            let row_bottom = (HEIGHT - 1 - row) as f64 / HEIGHT as f64;

            let mut segments = Vec::with_capacity(NUM_BANDS);
            for (i, &level) in bands.iter().enumerate() {
                let block = if level > row_bottom { HBAR_GLYPH } else { " " };

                let mut text = block.repeat(BAND_FILL_WIDTH);
                if i + 1 < NUM_BANDS {
                    push_band_gap(&mut text);
                }

                segments.push(SpectrumSegment {
                    text,
                    row_bottom,
                    kind: SpectrumSegmentKind::Gradient,
                });
            }

            lines.push(SpectrumLine { segments });
        }

        lines
    }

    fn render_bricks(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        const HEIGHT: usize = 5;

        let mut lines = Vec::with_capacity(HEIGHT);

        for row in 0..HEIGHT {
            let row_threshold = (HEIGHT - 1 - row) as f64 / HEIGHT as f64;

            let mut segments = Vec::with_capacity(NUM_BANDS);
            for (i, &level) in bands.iter().enumerate() {
                let mut text = if level > row_threshold {
                    "▄".repeat(BAND_FILL_WIDTH)
                } else {
                    " ".repeat(BAND_FILL_WIDTH)
                };
                if i + 1 < NUM_BANDS {
                    push_band_gap(&mut text);
                }

                segments.push(SpectrumSegment {
                    text,
                    row_bottom: row_threshold,
                    kind: SpectrumSegmentKind::Gradient,
                });
            }

            lines.push(SpectrumLine { segments });
        }

        lines
    }

    fn render_vertical_bars_gapped(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        const HEIGHT: usize = 5;

        let mut lines = Vec::with_capacity(HEIGHT);

        for row in 0..HEIGHT {
            let row_bottom = (HEIGHT - 1 - row) as f64 / HEIGHT as f64;
            let row_top = (HEIGHT - row) as f64 / HEIGHT as f64;

            let mut segments = Vec::with_capacity(NUM_BANDS);
            for (i, &level) in bands.iter().enumerate() {
                let block = if level >= row_top {
                    "█"
                } else if level > row_bottom {
                    let frac = (level - row_bottom) / (row_top - row_bottom);
                    let idx = (frac * (VBAR_BLOCKS.len() - 1) as f64) as usize;
                    VBAR_BLOCKS[idx.min(VBAR_BLOCKS.len() - 1)]
                } else {
                    " "
                };
                let mut text = block.repeat(BAND_FILL_WIDTH);
                if i + 1 < NUM_BANDS {
                    push_band_gap(&mut text);
                }

                segments.push(SpectrumSegment {
                    text,
                    row_bottom,
                    kind: SpectrumSegmentKind::Gradient,
                });
            }

            lines.push(SpectrumLine { segments });
        }

        lines
    }
}

/// A single row of the spectrum visualization.
pub struct SpectrumLine {
    pub segments: Vec<SpectrumSegment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectrumSegmentKind {
    Gradient,
    RetroGrid,
    RetroSun,
    RetroWave,
}

/// A colored segment within a spectrum line.
pub struct SpectrumSegment {
    pub text: String,
    pub row_bottom: f64,
    pub kind: SpectrumSegmentKind,
}

fn horizontal_row_fill_width(
    bands: &[f64; NUM_BANDS],
    row_bottom: f64,
    row_top: f64,
    total_width: usize,
) -> usize {
    if total_width == 0 {
        return 0;
    }

    let mut filled_units = 0.0;
    for (i, &level) in bands.iter().enumerate() {
        if level >= row_top {
            filled_units = (i + 1) as f64;
        } else if level > row_bottom {
            let frac = (level - row_bottom) / (row_top - row_bottom);
            filled_units = i as f64 + frac.clamp(0.0, 1.0);
        }
    }

    ((filled_units / bands.len() as f64) * total_width as f64).round() as usize
}

fn sample_linear(samples: &[f64], t: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    if samples.len() == 1 {
        return samples[0];
    }
    let pos = t.clamp(0.0, 1.0) * (samples.len() - 1) as f64;
    let idx = pos.floor() as usize;
    let next = (idx + 1).min(samples.len() - 1);
    let frac = pos - idx as f64;
    samples[idx] * (1.0 - frac) + samples[next] * frac
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

fn set_pixel(pixels: &mut [Vec<bool>], x: i32, y: i32) {
    if x < 0 || y < 0 {
        return;
    }
    let x = x as usize;
    let y = y as usize;
    if y < pixels.len() && x < pixels[y].len() {
        pixels[y][x] = true;
    }
}

fn plot_line(pixels: &mut [Vec<bool>], from: (i32, i32), to: (i32, i32)) {
    let (mut x0, mut y0) = from;
    let (x1, y1) = to;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        set_pixel(pixels, x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = err * 2;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn braille_bit(dr: usize, dc: usize) -> u8 {
    match (dr, dc) {
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
    pixels: &[Vec<bool>],
    width: usize,
    height: usize,
    kind: SpectrumSegmentKind,
) -> Vec<SpectrumLine> {
    let mut lines = Vec::with_capacity(height);
    for cell_y in 0..height {
        let mut line = String::with_capacity(width);
        for cell_x in 0..width {
            // Braille dot mapping in a 2x4 subcell block.
            // Left column: dots 1,2,3,7. Right column: dots 4,5,6,8.
            let x = cell_x * 2;
            let y = cell_y * 4;

            let mut bits = 0u8;
            if pixels[y][x] {
                bits |= 0x01;
            }
            if pixels[y + 1][x] {
                bits |= 0x02;
            }
            if pixels[y + 2][x] {
                bits |= 0x04;
            }
            if pixels[y + 3][x] {
                bits |= 0x40;
            }
            if pixels[y][x + 1] {
                bits |= 0x08;
            }
            if pixels[y + 1][x + 1] {
                bits |= 0x10;
            }
            if pixels[y + 2][x + 1] {
                bits |= 0x20;
            }
            if pixels[y + 3][x + 1] {
                bits |= 0x80;
            }

            if bits == 0 {
                line.push(' ');
            } else {
                let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
                line.push(ch);
            }
        }

        let row_bottom = (height - 1 - cell_y) as f64 / height as f64;
        lines.push(SpectrumLine {
            segments: vec![SpectrumSegment {
                text: line,
                row_bottom,
                kind,
            }],
        });
    }
    lines
}

fn push_band_gap(text: &mut String) {
    text.extend(std::iter::repeat_n(' ', BAND_GAP_WIDTH));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_empty() {
        let mut vis = Visualizer::new(44100.0);
        let bands = vis.analyze(&[]);
        assert_eq!(bands.len(), 10);
        assert!(bands.iter().all(|&b| (0.0..=1.0).contains(&b)));
    }

    #[test]
    fn test_analyze_sine_wave() {
        let mut vis = Visualizer::new(44100.0);
        let samples: Vec<f64> = (0..FFT_SIZE)
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / 44100.0).sin())
            .collect();
        let bands = vis.analyze(&samples);
        assert!(bands.iter().all(|&b| (0.0..=1.0).contains(&b)));
    }

    #[test]
    fn test_decay() {
        let mut vis = Visualizer::new(44100.0);
        let samples: Vec<f64> = (0..FFT_SIZE)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / 44100.0).sin())
            .collect();
        let bands1 = vis.analyze(&samples);
        let bands2 = vis.analyze(&[]);
        let sum1: f64 = bands1.iter().sum();
        let sum2: f64 = bands2.iter().sum();
        assert!(sum2 <= sum1, "should decay when silent");
    }

    #[test]
    fn test_render_bars_lines() {
        let vis = Visualizer::new(44100.0);
        let bands = [0.5; NUM_BANDS];
        let lines = vis.render(&bands);
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn test_render_bars_have_inter_band_gaps() {
        let vis = Visualizer::new(44100.0);
        let bands = [1.0; NUM_BANDS];
        let lines = vis.render(&bands);
        let top_row = &lines[0];
        let combined: String = top_row
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        let gap_count = combined.chars().filter(|&c| c == ' ').count();
        assert!(
            gap_count >= NUM_BANDS - 1,
            "expected at least {} inter-band spaces, got {gap_count}",
            NUM_BANDS - 1
        );
    }

    #[test]
    fn test_render_bars_are_stacked_columns() {
        let vis = Visualizer::new(44100.0);
        let bands = [1.0; NUM_BANDS];
        let lines = vis.render(&bands);

        let filled_rows = lines
            .iter()
            .filter(|line| {
                line.segments
                    .iter()
                    .any(|seg| seg.text.chars().any(|c| c != ' '))
            })
            .count();
        assert!(filled_rows >= 3, "expected stacked vertical columns");
    }

    #[test]
    fn test_render_bars_use_4_plus_1_band_ratio() {
        let vis = Visualizer::new(44100.0);
        let bands = [1.0; NUM_BANDS];
        let lines = vis.render(&bands);
        let top_row = &lines[0];

        for (i, seg) in top_row.segments.iter().enumerate() {
            let expected = BAND_FILL_WIDTH + usize::from(i + 1 < NUM_BANDS) * BAND_GAP_WIDTH;
            assert_eq!(
                seg.text.chars().count(),
                expected,
                "unexpected width for segment {i}"
            );
        }
    }

    #[test]
    fn test_cycle_mode() {
        let mut vis = Visualizer::new(44100.0);
        assert_eq!(vis.mode, VisMode::Bars);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::BarsGap);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::VBars);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Bricks);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Retro);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Logo);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::ScotlandFlag);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Scope);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Bars);
    }

    #[test]
    fn test_cycle_music_app_mode_skips_scope() {
        let mut vis = Visualizer::new(44100.0);
        assert_eq!(vis.mode, VisMode::Bars);
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::BarsGap);
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::VBars);
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::Bricks);
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::Logo);
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::ScotlandFlag);
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::Bars);

        vis.mode = VisMode::Retro;
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::Bars);

        vis.mode = VisMode::Logo;
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::ScotlandFlag);

        vis.mode = VisMode::ScotlandFlag;
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::Bars);

        vis.mode = VisMode::Scope;
        vis.cycle_music_app_mode();
        assert_eq!(vis.mode, VisMode::Bars);
    }

    #[test]
    fn test_synthetic_bands_move_when_playing() {
        let mut vis = Visualizer::new(44100.0);
        let first = vis.synthetic_bands(12.0, true, false);
        let second = vis.synthetic_bands(13.0, true, false);

        assert!(first.iter().all(|&band| (0.0..=1.0).contains(&band)));
        assert!(second.iter().all(|&band| (0.0..=1.0).contains(&band)));
        assert!(first.iter().any(|&band| band > 0.1));
        assert_ne!(first, second, "playing synthetic bands should animate");
    }

    #[test]
    fn test_synthetic_bands_freeze_when_paused() {
        let mut vis = Visualizer::new(44100.0);
        let playing = vis.synthetic_bands(12.0, true, false);
        let paused = vis.synthetic_bands(13.0, true, true);
        let paused_later = vis.synthetic_bands(40.0, true, true);

        assert_eq!(paused, paused_later);
        assert_eq!(paused, playing);
    }

    #[test]
    fn test_synthetic_bands_decay_when_stopped() {
        let mut vis = Visualizer::new(44100.0);
        let playing = vis.synthetic_bands(12.0, true, false);
        let stopped = vis.synthetic_bands(13.0, false, false);

        let playing_sum: f64 = playing.iter().sum();
        let stopped_sum: f64 = stopped.iter().sum();
        assert!(
            stopped_sum < playing_sum,
            "stopped synthetic bands should decay"
        );
    }

    #[test]
    fn test_render_vertical_bars_are_stacked_columns_with_inter_column_gaps() {
        let mut vis = Visualizer::new(44100.0);
        vis.mode = VisMode::VBars;
        let bands = [1.0; NUM_BANDS];
        let lines = vis.render(&bands);

        let filled_rows = lines
            .iter()
            .filter(|line| {
                line.segments
                    .iter()
                    .any(|seg| seg.text.chars().any(|c| c != ' '))
            })
            .count();
        assert!(filled_rows >= 3, "expected stacked vertical columns");

        let combined_top: String = lines[0]
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        let gap_count = combined_top.chars().filter(|&c| c == ' ').count();
        assert!(gap_count >= NUM_BANDS - 1);
    }

    #[test]
    fn test_render_solid_bars_are_contiguous_without_internal_gaps() {
        let mut vis = Visualizer::new(44100.0);
        vis.mode = VisMode::BarsGap;
        let bands = [1.0, 0.9, 0.8, 0.7, 0.6, 0.45, 0.3, 0.2, 0.1, 0.0];
        let lines = vis.render(&bands);
        let mid_row = &lines[2].segments[0].text;
        let filled = mid_row.trim_end_matches(' ');
        assert!(!filled.contains(' '), "solid bars should not contain gaps");
    }

    #[test]
    fn test_render_scope_dimensions() {
        let vis = Visualizer::new(44100.0);
        let samples = vec![0.0; 128];
        let lines = vis.render_scope(&samples, 24, 5);
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|line| line.segments.len() == 1));
        assert!(
            lines
                .iter()
                .all(|line| line.segments[0].text.chars().count() == 24)
        );
    }

    #[test]
    fn test_render_scope_draws_trace() {
        let vis = Visualizer::new(44100.0);
        let samples: Vec<f64> = (0..128)
            .map(|i| if i % 2 == 0 { -1.0 } else { 1.0 })
            .collect();
        let lines = vis.render_scope(&samples, 24, 5);
        let painted = lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .flat_map(|seg| seg.text.chars())
            .filter(|&c| c != ' ')
            .count();
        assert!(painted > 0, "scope should paint at least one trace point");
    }

    #[test]
    fn test_render_retro_dimensions() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.5; NUM_BANDS];
        let lines = vis.render_retro(&bands, 24, 5, true);
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.chars().count())
                .sum::<usize>()
                == 24
        }));
    }

    #[test]
    fn test_render_retro_draws_scene() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.9, 0.8, 0.75, 0.65, 0.55, 0.45, 0.35, 0.25, 0.2, 0.15];
        let lines = vis.render_retro(&bands, 24, 5, true);
        let painted = lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .flat_map(|seg| seg.text.chars())
            .filter(|&c| c != ' ')
            .count();
        assert!(painted > 0, "retro should paint at least one scene element");
    }

    #[test]
    fn test_render_retro_uses_braille_and_multiple_layers() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.9, 0.8, 0.75, 0.65, 0.55, 0.45, 0.35, 0.25, 0.2, 0.15];
        let lines = vis.render_retro(&bands, 24, 5, true);
        let chars: Vec<char> = lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .flat_map(|seg| seg.text.chars())
            .collect();
        let kinds: Vec<SpectrumSegmentKind> = lines
            .iter()
            .flat_map(|line| line.segments.iter().map(|seg| seg.kind))
            .collect();

        assert!(!chars.contains(&'●'));
        assert!(!chars.contains(&'│'));
        assert!(
            chars
                .iter()
                .any(|&c| ('\u{2801}'..='\u{28ff}').contains(&c)),
            "retro should render with braille glyphs for finer detail"
        );
        assert!(kinds.contains(&SpectrumSegmentKind::RetroGrid));
        assert!(kinds.contains(&SpectrumSegmentKind::RetroSun));
        assert!(kinds.contains(&SpectrumSegmentKind::RetroWave));
    }

    #[test]
    fn test_render_retro_has_striped_sun_breaks() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.9, 0.8, 0.75, 0.65, 0.55, 0.45, 0.35, 0.25, 0.2, 0.15];
        let lines = vis.render_retro(&bands, 40, 8, true);

        let striped_row_found = lines.iter().any(|line| {
            let has_sun = line
                .segments
                .iter()
                .any(|seg| seg.kind == SpectrumSegmentKind::RetroSun);
            let has_grid_gap = line.segments.iter().any(|seg| {
                seg.kind == SpectrumSegmentKind::RetroGrid
                    && seg.text.chars().any(|ch| ch == '\u{2800}')
            });
            has_sun && has_grid_gap
        });

        assert!(
            striped_row_found,
            "retro sun should include striped gaps like the Go renderer"
        );
    }

    #[test]
    fn test_render_retro_freezes_frame_when_not_animating() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.5; NUM_BANDS];

        assert_eq!(vis.frame, 0);
        let _ = vis.render_retro(&bands, 24, 8, false);
        assert_eq!(vis.frame, 0);

        let _ = vis.render_retro(&bands, 24, 8, true);
        assert_eq!(vis.frame, 1);
    }

    #[test]
    fn test_render_logo_dimensions() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.5; NUM_BANDS];
        let lines = vis.render_logo(&bands, 24, 5, true);
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.chars().count())
                .sum::<usize>()
                == 24
        }));
    }

    #[test]
    fn test_render_logo_uses_braille_and_gradient_segments() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.9, 0.15, 0.75, 0.2, 0.85, 0.65, 0.2, 0.7, 0.1, 0.95];
        let lines = vis.render_logo(&bands, 24, 5, true);
        let chars: Vec<char> = lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .flat_map(|seg| seg.text.chars())
            .collect();
        let kinds: Vec<SpectrumSegmentKind> = lines
            .iter()
            .flat_map(|line| line.segments.iter().map(|seg| seg.kind))
            .collect();

        assert!(
            chars
                .iter()
                .any(|&c| ('\u{2801}'..='\u{28ff}').contains(&c)),
            "logo should render with braille glyphs for finer dot detail"
        );
        assert!(
            kinds
                .iter()
                .all(|kind| *kind == SpectrumSegmentKind::Gradient)
        );
    }

    #[test]
    fn test_render_logo_dissolves_at_low_energy_and_fills_at_high_energy() {
        let mut vis = Visualizer::new(44100.0);
        let quiet = [0.0; NUM_BANDS];
        let loud = [1.0; NUM_BANDS];

        let quiet_lines = vis.render_logo(&quiet, 24, 5, false);
        let loud_lines = vis.render_logo(&loud, 24, 5, false);

        let painted_count = |lines: &[SpectrumLine]| {
            lines
                .iter()
                .flat_map(|line| line.segments.iter())
                .flat_map(|seg| seg.text.chars())
                .filter(|&ch| ch != ' ' && ch != '\u{2800}')
                .count()
        };

        assert!(
            painted_count(&loud_lines) > painted_count(&quiet_lines),
            "high-energy logo should fill in more densely than low-energy logo"
        );
    }

    #[test]
    fn test_render_logo_freezes_when_not_animating() {
        let mut vis = Visualizer::new(44100.0);
        let bands = [0.65; NUM_BANDS];

        let first = vis.render_logo(&bands, 24, 5, false);
        let second = vis.render_logo(&bands, 24, 5, false);
        assert_eq!(vis.frame, 0);
        assert_eq!(
            first[0].segments[0].text, second[0].segments[0].text,
            "logo should stay stable when animation is disabled"
        );

        let _ = vis.render_logo(&bands, 24, 5, true);
        assert_eq!(vis.frame, 1);
    }

    #[test]
    fn test_render_scope_uses_braille_not_chunky_glyphs() {
        let vis = Visualizer::new(44100.0);
        let samples: Vec<f64> = (0..128)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / 44100.0).sin())
            .collect();
        let lines = vis.render_scope(&samples, 24, 5);
        let chars: Vec<char> = lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .flat_map(|seg| seg.text.chars())
            .collect();

        assert!(!chars.contains(&'●'));
        assert!(!chars.contains(&'│'));
        assert!(
            chars
                .iter()
                .any(|&c| ('\u{2801}'..='\u{28ff}').contains(&c)),
            "scope should render with braille glyphs for finer detail"
        );
    }

    #[test]
    fn test_find_trigger_index_rising() {
        let mut vis = Visualizer::new(44100.0);
        vis.scope_trigger_edge = ScopeTriggerEdge::Rising;
        vis.scope_trigger_level = 0.0;
        vis.scope_trigger_debounce = 2;
        let samples = vec![-0.8, -0.3, 0.2, 0.6, 0.9];
        assert_eq!(vis.find_trigger_index(&samples), Some(2));
    }

    #[test]
    fn test_find_trigger_index_falling() {
        let mut vis = Visualizer::new(44100.0);
        vis.scope_trigger_edge = ScopeTriggerEdge::Falling;
        vis.scope_trigger_level = 0.0;
        vis.scope_trigger_debounce = 2;
        let samples = vec![0.9, 0.4, -0.2, -0.5, -0.9];
        assert_eq!(vis.find_trigger_index(&samples), Some(2));
    }

    #[test]
    fn test_find_trigger_index_debounce_filters_chatter() {
        let mut vis = Visualizer::new(44100.0);
        vis.scope_trigger_edge = ScopeTriggerEdge::Rising;
        vis.scope_trigger_level = 0.0;
        vis.scope_trigger_debounce = 2;
        let samples = vec![-0.4, 0.2, -0.1, 0.3, 0.5, 0.7];
        assert_eq!(vis.find_trigger_index(&samples), Some(3));
    }

    #[test]
    fn test_render_scope_trigger_alignment_uses_trigger_start() {
        let mut vis = Visualizer::new(44100.0);
        vis.scope_trigger_enabled = true;
        vis.scope_trigger_edge = ScopeTriggerEdge::Rising;
        vis.scope_trigger_level = 0.0;
        vis.scope_trigger_debounce = 2;
        let samples = vec![-1.0, -0.8, -0.4, 0.2, 0.6, 0.9, 0.8, 0.2, -0.4];
        let lines = vis.render_scope(&samples, 9, 5);
        let first_col: Vec<char> = lines
            .iter()
            .map(|line| line.segments[0].text.chars().next().unwrap_or(' '))
            .collect();
        assert_ne!(first_col[2], ' ');
    }
}
