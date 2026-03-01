use std::f64::consts::PI;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

const NUM_BANDS: usize = 10;
const FFT_SIZE: usize = 2048;

/// Unicode block elements for fractional bar height (9 levels including space).
const BAR_BLOCKS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Frequency edges for 10 spectrum bands (Hz).
const BAND_EDGES: [f64; 11] = [
    20.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0, 12800.0, 16000.0, 20000.0,
];

/// Visualizer mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisMode {
    Bars,
    Bricks,
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
            scope_trigger_enabled: true,
            scope_trigger_edge: ScopeTriggerEdge::Rising,
            scope_trigger_level: 0.0,
            scope_trigger_debounce: 2,
            planner: FftPlanner::new(),
        }
    }

    pub fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            VisMode::Bars => VisMode::Bricks,
            VisMode::Bricks => VisMode::Scope,
            VisMode::Scope => VisMode::Bars,
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
        #[allow(clippy::needless_range_loop)]
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

    /// Render spectrum bands as styled text lines.
    /// Returns a Vec of (line_string, Vec<(start_col, width, row_bottom)>) for styling.
    pub fn render(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        match self.mode {
            VisMode::Bars => self.render_bars(bands),
            VisMode::Bricks => self.render_bricks(bands),
            VisMode::Scope => self.render_bars(&[0.0; NUM_BANDS]),
        }
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
            for x in 0..px_w {
                pixels[mid][x] = true;
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
                }],
            });
        }
        lines
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

    fn render_bars(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        const HEIGHT: usize = 5;
        const BW: usize = 6;

        let mut lines = Vec::with_capacity(HEIGHT);

        for row in 0..HEIGHT {
            let row_bottom = (HEIGHT - 1 - row) as f64 / HEIGHT as f64;
            let row_top = (HEIGHT - row) as f64 / HEIGHT as f64;

            let mut segments = Vec::with_capacity(NUM_BANDS);
            for &level in bands.iter() {
                let block = if level >= row_top {
                    "█"
                } else if level > row_bottom {
                    let frac = (level - row_bottom) / (row_top - row_bottom);
                    let idx = (frac * (BAR_BLOCKS.len() - 1) as f64) as usize;
                    BAR_BLOCKS[idx.min(BAR_BLOCKS.len() - 1)]
                } else {
                    " "
                };

                segments.push(SpectrumSegment {
                    text: block.repeat(BW),
                    row_bottom,
                });
            }

            lines.push(SpectrumLine { segments });
        }

        lines
    }

    fn render_bricks(&self, bands: &[f64; NUM_BANDS]) -> Vec<SpectrumLine> {
        const HEIGHT: usize = 5;
        const BW: usize = 6;

        let mut lines = Vec::with_capacity(HEIGHT);

        for row in 0..HEIGHT {
            let row_threshold = (HEIGHT - 1 - row) as f64 / HEIGHT as f64;

            let mut segments = Vec::with_capacity(NUM_BANDS);
            for &level in bands.iter() {
                let text = if level > row_threshold {
                    "▄".repeat(BW)
                } else {
                    " ".repeat(BW)
                };

                segments.push(SpectrumSegment {
                    text,
                    row_bottom: row_threshold,
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

/// A colored segment within a spectrum line.
pub struct SpectrumSegment {
    pub text: String,
    pub row_bottom: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_empty() {
        let mut vis = Visualizer::new(44100.0);
        let bands = vis.analyze(&[]);
        assert_eq!(bands.len(), 10);
        assert!(bands.iter().all(|&b| b >= 0.0 && b <= 1.0));
    }

    #[test]
    fn test_analyze_sine_wave() {
        let mut vis = Visualizer::new(44100.0);
        // Generate 1kHz sine wave
        let samples: Vec<f64> = (0..FFT_SIZE)
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / 44100.0).sin())
            .collect();
        let bands = vis.analyze(&samples);
        assert!(bands.iter().all(|&b| b >= 0.0 && b <= 1.0));
        // The 1kHz band (index ~4-5) should have significant energy
    }

    #[test]
    fn test_decay() {
        let mut vis = Visualizer::new(44100.0);
        // Feed a loud signal
        let samples: Vec<f64> = (0..FFT_SIZE)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / 44100.0).sin())
            .collect();
        let bands1 = vis.analyze(&samples);
        // Now feed silence — should decay
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
        assert_eq!(lines.len(), 5); // HEIGHT = 5
    }

    #[test]
    fn test_cycle_mode() {
        let mut vis = Visualizer::new(44100.0);
        assert_eq!(vis.mode, VisMode::Bars);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Bricks);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Scope);
        vis.cycle_mode();
        assert_eq!(vis.mode, VisMode::Bars);
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
        // Trigger starts at sample=0.2, which maps near row 2 for height=5.
        assert_ne!(first_col[2], ' ');
    }
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
