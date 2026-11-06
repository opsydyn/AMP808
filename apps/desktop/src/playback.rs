use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::analysis::{
    AnalysisAvailability, AnalysisError, AnalysisSnapshot, AnalysisWorker, AnalyzerConfig,
    LatestAnalysis,
};
use crate::app::transport::{TransitionError, Transport, TransportEvent, TransportState};
use crate::audio::{
    AudioCallback, AudioDeviceError, FrameProducer, OutputStreamHandle, PlaybackBuffer,
    PlaybackDiagnostics, VolumeControl,
};
use crate::media::{DecodedChunk, MediaError, StreamingDecoder, TrackMetadata};
use crate::milkdrop::{MilkdropPcm, MilkdropPcmConsumer};

const FRAME_CAPACITY: usize = 16_384;
const ANALYSIS_CAPACITY: usize = 8_192;
const MILKDROP_CAPACITY: usize = 8_192;
const DECODE_CHUNK_SIZE: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeProgress {
    Wrote(usize),
    Backpressured,
    Ended,
}

#[derive(Debug)]
pub enum PlaybackError {
    InvalidChunkSize,
    Media(MediaError),
    Worker(String),
}

impl fmt::Display for PlaybackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChunkSize => {
                formatter.write_str("decode chunk size must be greater than zero")
            }
            Self::Media(error) => error.fmt(formatter),
            Self::Worker(message) => write!(formatter, "decode worker failed: {message}"),
        }
    }
}

impl std::error::Error for PlaybackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Media(error) => Some(error),
            Self::InvalidChunkSize | Self::Worker(_) => None,
        }
    }
}

impl From<MediaError> for PlaybackError {
    fn from(error: MediaError) -> Self {
        Self::Media(error)
    }
}

pub struct DecodePump {
    decoder: StreamingDecoder,
    producer: FrameProducer,
    chunk_size: usize,
    pending: Option<DecodedChunk>,
    pending_offset: usize,
}

impl DecodePump {
    pub fn new(
        decoder: StreamingDecoder,
        producer: FrameProducer,
        chunk_size: usize,
    ) -> Result<Self, PlaybackError> {
        if chunk_size == 0 {
            return Err(PlaybackError::InvalidChunkSize);
        }
        Ok(Self {
            decoder,
            producer,
            chunk_size,
            pending: None,
            pending_offset: 0,
        })
    }

    pub fn poll_once(&mut self) -> Result<DecodeProgress, PlaybackError> {
        if self.pending.is_none() {
            let Some(chunk) = self.decoder.next_chunk(self.chunk_size)? else {
                return Ok(DecodeProgress::Ended);
            };
            self.pending = Some(chunk);
            self.pending_offset = 0;
        }

        let pending = self
            .pending
            .as_ref()
            .expect("pending chunk was just loaded");
        let written = self
            .producer
            .push_frames(&pending.frames()[self.pending_offset..]);
        if written == 0 {
            return Ok(DecodeProgress::Backpressured);
        }

        self.pending_offset += written;
        if self.pending_offset == pending.frames().len() {
            self.pending = None;
            self.pending_offset = 0;
        }
        Ok(DecodeProgress::Wrote(written))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodeWorkerState {
    Starting,
    Running,
    Ended,
    Failed(String),
    Stopped,
}

#[derive(Clone)]
pub struct LatestDecodeStatus {
    state: Arc<Mutex<Arc<DecodeWorkerState>>>,
}

impl LatestDecodeStatus {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Arc::new(DecodeWorkerState::Starting))),
        }
    }

    fn publish(&self, state: DecodeWorkerState) -> bool {
        let Ok(mut current) = self.state.lock() else {
            return false;
        };
        *current = Arc::new(state);
        true
    }

    pub fn load(&self) -> Option<Arc<DecodeWorkerState>> {
        self.state.try_lock().ok().map(|state| Arc::clone(&state))
    }
}

pub struct DecodeWorker {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<Result<(), PlaybackError>>>,
}

