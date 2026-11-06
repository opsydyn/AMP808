use std::cell::RefCell;
use std::rc::Rc;

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
use std::fmt;

use gpui_kit::{Bounds, Pixels, Window};
use rtrb::{Consumer, Producer, RingBuffer};

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
mod macos;

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
const AMP808_FIRST_LIGHT: &str = include_str!("../assets/presets/amp808-first-light.milk");

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MilkdropFailureMode {
    #[cfg(feature = "milkdrop-test-hooks")]
    Initialization,
    Preset,
    #[cfg(feature = "milkdrop-test-hooks")]
    Invalid(String),
}

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
impl MilkdropFailureMode {
    pub(super) fn into_error(self) -> MilkdropError {
        let message = match self {
            #[cfg(feature = "milkdrop-test-hooks")]
            Self::Initialization => "injected projectM initialization failure".to_owned(),
            Self::Preset => "injected projectM preset failure".to_owned(),
            #[cfg(feature = "milkdrop-test-hooks")]
            Self::Invalid(value) => format!(
                "unsupported AMP808_MILKDROP_INJECT_FAILURE value `{value}`; expected `initialization` or `preset`"
            ),
        };
        MilkdropError::backend(message)
    }
}

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
#[derive(Debug, Default)]
struct MilkdropFailureInjection {
    mode: Option<MilkdropFailureMode>,
}

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
impl MilkdropFailureInjection {
    fn from_env() -> Self {
        #[cfg(feature = "milkdrop-test-hooks")]
        {
            let value = std::env::var("AMP808_MILKDROP_INJECT_FAILURE");
            Self::from_value(value.as_deref().ok())
        }
        #[cfg(not(feature = "milkdrop-test-hooks"))]
        Self::default()
    }

    #[cfg(feature = "milkdrop-test-hooks")]
    fn from_value(value: Option<&str>) -> Self {
        let mode = value.map(|value| match value {
            "initialization" => MilkdropFailureMode::Initialization,
            "preset" => MilkdropFailureMode::Preset,
            value => MilkdropFailureMode::Invalid(value.to_owned()),
        });
        Self { mode }
    }

    fn take(&mut self) -> Option<MilkdropFailureMode> {
        self.mode.take()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(all(target_os = "macos", feature = "milkdrop-projectm")),
    allow(
        dead_code,
        reason = "running and failure states are produced only by the optional native backend"
    )
)]
pub enum MilkdropStatus {
    Unavailable,
    Initializing,
    Running,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MilkdropMetrics {
    rendered_frames: u64,
}

impl MilkdropMetrics {
    fn record_rendered_frame(&mut self) {
        self.rendered_frames = self.rendered_frames.saturating_add(1);
    }

    const fn rendered_frames(self) -> u64 {
        self.rendered_frames
    }
}

pub struct MilkdropPcmProducer {
    producer: Producer<f32>,
}

impl MilkdropPcmProducer {
    pub fn push_samples(&mut self, samples: &[f32]) -> usize {
        let mut dropped = 0;
        for sample in samples {
            if self.producer.push(*sample).is_err() {
                dropped += 1;
            }
        }
        dropped
    }
}

pub struct MilkdropPcmConsumer {
    consumer: Consumer<f32>,
}

impl MilkdropPcmConsumer {
    pub fn pop_into(&mut self, output: &mut [f32]) -> usize {
        let mut count = 0;
        for destination in output {
            let Ok(sample) = self.consumer.pop() else {
                break;
            };
            *destination = sample;
            count += 1;
        }
        count
    }
}

pub struct MilkdropPcm;

impl MilkdropPcm {
    pub fn bounded(capacity: usize) -> (MilkdropPcmProducer, MilkdropPcmConsumer) {
        let (producer, consumer) = RingBuffer::new(capacity);
        (
            MilkdropPcmProducer { producer },
            MilkdropPcmConsumer { consumer },
        )
    }
}

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
#[derive(Debug)]
pub struct MilkdropError {
    message: String,
}

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
impl MilkdropError {
    fn backend(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
impl fmt::Display for MilkdropError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MilkDrop backend failed: {}", self.message)
    }
}

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
impl std::error::Error for MilkdropError {}

#[derive(Clone)]
pub struct MilkdropViewHost {
    inner: Rc<RefCell<MilkdropViewHostInner>>,
}

struct MilkdropViewHostInner {
    pcm: Option<MilkdropPcmConsumer>,
    status: MilkdropStatus,
    metrics: MilkdropMetrics,
    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    view: Option<macos::ProjectmView>,
    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    failure_injection: MilkdropFailureInjection,
}

impl MilkdropViewHost {
    pub fn new() -> Self {
        #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
        {
            Self::with_failure_injection(MilkdropFailureInjection::from_env())
        }
        #[cfg(not(all(target_os = "macos", feature = "milkdrop-projectm")))]
        Self {
            inner: Rc::new(RefCell::new(MilkdropViewHostInner {
                pcm: None,
                status: MilkdropStatus::Unavailable,
                metrics: MilkdropMetrics::default(),
            })),
        }
    }

    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    fn with_failure_injection(failure_injection: MilkdropFailureInjection) -> Self {
        Self {
            inner: Rc::new(RefCell::new(MilkdropViewHostInner {
                pcm: None,
                status: MilkdropStatus::Unavailable,
                metrics: MilkdropMetrics::default(),
                view: None,
                failure_injection,
            })),
        }
    }

