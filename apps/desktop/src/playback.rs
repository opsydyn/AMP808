use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::audio::FrameProducer;
use crate::media::{DecodedChunk, MediaError, StreamingDecoder};

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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use crate::audio::PlaybackBuffer;
    use crate::media::StreamingDecoder;

    use super::{DecodeProgress, DecodePump, DecodeWorker, DecodeWorkerState};

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
