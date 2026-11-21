use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use gpui_kit::{Bounds, Pixels, Window};

use crate::analysis::{AnalysisAvailability, AnalysisSnapshot};

#[cfg(target_os = "macos")]
mod macos;

const SHADER_TIME_WRAP_SECONDS: f32 = 4_096.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CosmicStatus {
    Initializing,
    Running,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CosmicLifecycle {
    status: CosmicStatus,
    rendered_frames: u64,
}

impl CosmicLifecycle {
    pub(crate) fn record_presented_frame(&mut self) {
        if matches!(self.status, CosmicStatus::Failed(_)) {
            return;
        }
        self.status = CosmicStatus::Running;
        self.rendered_frames = self.rendered_frames.saturating_add(1);
    }

    pub(crate) fn record_failure(&mut self, message: impl Into<String>) {
        self.status = CosmicStatus::Failed(message.into());
        self.rendered_frames = 0;
    }

    pub(crate) const fn status(&self) -> &CosmicStatus {
        &self.status
    }

    pub(crate) const fn uses_opaque_fallback(&self) -> bool {
        matches!(self.status, CosmicStatus::Failed(_))
    }

    pub(crate) const fn rendered_frames(&self) -> u64 {
        self.rendered_frames
    }
}

impl Default for CosmicLifecycle {
    fn default() -> Self {
        Self {
            status: CosmicStatus::Initializing,
            rendered_frames: 0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct CosmicError {
    message: String,
}

impl CosmicError {
    fn backend(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CosmicError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cosmic background failed: {}", self.message)
    }
}

impl std::error::Error for CosmicError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CosmicMotion {
    Standard,
    Reduced,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CosmicFrame {
    time_seconds: f32,
    signal: CosmicSignal,
}

impl CosmicFrame {
    pub(crate) fn new(elapsed: Duration, motion: CosmicMotion, signal: CosmicSignal) -> Self {
        match motion {
            CosmicMotion::Standard => Self {
                time_seconds: elapsed.as_secs_f32() % SHADER_TIME_WRAP_SECONDS,
                signal,
            },
            CosmicMotion::Reduced => Self::default(),
        }
    }

    pub(crate) const fn time_seconds(self) -> f32 {
        self.time_seconds
    }

    pub(crate) const fn signal(self) -> CosmicSignal {
        self.signal
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CosmicSignal {
    bass: f32,
    mid: f32,
    high: f32,
    rms: f32,
    peak: f32,
}

impl CosmicSignal {
    pub(crate) fn from_snapshot(snapshot: Option<&AnalysisSnapshot>) -> Self {
        let Some(snapshot) =
            snapshot.filter(|snapshot| snapshot.availability() == AnalysisAvailability::Available)
        else {
            return Self::default();
        };

        let spectrum = snapshot.spectrum();
        let first_boundary = spectrum.len() / 3;
        let second_boundary = first_boundary.saturating_mul(2);
        Self {
            bass: band_energy(&spectrum[..first_boundary]),
            mid: band_energy(&spectrum[first_boundary..second_boundary]),
            high: band_energy(&spectrum[second_boundary..]),
            rms: normalize_energy(snapshot.rms()),
            peak: normalize_energy(snapshot.peak()),
        }
    }

    pub(crate) const fn bass(self) -> f32 {
        self.bass
    }

    pub(crate) const fn mid(self) -> f32 {
        self.mid
    }

    pub(crate) const fn high(self) -> f32 {
        self.high
    }

    pub(crate) const fn rms(self) -> f32 {
        self.rms
    }

    pub(crate) const fn peak(self) -> f32 {
        self.peak
    }
}

fn band_energy(band: &[f32]) -> f32 {
    if band.is_empty() {
        return 0.0;
    }
    let total = band.iter().copied().map(normalize_energy).sum::<f32>();
    normalize_energy(total / band.len() as f32)
}

fn normalize_energy(energy: f32) -> f32 {
    if energy.is_finite() {
        energy.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[derive(Clone)]
pub(crate) struct CosmicBackgroundHost {
    inner: Rc<RefCell<CosmicBackgroundHostInner>>,
}

struct CosmicBackgroundHostInner {
    started_at: Instant,
    lifecycle: CosmicLifecycle,
    #[cfg(target_os = "macos")]
    view: Option<macos::CosmicMetalView>,
}

impl CosmicBackgroundHost {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(CosmicBackgroundHostInner {
                started_at: Instant::now(),
                lifecycle: CosmicLifecycle::default(),
                #[cfg(target_os = "macos")]
                view: None,
            })),
        }
    }

    pub(crate) fn present(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        motion: CosmicMotion,
        signal: CosmicSignal,
    ) {
        self.inner
            .borrow_mut()
            .present(bounds, window, motion, signal);
    }

    pub(crate) fn status(&self) -> CosmicStatus {
        self.inner.borrow().lifecycle.status().clone()
    }

    pub(crate) fn rendered_frames(&self) -> u64 {
        self.inner.borrow().lifecycle.rendered_frames()
    }
}

impl Default for CosmicBackgroundHost {
    fn default() -> Self {
        Self::new()
    }
}

impl CosmicBackgroundHostInner {
    #[cfg(target_os = "macos")]
    fn present(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        motion: CosmicMotion,
        signal: CosmicSignal,
    ) {
        if self.lifecycle.uses_opaque_fallback() {
            return;
        }

        if self.view.is_none() {
            match macos::CosmicMetalView::attach(window) {
                Ok(view) => self.view = Some(view),
                Err(error) => {
                    self.lifecycle.record_failure(error.to_string());
                    return;
                }
            }
        }

        let frame = CosmicFrame::new(self.started_at.elapsed(), motion, signal);
        let view = self
            .view
            .as_mut()
            .expect("cosmic view was initialized above");
        match view.present(bounds, window.scale_factor(), frame) {
            Ok(true) => self.lifecycle.record_presented_frame(),
            Ok(false) => {}
            Err(error) => {
                view.hide();
                self.view = None;
                self.lifecycle.record_failure(error.to_string());
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn present(
        &mut self,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _motion: CosmicMotion,
        _signal: CosmicSignal,
    ) {
        self.lifecycle
            .record_failure("native Metal rendering is available only on macOS");
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;
    use std::time::Duration;

    use crate::analysis::{Analyzer, AnalyzerConfig};

    use super::{CosmicFrame, CosmicLifecycle, CosmicMotion, CosmicSignal, CosmicStatus};

    #[test]
    fn available_analysis_maps_to_finite_normalized_cosmic_energy() {
        let config = AnalyzerConfig::default();
        let samples = (0..config.fft_size())
            .map(|sample| {
                let phase = sample as f32 / config.fft_size() as f32;
                0.45 * (TAU * 4.0 * phase).sin()
                    + 0.3 * (TAU * 32.0 * phase).sin()
                    + 0.2 * (TAU * 180.0 * phase).sin()
            })
            .collect::<Vec<_>>();
        let snapshot = Analyzer::new(config)
            .expect("test analyzer configuration is valid")
            .analyze(&samples, 1)
            .expect("synthetic test signal has a complete FFT window");

        let signal = CosmicSignal::from_snapshot(Some(&snapshot));

        for energy in [
            signal.bass(),
            signal.mid(),
            signal.high(),
            signal.rms(),
            signal.peak(),
        ] {
            assert!(energy.is_finite(), "cosmic energy must be finite: {energy}");
            assert!(
                (0.0..=1.0).contains(&energy),
                "cosmic energy must be normalized: {energy}"
            );
        }
        assert!(
            signal.bass() > 0.0,
            "low-frequency energy should be retained"
        );
        assert!(
            signal.mid() > 0.0,
            "mid-frequency energy should be retained"
        );
        assert!(
            signal.high() > 0.0,
            "high-frequency energy should be retained"
        );
        assert!(signal.rms() > 0.0, "RMS energy should be retained");
        assert!(signal.peak() > 0.0, "peak energy should be retained");
    }

    #[test]
    fn reduced_motion_freezes_time_and_removes_audio_displacement() {
        let signal = CosmicSignal {
            bass: 0.8,
            mid: 0.6,
            high: 0.4,
            rms: 0.5,
            peak: 0.9,
        };

        let frame = CosmicFrame::new(Duration::from_secs_f32(12.5), CosmicMotion::Reduced, signal);

        assert_eq!(frame.time_seconds(), 0.0);
        assert_eq!(frame.signal(), CosmicSignal::default());
    }

    #[test]
    fn renderer_failure_is_terminal_and_requires_the_opaque_fallback() {
        let mut lifecycle = CosmicLifecycle::default();

        lifecycle.record_failure("shader compilation failed");
        lifecycle.record_presented_frame();

        assert_eq!(
            lifecycle.status(),
            &CosmicStatus::Failed("shader compilation failed".into())
        );
        assert!(lifecycle.uses_opaque_fallback());
        assert_eq!(lifecycle.rendered_frames(), 0);
    }
}