    pub fn replace_pcm(&self, pcm: Option<MilkdropPcmConsumer>) {
        let mut inner = self.inner.borrow_mut();
        inner.pcm = pcm;
        inner.metrics = MilkdropMetrics::default();
        inner.status = if inner.pcm.is_some()
            && cfg!(all(target_os = "macos", feature = "milkdrop-projectm"))
        {
            MilkdropStatus::Initializing
        } else {
            MilkdropStatus::Unavailable
        };
        if inner.pcm.is_none() {
            inner.hide();
        }
    }

    pub fn status(&self) -> MilkdropStatus {
        self.inner.borrow().status.clone()
    }

    pub fn rendered_frames(&self) -> u64 {
        self.inner.borrow().metrics.rendered_frames()
    }

    pub fn hide(&self) {
        self.inner.borrow_mut().hide();
    }

    pub fn present(&self, bounds: Bounds<Pixels>, window: &mut Window, advance: bool) {
        self.inner.borrow_mut().present(bounds, window, advance);
    }
}

impl Default for MilkdropViewHost {
    fn default() -> Self {
        Self::new()
    }
}

impl MilkdropViewHostInner {
    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    fn take_failure_injection(&mut self) -> Option<MilkdropFailureMode> {
        self.failure_injection.take()
    }

    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    fn present(&mut self, bounds: Bounds<Pixels>, window: &mut Window, advance: bool) {
        if matches!(self.status, MilkdropStatus::Failed(_)) {
            return;
        }
        if self.pcm.is_none() {
            self.status = MilkdropStatus::Unavailable;
            self.hide();
            return;
        }

        if self.view.is_none() {
            let failure_injection = self.take_failure_injection();
            match macos::ProjectmView::attach(window, bounds, AMP808_FIRST_LIGHT, failure_injection)
            {
                Ok(view) => self.view = Some(view),
                Err(error) => {
                    self.status = MilkdropStatus::Failed(error.to_string());
                    return;
                }
            }
        }

        let view = self
            .view
            .as_mut()
            .expect("MilkDrop view was initialized above");
        let result = if advance {
            view.present(
                bounds,
                self.pcm
                    .as_mut()
                    .expect("MilkDrop PCM was checked before view initialization"),
            )
        } else {
            view.align(bounds)
        };
        match result {
            Ok(()) => {
                self.status = MilkdropStatus::Running;
                if advance {
                    self.metrics.record_rendered_frame();
                }
            }
            Err(error) => {
                self.view = None;
                self.status = MilkdropStatus::Failed(error.to_string());
            }
        }
    }

    #[cfg(not(all(target_os = "macos", feature = "milkdrop-projectm")))]
    fn present(&mut self, _bounds: Bounds<Pixels>, _window: &mut Window, _advance: bool) {
        self.status = MilkdropStatus::Unavailable;
    }

    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    fn hide(&mut self) {
        if let Some(view) = self.view.as_mut() {
            view.hide();
        }
    }