impl DecodeWorker {
    pub fn spawn(
        decoder: StreamingDecoder,
        producer: FrameProducer,
        chunk_size: usize,
    ) -> Result<(Self, LatestDecodeStatus), PlaybackError> {
        let mut pump = DecodePump::new(decoder, producer, chunk_size)?;
        let status = LatestDecodeStatus::new();
        let thread_status = status.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = thread::Builder::new()
            .name("amp808-decode".to_owned())
            .spawn(move || {
                thread_status.publish(DecodeWorkerState::Running);
                while !thread_stop.load(Ordering::Acquire) {
                    match pump.poll_once() {
                        Ok(DecodeProgress::Wrote(_)) => {}
                        Ok(DecodeProgress::Backpressured) => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Ok(DecodeProgress::Ended) => {
                            thread_status.publish(DecodeWorkerState::Ended);
                            return Ok(());
                        }
                        Err(error) => {
                            thread_status.publish(DecodeWorkerState::Failed(error.to_string()));
                            return Err(error);
                        }
                    }
                }
                thread_status.publish(DecodeWorkerState::Stopped);
                Ok(())
            })
            .map_err(|error| PlaybackError::Worker(error.to_string()))?;

        Ok((
            Self {
                stop,
                join: Some(join),
            },
            status,
        ))
    }

    pub fn stop(mut self) -> Result<(), PlaybackError> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Result<(), PlaybackError> {
        self.stop.store(true, Ordering::Release);
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        join.join()
            .map_err(|_| PlaybackError::Worker("thread panicked".to_owned()))?
    }
}

impl Drop for DecodeWorker {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

trait AudioOutput: Send {
    fn volume_control(&self) -> VolumeControl;
    fn take_error(&self) -> Option<String>;
    fn play(&self) -> Result<(), AudioDeviceError>;
    fn pause(&self) -> Result<(), AudioDeviceError>;
}

impl AudioOutput for OutputStreamHandle {
    fn volume_control(&self) -> VolumeControl {
        self.volume_control()
    }

    fn take_error(&self) -> Option<String> {
        self.take_error()
    }

    fn play(&self) -> Result<(), AudioDeviceError> {
        self.play()
    }

    fn pause(&self) -> Result<(), AudioDeviceError> {
        self.pause()
    }
}

#[derive(Debug)]
pub enum PlaybackSessionError {
    Media(MediaError),
    Audio(AudioDeviceError),
    Analysis(AnalysisError),
    Playback(PlaybackError),
    Transport(TransitionError),
}

impl fmt::Display for PlaybackSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Media(error) => error.fmt(formatter),
            Self::Audio(error) => error.fmt(formatter),
            Self::Analysis(error) => error.fmt(formatter),
            Self::Playback(error) => error.fmt(formatter),
            Self::Transport(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PlaybackSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Media(error) => Some(error),
            Self::Audio(error) => Some(error),
            Self::Analysis(error) => Some(error),
            Self::Playback(error) => Some(error),
            Self::Transport(error) => Some(error),
        }
    }
}

impl From<MediaError> for PlaybackSessionError {
    fn from(error: MediaError) -> Self {
        Self::Media(error)
    }
}

impl From<AudioDeviceError> for PlaybackSessionError {
    fn from(error: AudioDeviceError) -> Self {
        Self::Audio(error)
    }
}

impl From<AnalysisError> for PlaybackSessionError {
    fn from(error: AnalysisError) -> Self {
        Self::Analysis(error)
    }
}

impl From<PlaybackError> for PlaybackSessionError {
    fn from(error: PlaybackError) -> Self {
        Self::Playback(error)
    }
}

impl From<TransitionError> for PlaybackSessionError {
    fn from(error: TransitionError) -> Self {
        Self::Transport(error)
    }
}

#[derive(Clone, Debug)]
pub struct PlaybackSessionSnapshot {
    analysis: Arc<AnalysisSnapshot>,
    decode: Arc<DecodeWorkerState>,
    output_error: Option<String>,
}

impl PlaybackSessionSnapshot {
    pub fn analysis(&self) -> &AnalysisSnapshot {
        &self.analysis
    }

    pub fn decode(&self) -> &DecodeWorkerState {
        &self.decode
    }

