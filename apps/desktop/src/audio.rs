use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use rtrb::{Consumer, Producer, RingBuffer};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputSampleFormat {
    F32,
    I16,
    U16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedSampleFormat(cpal::SampleFormat);

impl std::fmt::Display for UnsupportedSampleFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "unsupported audio output sample format: {}",
            self.0
        )
    }
}

impl std::error::Error for UnsupportedSampleFormat {}

impl TryFrom<cpal::SampleFormat> for OutputSampleFormat {
    type Error = UnsupportedSampleFormat;

    fn try_from(value: cpal::SampleFormat) -> Result<Self, Self::Error> {
        match value {
            cpal::SampleFormat::F32 => Ok(Self::F32),
            cpal::SampleFormat::I16 => Ok(Self::I16),
            cpal::SampleFormat::U16 => Ok(Self::U16),
            _ => Err(UnsupportedSampleFormat(value)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TapDiagnostics {
    accepted: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

impl TapDiagnostics {
    pub fn accepted(&self) -> u64 {
        self.accepted.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PushReport {
    accepted: usize,
    dropped: usize,
}

impl PushReport {
    pub const fn accepted(self) -> usize {
        self.accepted
    }

    pub const fn dropped(self) -> usize {
        self.dropped
    }
}

pub struct AnalysisTapProducer {
    producer: Producer<f32>,
    diagnostics: TapDiagnostics,
}

impl AnalysisTapProducer {
    pub fn push_samples(&mut self, samples: &[f32]) -> PushReport {
        let mut accepted = 0;
        let mut dropped = 0;

        for sample in samples {
            if self.producer.push(*sample).is_ok() {
                accepted += 1;
            } else {
                dropped += 1;
            }
        }

        self.diagnostics
            .accepted
            .fetch_add(accepted as u64, Ordering::Relaxed);
        self.diagnostics
            .dropped
            .fetch_add(dropped as u64, Ordering::Relaxed);

        PushReport { accepted, dropped }
    }

    fn try_push_unreported(&mut self, sample: f32) -> bool {
        self.producer.push(sample).is_ok()
    }

    fn record_counts(&self, accepted: usize, dropped: usize) {
        self.diagnostics
            .accepted
            .fetch_add(accepted as u64, Ordering::Relaxed);
        self.diagnostics
            .dropped
            .fetch_add(dropped as u64, Ordering::Relaxed);
    }
}

pub struct AnalysisTapConsumer {
    consumer: Consumer<f32>,
}

impl AnalysisTapConsumer {
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

pub struct AnalysisTap;

impl AnalysisTap {
    pub fn bounded(capacity: usize) -> (AnalysisTapProducer, AnalysisTapConsumer, TapDiagnostics) {
        let (producer, consumer) = RingBuffer::new(capacity);
        let diagnostics = TapDiagnostics {
            accepted: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
        };

        (
            AnalysisTapProducer {
                producer,
                diagnostics: diagnostics.clone(),
            },
            AnalysisTapConsumer { consumer },
            diagnostics,
        )
    }
}

#[derive(Clone, Debug)]
pub struct VolumeControl {
    value: Arc<AtomicU64>,
}

impl VolumeControl {
    pub fn new() -> Self {
        Self {
            value: Arc::new(AtomicU64::new(u64::from(1.0_f32.to_bits()))),
        }
    }

    pub fn set(&self, value: f32) {
        let value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.value
            .store(u64::from(value.to_bits()), Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.value.load(Ordering::Relaxed) as u32)
    }
}

impl Default for VolumeControl {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FrameProducer {
    producer: Producer<[f32; 2]>,
}

impl FrameProducer {
    pub fn push_frames(&mut self, frames: &[[f32; 2]]) -> usize {
        let mut accepted = 0;
        for frame in frames {
            if self.producer.push(*frame).is_err() {
                break;
            }
            accepted += 1;
        }
        accepted
    }
}

#[derive(Clone, Debug)]
pub struct PlaybackDiagnostics {
    underflow_frames: Arc<AtomicU64>,
    analysis: TapDiagnostics,
}

impl PlaybackDiagnostics {
    pub fn underflow_frames(&self) -> u64 {
        self.underflow_frames.load(Ordering::Relaxed)
    }

    pub fn analysis_accepted(&self) -> u64 {
        self.analysis.accepted()
    }

    pub fn analysis_dropped(&self) -> u64 {
        self.analysis.dropped()
    }
}

pub struct AudioCallback {
    consumer: Consumer<[f32; 2]>,
    analysis: AnalysisTapProducer,
    volume: VolumeControl,
    underflow_frames: Arc<AtomicU64>,
}

impl AudioCallback {
    pub fn volume_control(&self) -> VolumeControl {
        self.volume.clone()
    }

    pub fn render_f32(&mut self, output: &mut [f32], channel_count: usize) {
        self.render_output(output, channel_count);
    }

    fn render_output<T>(&mut self, output: &mut [T], channel_count: usize)
    where
        T: Sample + FromSample<f32>,
    {
        if channel_count == 0 {
            output.fill(T::from_sample(0.0));
            return;
        }

        let volume = self.volume.get();
        let mut underflow_frames = 0_u64;
        let mut tap_accepted = 0;
        let mut tap_dropped = 0;

        for output_frame in output.chunks_mut(channel_count) {
            let frame = match self.consumer.pop() {
                Ok(frame) => [frame[0] * volume, frame[1] * volume],
                Err(_) => {
                    underflow_frames += 1;
                    [0.0, 0.0]
                }
            };

            if channel_count == 1 {
                output_frame[0] = T::from_sample((frame[0] + frame[1]) * 0.5);
            } else {
                output_frame[0] = T::from_sample(frame[0]);
                output_frame[1] = T::from_sample(frame[1]);
                output_frame[2..].fill(T::from_sample(0.0));
            }

            let mono = (frame[0] + frame[1]) * 0.5;
            if self.analysis.try_push_unreported(mono) {
                tap_accepted += 1;
            } else {
                tap_dropped += 1;
            }
        }

        if underflow_frames > 0 {
            self.underflow_frames
                .fetch_add(underflow_frames, Ordering::Relaxed);
        }
        self.analysis.record_counts(tap_accepted, tap_dropped);
    }
}

#[derive(Debug)]
pub struct AudioDeviceError(String);

impl std::fmt::Display for AudioDeviceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AudioDeviceError {}

fn select_output_config(
    ranges: impl IntoIterator<Item = cpal::SupportedStreamConfigRange>,
    sample_rate: u32,
) -> Result<cpal::SupportedStreamConfig, AudioDeviceError> {
    ranges
        .into_iter()
        .filter(|range| range.channels() > 0)
        .filter_map(|range| {
            let format_rank = match OutputSampleFormat::try_from(range.sample_format()) {
                Ok(OutputSampleFormat::F32) => 0,
                Ok(OutputSampleFormat::I16) => 1,
                Ok(OutputSampleFormat::U16) => 2,
                Err(_) => return None,
            };
            let channel_rank = usize::from(range.channels() != 2);
            range
                .try_with_sample_rate(sample_rate)
                .map(|config| ((channel_rank, format_rank), config))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, config)| config)
        .ok_or_else(|| {
            AudioDeviceError(format!(
                "default audio output does not support {sample_rate} Hz with a compatible sample format"
            ))
        })
}

pub struct OutputStreamHandle {
    _stream: cpal::Stream,
    volume: VolumeControl,
    latest_error: Arc<Mutex<Option<String>>>,
}

impl OutputStreamHandle {
    pub fn open_default(
        callback: AudioCallback,
        sample_rate: u32,
    ) -> Result<Self, AudioDeviceError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            AudioDeviceError("no default audio output device is available".into())
        })?;
        let supported = select_output_config(
            device
                .supported_output_configs()
                .map_err(|error| AudioDeviceError(error.to_string()))?,
            sample_rate,
        )?;
        let output_format = OutputSampleFormat::try_from(supported.sample_format())
            .map_err(|error| AudioDeviceError(error.to_string()))?;
        let config = supported.config();
        let channel_count = usize::from(config.channels);
        let volume = callback.volume_control();
        let latest_error = Arc::new(Mutex::new(None));
        let stream = match output_format {
            OutputSampleFormat::F32 => build_output_stream::<f32>(
                &device,
                config,
                channel_count,
                callback,
                latest_error.clone(),
            ),
            OutputSampleFormat::I16 => build_output_stream::<i16>(
                &device,
                config,
                channel_count,
                callback,
                latest_error.clone(),
            ),
            OutputSampleFormat::U16 => build_output_stream::<u16>(
                &device,
                config,
                channel_count,
                callback,
                latest_error.clone(),
            ),
        }
        .map_err(|error| AudioDeviceError(error.to_string()))?;
        stream
            .play()
            .map_err(|error| AudioDeviceError(error.to_string()))?;

        Ok(Self {
            _stream: stream,
            volume,
            latest_error,
        })
    }

    pub fn volume_control(&self) -> VolumeControl {
        self.volume.clone()
    }

    pub fn take_error(&self) -> Option<String> {
        self.latest_error
            .lock()
            .expect("audio error state lock poisoned")
            .take()
    }
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channel_count: usize,
    mut callback: AudioCallback,
    latest_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    device.build_output_stream(
        config,
        move |output: &mut [T], _| callback.render_output(output, channel_count),
        move |error| {
            if let Ok(mut slot) = latest_error.try_lock() {
                *slot = Some(error.to_string());
            }
        },
        None,
    )
}

pub struct PlaybackBuffer;

impl PlaybackBuffer {
    pub fn bounded(
        frame_capacity: usize,
        analysis_capacity: usize,
    ) -> (
        FrameProducer,
        AudioCallback,
        AnalysisTapConsumer,
        PlaybackDiagnostics,
    ) {
        let (frame_producer, frame_consumer) = RingBuffer::new(frame_capacity);
        let (analysis_producer, analysis_consumer, analysis_diagnostics) =
            AnalysisTap::bounded(analysis_capacity);
        let volume = VolumeControl::new();
        let underflow_frames = Arc::new(AtomicU64::new(0));
        let diagnostics = PlaybackDiagnostics {
            underflow_frames: underflow_frames.clone(),
            analysis: analysis_diagnostics,
        };

        (
            FrameProducer {
                producer: frame_producer,
            },
            AudioCallback {
                consumer: frame_consumer,
                analysis: analysis_producer,
                volume,
                underflow_frames,
            },
            analysis_consumer,
            diagnostics,
        )
    }
}

#[cfg(test)]
mod tests {
    use cpal::{SampleFormat, SupportedBufferSize, SupportedStreamConfigRange};

    use super::{
        AnalysisTap, OutputSampleFormat, PlaybackBuffer, VolumeControl, select_output_config,
    };

    #[test]
    fn output_selection_uses_the_track_rate_and_prefers_stereo_f32() {
        let ranges = [
            SupportedStreamConfigRange::new(
                2,
                44_100,
                96_000,
                SupportedBufferSize::Unknown,
                SampleFormat::I16,
            ),
            SupportedStreamConfigRange::new(
                2,
                44_100,
                96_000,
                SupportedBufferSize::Unknown,
                SampleFormat::F32,
            ),
        ];

        let selected = select_output_config(ranges, 48_000).unwrap();

        assert_eq!(selected.sample_rate(), 48_000);
        assert_eq!(selected.channels(), 2);
        assert_eq!(selected.sample_format(), SampleFormat::F32);
    }

    #[test]
    fn tap_is_bounded_and_drops_new_analysis_samples_on_overflow() {
        let (mut producer, mut consumer, diagnostics) = AnalysisTap::bounded(4);

        let report = producer.push_samples(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);

        assert_eq!(report.accepted(), 4);
        assert_eq!(report.dropped(), 2);
        assert_eq!(diagnostics.accepted(), 4);
        assert_eq!(diagnostics.dropped(), 2);

        let mut output = [0.0; 8];
        let count = consumer.pop_into(&mut output);
        assert_eq!(count, 4);
        assert_eq!(&output[..count], &[0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn tap_capacity_is_reused_after_the_worker_consumes_samples() {
        let (mut producer, mut consumer, diagnostics) = AnalysisTap::bounded(2);
        producer.push_samples(&[0.1, 0.2]);
        let mut output = [0.0; 2];
        assert_eq!(consumer.pop_into(&mut output), 2);

        let report = producer.push_samples(&[0.3, 0.4]);

        assert_eq!(report.accepted(), 2);
        assert_eq!(report.dropped(), 0);
        assert_eq!(diagnostics.accepted(), 4);
        assert_eq!(diagnostics.dropped(), 0);
    }

    #[test]
    fn empty_tap_returns_immediately_without_samples() {
        let (_producer, mut consumer, _diagnostics) = AnalysisTap::bounded(8);
        let mut output = [0.0; 8];

        assert_eq!(consumer.pop_into(&mut output), 0);
    }

    #[test]
    fn callback_applies_volume_taps_mono_and_writes_silence_on_underflow() {
        let (mut frame_producer, mut callback, mut analysis_consumer, diagnostics) =
            PlaybackBuffer::bounded(4, 8);
        let volume = callback.volume_control();
        volume.set(0.5);
        assert_eq!(frame_producer.push_frames(&[[1.0, -1.0], [0.5, 0.25]]), 2);
        let mut output = [99.0; 6];

        callback.render_f32(&mut output, 2);

        assert_eq!(output, [0.5, -0.5, 0.25, 0.125, 0.0, 0.0]);
        assert_eq!(diagnostics.underflow_frames(), 1);
        assert_eq!(diagnostics.analysis_accepted(), 3);
        assert_eq!(diagnostics.analysis_dropped(), 0);
        let mut tapped = [0.0; 3];
        assert_eq!(analysis_consumer.pop_into(&mut tapped), 3);
        assert_eq!(tapped, [0.0, 0.1875, 0.0]);
    }

    #[test]
    fn decode_side_observes_bounded_backpressure_without_dropping_old_frames() {
        let (mut producer, mut callback, _analysis_consumer, _diagnostics) =
            PlaybackBuffer::bounded(2, 4);

        assert_eq!(
            producer.push_frames(&[[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]),
            2
        );
        let mut output = [0.0; 4];
        callback.render_f32(&mut output, 2);

        assert_eq!(output, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn volume_control_clamps_non_finite_and_out_of_range_values() {
        let volume = VolumeControl::new();

        volume.set(2.0);
        assert_eq!(volume.get(), 1.0);
        volume.set(-0.2);
        assert_eq!(volume.get(), 0.0);
        volume.set(f32::NAN);
        assert_eq!(volume.get(), 0.0);
    }

    #[test]
    fn output_format_selection_is_explicit_and_rejects_unimplemented_formats() {
        assert_eq!(
            OutputSampleFormat::try_from(cpal::SampleFormat::F32).unwrap(),
            OutputSampleFormat::F32
        );
        assert_eq!(
            OutputSampleFormat::try_from(cpal::SampleFormat::I16).unwrap(),
            OutputSampleFormat::I16
        );
        assert_eq!(
            OutputSampleFormat::try_from(cpal::SampleFormat::U16).unwrap(),
            OutputSampleFormat::U16
        );
        assert!(OutputSampleFormat::try_from(cpal::SampleFormat::F64).is_err());
    }
}