    #[cfg(not(all(target_os = "macos", feature = "milkdrop-projectm")))]
    fn hide(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::{MilkdropMetrics, MilkdropPcm, MilkdropStatus, MilkdropViewHost};

    #[cfg(all(target_os = "macos", feature = "milkdrop-test-hooks"))]
    use super::{MilkdropFailureInjection, MilkdropFailureMode};

    #[cfg(all(target_os = "macos", feature = "milkdrop-test-hooks"))]
    #[test]
    fn configured_native_failure_is_strict_and_consumed_once() {
        for (value, expected) in [
            ("initialization", MilkdropFailureMode::Initialization),
            ("preset", MilkdropFailureMode::Preset),
        ] {
            let mut injection = MilkdropFailureInjection::from_value(Some(value));

            assert_eq!(injection.take(), Some(expected));
            assert_eq!(injection.take(), None);
        }

        let mut invalid = MilkdropFailureInjection::from_value(Some("typo"));
        assert_eq!(
            invalid.take(),
            Some(MilkdropFailureMode::Invalid("typo".into()))
        );

        assert_eq!(MilkdropFailureInjection::from_value(None).take(), None);
    }

    #[cfg(all(target_os = "macos", feature = "milkdrop-test-hooks"))]
    #[test]
    fn track_reload_recovers_after_the_one_shot_native_failure() {
        let host = MilkdropViewHost::with_failure_injection(MilkdropFailureInjection::from_value(
            Some("initialization"),
        ));
        let (_first_producer, first_consumer) = MilkdropPcm::bounded(2);
        host.replace_pcm(Some(first_consumer));

        assert_eq!(
            host.inner.borrow_mut().take_failure_injection(),
            Some(MilkdropFailureMode::Initialization)
        );
        host.inner.borrow_mut().status = MilkdropStatus::Failed("injected failure".into());

        let (_second_producer, second_consumer) = MilkdropPcm::bounded(2);
        host.replace_pcm(Some(second_consumer));

        assert_eq!(host.status(), MilkdropStatus::Initializing);
        assert_eq!(host.inner.borrow_mut().take_failure_injection(), None);
    }

    #[cfg(all(target_os = "macos", feature = "milkdrop-test-hooks"))]
    #[test]
    fn injected_failure_messages_name_the_failed_native_stage() {
        assert_eq!(
            MilkdropFailureMode::Initialization.into_error().to_string(),
            "MilkDrop backend failed: injected projectM initialization failure"
        );
        assert_eq!(
            MilkdropFailureMode::Preset.into_error().to_string(),
            "MilkDrop backend failed: injected projectM preset failure"
        );
        assert_eq!(
            MilkdropFailureMode::Invalid("typo".into())
                .into_error()
                .to_string(),
            "MilkDrop backend failed: unsupported AMP808_MILKDROP_INJECT_FAILURE value `typo`; expected `initialization` or `preset`"
        );
    }

    #[test]
    fn milkdrop_metrics_count_only_completed_render_frames() {
        let mut metrics = MilkdropMetrics::default();

        metrics.record_rendered_frame();
        metrics.record_rendered_frame();

        assert_eq!(metrics.rendered_frames(), 2);
    }

    #[test]
    fn milkdrop_pcm_handoff_is_bounded_and_drops_excess_visual_samples() {
        let (mut producer, mut consumer) = MilkdropPcm::bounded(2);

        let dropped = producer.push_samples(&[0.1, 0.2, 0.3]);

        assert_eq!(dropped, 1);
        let mut samples = [0.0; 3];
        assert_eq!(consumer.pop_into(&mut samples), 2);
        assert_eq!(&samples[..2], &[0.1, 0.2]);
    }

    #[test]
    fn milkdrop_view_starts_with_an_honest_unavailable_status() {
        let host = MilkdropViewHost::new();

        assert_eq!(host.status(), MilkdropStatus::Unavailable);
    }

    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    #[test]
    fn installing_pcm_marks_the_native_view_as_initializing() {
        let (_producer, consumer) = MilkdropPcm::bounded(2);
        let host = MilkdropViewHost::new();

        host.replace_pcm(Some(consumer));

        assert_eq!(host.status(), MilkdropStatus::Initializing);
    }
}