    pub fn output_error(&self) -> Option<&str> {
        self.output_error.as_deref()
    }
}

pub struct PlaybackSession {
    _decode_worker: DecodeWorker,
    _analysis_worker: AnalysisWorker,
    output: Box<dyn AudioOutput>,
    volume: VolumeControl,
    metadata: TrackMetadata,
    latest_analysis: LatestAnalysis,
    paused_analysis: Arc<AnalysisSnapshot>,
    decode_status: LatestDecodeStatus,
    transport: Transport,
    diagnostics: PlaybackDiagnostics,
    path: PathBuf,
    start_position_seconds: f64,
    milkdrop_pcm: Option<MilkdropPcmConsumer>,
}

impl PlaybackSession {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PlaybackSessionError> {
        let path = path.as_ref().to_path_buf();
        let (decoder, metadata) = StreamingDecoder::open(&path)?;
        Self::assemble(
            path,
            decoder,
            metadata,
            0.0,
            OutputStreamHandle::open_default,
        )
    }

    pub fn open_at(
        path: impl AsRef<Path>,
        position_seconds: f64,
    ) -> Result<Self, PlaybackSessionError> {
        let path = path.as_ref().to_path_buf();
        let (decoder, metadata) = StreamingDecoder::open_at(&path, position_seconds)?;
        Self::assemble(
            path,
            decoder,
            metadata,
            position_seconds,
            OutputStreamHandle::open_default,
        )
    }

    fn open_with_output<F, O>(
        path: impl AsRef<std::path::Path>,
        open_output: F,
    ) -> Result<Self, PlaybackSessionError>
    where
        F: FnOnce(AudioCallback, u32) -> Result<O, AudioDeviceError>,
        O: AudioOutput + 'static,
    {
        let path = path.as_ref().to_path_buf();
        let (decoder, metadata) = StreamingDecoder::open(&path)?;
        Self::assemble(path, decoder, metadata, 0.0, open_output)
    }

    fn open_at_with_output<F, O>(
        path: impl AsRef<Path>,
        position_seconds: f64,
        open_output: F,
    ) -> Result<Self, PlaybackSessionError>
    where
        F: FnOnce(AudioCallback, u32) -> Result<O, AudioDeviceError>,
        O: AudioOutput + 'static,
    {
        let path = path.as_ref().to_path_buf();
        let (decoder, metadata) = StreamingDecoder::open_at(&path, position_seconds)?;
        Self::assemble(path, decoder, metadata, position_seconds, open_output)
    }

    fn assemble<F, O>(
        path: PathBuf,
        decoder: StreamingDecoder,
        metadata: TrackMetadata,
        start_position_seconds: f64,
        open_output: F,
    ) -> Result<Self, PlaybackSessionError>
    where
        F: FnOnce(AudioCallback, u32) -> Result<O, AudioDeviceError>,
        O: AudioOutput + 'static,
    {
        let (producer, callback, analysis_consumer, diagnostics) =
            PlaybackBuffer::bounded(FRAME_CAPACITY, ANALYSIS_CAPACITY);
        let analysis_config = AnalyzerConfig::default();
        let latest_analysis = LatestAnalysis::new(analysis_config);
        let paused_analysis = Arc::new(AnalysisSnapshot::empty(
            AnalysisAvailability::Paused,
            0,
            analysis_config,
        ));
        let (milkdrop_producer, milkdrop_pcm) = MilkdropPcm::bounded(MILKDROP_CAPACITY);
        let analysis_worker = AnalysisWorker::spawn_with_milkdrop(
            analysis_consumer,
            latest_analysis.clone(),
            analysis_config,
            milkdrop_producer,
        )?;
        let output = open_output(callback, metadata.sample_rate())?;
        let volume = output.volume_control();
        let (decode_worker, decode_status) =
            DecodeWorker::spawn(decoder, producer, DECODE_CHUNK_SIZE)?;
        let mut transport = Transport::new();
        transport.apply(TransportEvent::LoadRequested)?;
        transport.apply(TransportEvent::Loaded)?;
        transport.apply(TransportEvent::PlayRequested)?;

        Ok(Self {
            _decode_worker: decode_worker,
            _analysis_worker: analysis_worker,
            output: Box::new(output),
            volume,
            metadata,
            latest_analysis,
            paused_analysis,
            decode_status,
            transport,
            diagnostics,
            path,
            start_position_seconds,
            milkdrop_pcm: Some(milkdrop_pcm),
        })
    }

