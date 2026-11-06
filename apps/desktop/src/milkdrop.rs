use std::cell::RefCell;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::rc::Rc;

use gpui_kit::{Bounds, Pixels, Window};
use rtrb::{Consumer, Producer, RingBuffer};

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
mod macos;

#[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
const AMP808_FIRST_LIGHT: &str = include_str!("../assets/presets/amp808-first-light.milk");

pub const MAX_MILKDROP_PRESET_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilkdropPreset {
    name: String,
    source: String,
}

impl MilkdropPreset {
    pub fn read(path: &Path) -> Result<Self, MilkdropPresetError> {
        let is_milk = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("milk"));
        if !is_milk {
            return Err(MilkdropPresetError::WrongExtension);
        }

        let file = File::open(path).map_err(MilkdropPresetError::Io)?;
        let mut bytes = Vec::new();
        file.take((MAX_MILKDROP_PRESET_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(MilkdropPresetError::Io)?;
        if bytes.len() > MAX_MILKDROP_PRESET_BYTES {
            return Err(MilkdropPresetError::TooLarge {
                maximum: MAX_MILKDROP_PRESET_BYTES,
            });
        }
        let source = String::from_utf8(bytes).map_err(|_| MilkdropPresetError::InvalidUtf8)?;
        if source.contains('\0') {
            return Err(MilkdropPresetError::InteriorNul);
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("local preset.milk")
            .to_owned();
        Ok(Self { name, source })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

#[derive(Debug)]
pub enum MilkdropPresetError {
    WrongExtension,
    TooLarge { maximum: usize },
    InvalidUtf8,
    InteriorNul,
    Io(io::Error),
}

impl fmt::Display for MilkdropPresetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongExtension => write!(formatter, "select a file with the .milk extension"),
            Self::TooLarge { maximum } => write!(
                formatter,
                "preset exceeds the {} MiB local file limit",
                maximum / 1024 / 1024
            ),
            Self::InvalidUtf8 => write!(formatter, "preset is not valid UTF-8 text"),
            Self::InteriorNul => write!(formatter, "preset contains an interior nul byte"),
            Self::Io(error) => write!(formatter, "preset could not be read: {error}"),
        }
    }
}

impl std::error::Error for MilkdropPresetError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PresetSwapError {
    Rejected(String),
    RollbackFailed { candidate: String, rollback: String },
}

pub(super) fn install_candidate_with_rollback(
    active: &mut String,
    candidate: &str,
    mut install: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), PresetSwapError> {
    match install(candidate) {
        Ok(()) => {
            *active = candidate.to_owned();
            Ok(())
        }
        Err(candidate_error) => match install(active) {
            Ok(()) => Err(PresetSwapError::Rejected(candidate_error)),
            Err(rollback_error) => Err(PresetSwapError::RollbackFailed {
                candidate: candidate_error,
                rollback: rollback_error,
            }),
        },
    }
}

fn readable_projectm_error(error: &str) -> String {
    match error.trim() {
        "" | "std::exception" => "projectM could not parse the preset".to_owned(),
        error => error.to_owned(),
    }
}

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

#[derive(Debug)]
pub struct MilkdropError {
    message: String,
}

impl MilkdropError {
    fn backend(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MilkdropError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MilkDrop backend failed: {}", self.message)
    }
}

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

    pub fn install_preset(&self, preset: &MilkdropPreset) -> Result<(), MilkdropError> {
        self.inner.borrow_mut().install_preset(preset)
    }
}

impl Default for MilkdropViewHost {
    fn default() -> Self {
        Self::new()
    }
}

impl MilkdropViewHostInner {
    #[cfg(all(target_os = "macos", feature = "milkdrop-projectm"))]
    fn install_preset(&mut self, preset: &MilkdropPreset) -> Result<(), MilkdropError> {
        let result = self
            .view
            .as_mut()
            .ok_or_else(|| {
                MilkdropError::backend(
                    "start local playback and select MilkDrop before loading a preset",
                )
            })?
            .install_preset(preset.source());
        match result {
            Ok(()) => {
                self.status = MilkdropStatus::Running;
                Ok(())
            }
            Err(PresetSwapError::Rejected(error)) => {
                let error = readable_projectm_error(&error);
                Err(MilkdropError::backend(format!(
                    "selected preset was rejected ({error}); restored the previous preset"
                )))
            }
            Err(PresetSwapError::RollbackFailed {
                candidate,
                rollback,
            }) => {
                let candidate = readable_projectm_error(&candidate);
                let rollback = readable_projectm_error(&rollback);
                let error = MilkdropError::backend(format!(
                    "selected preset failed ({candidate}) and the previous preset could not be restored ({rollback})"
                ));
                self.view = None;
                self.status = MilkdropStatus::Failed(error.to_string());
                Err(error)
            }
        }
    }