    pub const fn metadata(&self) -> &TrackMetadata {
        &self.metadata
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn take_milkdrop_pcm(&mut self) -> Option<MilkdropPcmConsumer> {
        self.milkdrop_pcm.take()
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume.set(volume);
    }

    pub fn volume(&self) -> f32 {
        self.volume.get()
    }

    pub fn position_seconds(&self) -> f64 {
        self.start_position_seconds
            + self.diagnostics.rendered_frames() as f64 / f64::from(self.metadata.sample_rate())
    }

    pub fn underflow_frames(&self) -> u64 {
        self.diagnostics.underflow_frames()
    }

    pub const fn transport_state(&self) -> &TransportState {
        self.transport.state()
    }

    pub fn pause(&mut self) -> Result<(), PlaybackSessionError> {
        let mut next = self.transport.clone();
        next.apply(TransportEvent::PauseRequested)?;
        self.output.pause()?;
        self.transport = next;
        Ok(())
    }

    pub fn play(&mut self) -> Result<(), PlaybackSessionError> {
        let mut next = self.transport.clone();
        next.apply(TransportEvent::PlayRequested)?;
        self.output.play()?;
        self.transport = next;
        Ok(())
    }

    pub fn poll(&mut self) -> Result<(), PlaybackSessionError> {
        let decode_ended = self
            .decode_status
            .load()
            .is_some_and(|state| *state == DecodeWorkerState::Ended);
        let output_drained =
            self.diagnostics.rendered_frames() >= self.diagnostics.decoded_frames();
        if self.transport.state() == &TransportState::Playing && decode_ended && output_drained {
            let mut next = self.transport.clone();
            next.apply(TransportEvent::EndReached)?;
            self.output.pause()?;
            self.transport = next;
        }
        Ok(())
    }

    pub fn try_snapshot(&self) -> Option<PlaybackSessionSnapshot> {
        let analysis = if self.transport.state() == &TransportState::Paused {
            Arc::clone(&self.paused_analysis)
        } else {
            self.latest_analysis.load()?
        };
        let decode = self.decode_status.load()?;
        Some(PlaybackSessionSnapshot {
            analysis,
            decode,
            output_error: self.output.take_error(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use crate::analysis::AnalysisAvailability;
    use crate::app::transport::TransportState;
    use crate::audio::{AudioCallback, AudioDeviceError, PlaybackBuffer, VolumeControl};
    use crate::media::StreamingDecoder;

    use super::{
        AudioOutput, DecodeProgress, DecodePump, DecodeWorker, DecodeWorkerState, PlaybackSession,
    };

    #[test]
    fn playback_session_assembles_local_pcm_metadata_analysis_and_volume() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("session.wav");
        write_stereo_wav(&path, 8_000, 4_096);

        let session = PlaybackSession::open_with_output(&path, |callback, sample_rate| {
            assert_eq!(sample_rate, 8_000);
            Ok(TestOutput::open(callback))
        })
        .unwrap();

        assert_eq!(session.metadata().title(), "session");
        assert_eq!(session.metadata().sample_rate(), 8_000);
        assert_eq!(session.underflow_frames(), 0);
        session.set_volume(0.35);
        assert!((session.volume() - 0.35).abs() < f32::EPSILON);

        let deadline = Instant::now() + Duration::from_secs(1);
        let produced_analysis = loop {
            if session.try_snapshot().is_some_and(|snapshot| {
                snapshot.analysis().availability() == AnalysisAvailability::Available
                    && snapshot.analysis().sequence() > 0
            }) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::yield_now();
        };

        assert!(produced_analysis, "session did not publish PCM analysis");
    }

    #[test]
    fn playback_session_exposes_bounded_pcm_for_the_milkdrop_view() {
        let directory = tempdir().expect("temporary media directory is available");
        let path = directory.path().join("milkdrop-session.wav");
        write_stereo_wav(&path, 8_000, 8_192);
        let mut session =
            PlaybackSession::open_with_output(&path, |callback, _| Ok(TestOutput::open(callback)))
                .expect("test playback session opens");
        let mut milkdrop = session
            .take_milkdrop_pcm()
            .expect("new playback session owns a MilkDrop PCM consumer");
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut samples = [0.0; 2_048];

        let mut received = 0;
        while received < samples.len() {
            received += milkdrop.pop_into(&mut samples[received..]);
            assert!(
                Instant::now() < deadline,
                "complete MilkDrop PCM window did not arrive before timeout"
            );
            std::thread::yield_now();
        }

        assert_eq!(received, 2_048, "analysis forwards one complete PCM window");
        assert!(
            session.take_milkdrop_pcm().is_none(),
            "the bounded consumer has a single owner"
        );
    }

    #[test]
    fn playback_session_pauses_and_resumes_output_and_visualizers() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("pause-resume.wav");
        write_stereo_wav(&path, 8_000, 16_384);
        let output_playing = Arc::new(AtomicBool::new(true));
        let observed_output = Arc::clone(&output_playing);

        let mut session = PlaybackSession::open_with_output(&path, move |callback, _| {
            Ok(TestOutput::open_with_state(callback, output_playing))
        })
        .unwrap();

        assert_eq!(session.transport_state(), &TransportState::Playing);
        session.pause().unwrap();
        assert_eq!(session.transport_state(), &TransportState::Paused);
        assert!(!observed_output.load(Ordering::Acquire));

        let paused = session.try_snapshot().unwrap();
        assert_eq!(
            paused.analysis().availability(),
            AnalysisAvailability::Paused
        );
        assert!(
            paused
                .analysis()
                .waveform()
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert!(
            paused
                .analysis()
                .spectrum()
                .iter()
                .all(|sample| *sample == 0.0)
        );

        session.play().unwrap();
        assert_eq!(session.transport_state(), &TransportState::Playing);
        assert!(observed_output.load(Ordering::Acquire));
    }

    #[test]
    fn playback_session_reports_position_from_frames_rendered_by_output() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("position.wav");
        write_stereo_wav(&path, 8_000, 16_384);
        let session =
            PlaybackSession::open_with_output(&path, |callback, _| Ok(TestOutput::open(callback)))
                .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);

        while session.position_seconds() == 0.0 && Instant::now() < deadline {
            std::thread::yield_now();
        }

        assert!(session.position_seconds() > 0.0);
        assert!(session.position_seconds() <= session.metadata().duration_seconds().unwrap());
    }

    #[test]
    fn playback_session_ends_after_decoded_frames_are_drained() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("end-of-stream.wav");
        write_stereo_wav(&path, 8_000, 2_048);
        let output_playing = Arc::new(AtomicBool::new(true));
        let observed_output = Arc::clone(&output_playing);
        let mut session = PlaybackSession::open_with_output(&path, move |callback, _| {
            Ok(TestOutput::open_with_state(callback, output_playing))
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);

        while session.transport_state() != &TransportState::Ended && Instant::now() < deadline {
            session.poll().unwrap();
            let _ = session.try_snapshot();
            std::thread::yield_now();
        }

        assert_eq!(session.transport_state(), &TransportState::Ended);
        assert!(!observed_output.load(Ordering::Acquire));
    }

    #[test]
    fn playback_session_rebuilds_from_a_requested_seek_position() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("seek-session.wav");
        write_stereo_wav(&path, 8_000, 16_384);

        let session = PlaybackSession::open_at_with_output(&path, 0.25, |callback, _| {
            Ok(TestOutput::open(callback))
        })
        .unwrap();

        assert_eq!(session.path(), path.as_path());
        assert!(session.position_seconds() >= 0.25);
        assert_eq!(session.transport_state(), &TransportState::Playing);
    }

    struct TestOutput {
        volume: VolumeControl,
        playing: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }

    impl TestOutput {
        fn open(callback: AudioCallback) -> Self {
            Self::open_with_state(callback, Arc::new(AtomicBool::new(true)))
        }

        fn open_with_state(mut callback: AudioCallback, playing: Arc<AtomicBool>) -> Self {
            let volume = callback.volume_control();
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = Arc::clone(&stop);
            let thread_playing = Arc::clone(&playing);
            let join = std::thread::spawn(move || {
                let mut output = [0.0; 256];
                while !thread_stop.load(Ordering::Acquire) {
                    if thread_playing.load(Ordering::Acquire) {
                        callback.render_f32(&mut output, 2);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            Self {
                volume,
                playing,
                stop,
                join: Some(join),
            }
        }
    }

    impl AudioOutput for TestOutput {
        fn volume_control(&self) -> VolumeControl {
            self.volume.clone()
        }

        fn take_error(&self) -> Option<String> {
            None
        }

        fn play(&self) -> Result<(), AudioDeviceError> {
            self.playing.store(true, Ordering::Release);
            Ok(())
        }

        fn pause(&self) -> Result<(), AudioDeviceError> {
            self.playing.store(false, Ordering::Release);
            Ok(())
        }
    }

    impl Drop for TestOutput {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    #[test]
    fn decode_pump_preserves_pending_frames_while_the_audio_buffer_is_full() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("backpressure.wav");
        write_stereo_wav(&path, 8_000, 8);
        let (decoder, _) = StreamingDecoder::open(&path).unwrap();
        let (producer, mut callback, _analysis, _diagnostics) = PlaybackBuffer::bounded(2, 16);
        let mut pump = DecodePump::new(decoder, producer, 4).unwrap();

        assert_eq!(pump.poll_once().unwrap(), DecodeProgress::Wrote(2));
        assert_eq!(pump.poll_once().unwrap(), DecodeProgress::Backpressured);

        let mut output = [0.0; 4];
        callback.render_f32(&mut output, 2);
        assert_eq!(pump.poll_once().unwrap(), DecodeProgress::Wrote(2));

        let mut written = 4;
        loop {
            callback.render_f32(&mut output, 2);
            match pump.poll_once().unwrap() {
                DecodeProgress::Wrote(frames) => written += frames,
                DecodeProgress::Backpressured => {}
                DecodeProgress::Ended => break,
            }
        }

        assert_eq!(written, 8);
    }

    #[test]
    fn decode_worker_reaches_end_while_a_callback_drains_the_bounded_buffer() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("worker.wav");
        write_stereo_wav(&path, 8_000, 2_048);
        let (decoder, _) = StreamingDecoder::open(&path).unwrap();
        let (producer, mut callback, _analysis, _diagnostics) = PlaybackBuffer::bounded(64, 128);
        let (worker, status) = DecodeWorker::spawn(decoder, producer, 128).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut output = [0.0; 128];

        let ended = loop {
            callback.render_f32(&mut output, 2);
            if status
                .load()
                .is_some_and(|state| *state == DecodeWorkerState::Ended)
            {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::yield_now();
        };

        assert!(ended, "decode worker did not reach end before timeout");
        worker.stop().unwrap();
    }

    fn write_stereo_wav(path: &Path, sample_rate: u32, frame_count: u32) {
        let channel_count = 2_u16;
        let bits_per_sample = 16_u16;
        let bytes_per_sample = u32::from(bits_per_sample / 8);
        let data_size = frame_count * u32::from(channel_count) * bytes_per_sample;
        let mut bytes = Vec::with_capacity(44 + data_size as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channel_count.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(
            &(sample_rate * u32::from(channel_count) * bytes_per_sample).to_le_bytes(),
        );
        bytes.extend_from_slice(&(channel_count * (bits_per_sample / 8)).to_le_bytes());
        bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());

        for frame in 0..frame_count {
            let left = ((frame % 512) as i16).saturating_mul(32);
            let right = -left;
            bytes.extend_from_slice(&left.to_le_bytes());
            bytes.extend_from_slice(&right.to_le_bytes());
        }

        fs::write(path, bytes).unwrap();
    }
}