    #[cfg(not(all(target_os = "macos", feature = "milkdrop-projectm")))]
    fn install_preset(&mut self, _preset: &MilkdropPreset) -> Result<(), MilkdropError> {
        Err(MilkdropError::backend(
            "projectM support is unavailable in this build",
        ))
    }
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
    use std::fs;

    use super::{
        MAX_MILKDROP_PRESET_BYTES, MilkdropMetrics, MilkdropPcm, MilkdropPreset,
        MilkdropPresetError, MilkdropStatus, MilkdropViewHost, PresetSwapError,
        install_candidate_with_rollback, readable_projectm_error,
    };
    use tempfile::tempdir;

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

    #[test]
    fn local_milkdrop_preset_accepts_bounded_utf8_milk_files() {
        let directory = tempdir().expect("temporary preset directory is created");
        let path = directory.path().join("Cosmic Bloom.MILK");
        fs::write(&path, b"[preset00]\nfRating=5.0\n")
            .expect("valid MilkDrop preset fixture is written");

        let preset = MilkdropPreset::read(&path).expect("valid .milk preset loads");

        assert_eq!(preset.name(), "Cosmic Bloom.MILK");
        assert_eq!(preset.source(), "[preset00]\nfRating=5.0\n");
    }

    #[test]
    fn local_milkdrop_preset_rejects_wrong_extension_invalid_text_and_nul() {
        let directory = tempdir().expect("temporary preset directory is created");

        let wrong_extension = directory.path().join("preset.txt");
        fs::write(&wrong_extension, b"[preset00]").expect("fixture is written");
        assert!(matches!(
            MilkdropPreset::read(&wrong_extension),
            Err(MilkdropPresetError::WrongExtension)
        ));

        let invalid_text = directory.path().join("invalid.milk");
        fs::write(&invalid_text, [0xff, 0xfe]).expect("fixture is written");
        assert!(matches!(
            MilkdropPreset::read(&invalid_text),
            Err(MilkdropPresetError::InvalidUtf8)
        ));

        let nul = directory.path().join("nul.milk");
        fs::write(&nul, b"[preset00]\0fRating=5.0").expect("fixture is written");
        assert!(matches!(
            MilkdropPreset::read(&nul),
            Err(MilkdropPresetError::InteriorNul)
        ));
    }

    #[test]
    fn local_milkdrop_preset_enforces_the_read_limit() {
        let directory = tempdir().expect("temporary preset directory is created");
        let path = directory.path().join("too-large.milk");
        fs::write(&path, vec![b'x'; MAX_MILKDROP_PRESET_BYTES + 1])
            .expect("oversized preset fixture is written");

        assert!(matches!(
            MilkdropPreset::read(&path),
            Err(MilkdropPresetError::TooLarge { .. })
        ));
    }

    #[test]
    fn rejected_preset_restores_the_last_known_good_preset() {
        let mut active = "bundled".to_owned();
        let mut attempts = Vec::new();

        let error = install_candidate_with_rollback(&mut active, "broken", |preset| {
            attempts.push(preset.to_owned());
            (preset != "broken")
                .then_some(())
                .ok_or("parse error".to_owned())
        })
        .expect_err("broken candidate is rejected");

        assert_eq!(active, "bundled");
        assert_eq!(attempts, ["broken", "bundled"]);
        assert_eq!(error, PresetSwapError::Rejected("parse error".to_owned()));
    }

    #[test]
    fn accepted_preset_becomes_the_next_rollback_target() {
        let mut active = "bundled".to_owned();

        install_candidate_with_rollback(&mut active, "cosmic", |_| Ok(()))
            .expect("candidate installs");

        assert_eq!(active, "cosmic");
    }

    #[test]
    fn failed_rollback_is_distinguished_from_a_recoverable_rejection() {
        let mut active = "bundled".to_owned();

        let error = install_candidate_with_rollback(&mut active, "broken", |preset| {
            Err(format!("{preset} failed"))
        })
        .expect_err("candidate and rollback both fail");

        assert_eq!(
            error,
            PresetSwapError::RollbackFailed {
                candidate: "broken failed".to_owned(),
                rollback: "bundled failed".to_owned(),
            }
        );
        assert_eq!(active, "bundled");
    }

    #[test]
    fn opaque_projectm_exception_is_presented_as_a_useful_preset_error() {
        assert_eq!(
            readable_projectm_error("std::exception"),
            "projectM could not parse the preset"
        );
        assert_eq!(
            readable_projectm_error("unknown function `warp_99`"),
            "unknown function `warp_99`"
        );
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
