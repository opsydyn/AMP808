use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTag};
use symphonia::core::units::Time;

#[derive(Clone, Debug, PartialEq)]
pub struct TrackMetadata {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    sample_rate: u32,
    channels: usize,
    duration_seconds: Option<f64>,
}

impl TrackMetadata {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    pub fn album(&self) -> Option<&str> {
        self.album.as_deref()
    }

    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub const fn channels(&self) -> usize {
        self.channels
    }

    pub const fn duration_seconds(&self) -> Option<f64> {
        self.duration_seconds
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedChunk {
    frames: Box<[[f32; 2]]>,
    sample_rate: u32,
}

impl DecodedChunk {
    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }

    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

#[derive(Debug)]
pub struct MediaError {
    path: PathBuf,
    message: String,
}

impl MediaError {
    fn new(path: &Path, error: impl fmt::Display) -> Self {
        Self {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    }
}

impl fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for MediaError {}

pub struct StreamingDecoder {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    sample_rate: u32,
    pending: VecDeque<[f32; 2]>,
    ended: bool,
}

struct ProbedMedia {
    format: Box<dyn FormatReader>,
    codec_params: AudioCodecParameters,
    track_id: u32,
    metadata: TrackMetadata,
}

pub(crate) fn probe_metadata(path: impl AsRef<Path>) -> Result<TrackMetadata, MediaError> {
    probe(path.as_ref()).map(|probed| probed.metadata)
}

impl fmt::Debug for StreamingDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamingDecoder")
            .field("path", &self.path)
            .field("track_id", &self.track_id)
            .field("sample_rate", &self.sample_rate)
            .field("pending_frames", &self.pending.len())
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl StreamingDecoder {
    pub fn open(path: impl AsRef<Path>) -> Result<(Self, TrackMetadata), MediaError> {
        let path = path.as_ref();
        let ProbedMedia {
            format,
            codec_params,
            track_id,
            metadata,
        } = probe(path)?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
            .map_err(|error| MediaError::new(path, error))?;
        let streaming = Self {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            sample_rate: metadata.sample_rate,
            pending: VecDeque::new(),
            ended: false,
        };
        Ok((streaming, metadata))
    }

    pub fn open_at(
        path: impl AsRef<Path>,
        position_seconds: f64,
    ) -> Result<(Self, TrackMetadata), MediaError> {
        let path = path.as_ref();
        let time = Time::try_from_secs_f64(position_seconds)
            .filter(|time| !time.is_negative())
            .ok_or_else(|| {
                MediaError::new(path, "seek position must be finite and non-negative")
            })?;
        let (mut decoder, metadata) = Self::open(path)?;
        decoder
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(decoder.track_id),
                },
            )
            .map_err(|error| MediaError::new(path, error))?;
        decoder.decoder.reset();
        decoder.pending.clear();
        decoder.ended = false;
        Ok((decoder, metadata))
    }

    pub fn next_chunk(&mut self, max_frames: usize) -> Result<Option<DecodedChunk>, MediaError> {
        if max_frames == 0 {
            return Err(MediaError::new(
                &self.path,
                "decode chunk size must be greater than zero",
            ));
        }

        while self.pending.is_empty() && !self.ended {
            let Some(packet) = self
                .format
                .next_packet()
                .map_err(|error| MediaError::new(&self.path, error))?
            else {
                self.ended = true;
                break;
            };
            if packet.track_id != self.track_id {
                continue;
            }

            let decoded = self
                .decoder
                .decode(&packet)
                .map_err(|error| MediaError::new(&self.path, error))?;
            self.sample_rate = decoded.spec().rate();
            let channel_count = decoded.spec().channels().count();
            let mut interleaved = Vec::<f32>::with_capacity(decoded.samples_interleaved());
            decoded.copy_to_vec_interleaved(&mut interleaved);
            for frame in interleaved.chunks_exact(channel_count) {
                let left = frame[0];
                let right = frame.get(1).copied().unwrap_or(left);
                self.pending.push_back([left, right]);
            }
        }

        if self.pending.is_empty() {
            return Ok(None);
        }

        let frame_count = max_frames.min(self.pending.len());
        let frames = self.pending.drain(..frame_count).collect::<Vec<_>>();
        Ok(Some(DecodedChunk {
            frames: frames.into_boxed_slice(),
            sample_rate: self.sample_rate,
        }))
    }
}

fn probe(path: &Path) -> Result<ProbedMedia, MediaError> {
    let file = File::open(path).map_err(|error| MediaError::new(path, error))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| MediaError::new(path, error))?;

    let (track_id, codec_params, sample_rate, channels, duration_seconds) = {
        let track = format
            .first_track_known_codec(TrackType::Audio)
            .ok_or_else(|| MediaError::new(path, "no supported audio track"))?;
        let codec_params = match track.codec_params.as_ref() {
            Some(CodecParameters::Audio(params)) => params.clone(),
            _ => return Err(MediaError::new(path, "audio track has no codec parameters")),
        };
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| MediaError::new(path, "audio track has no sample rate"))?;
        let channels = codec_params
            .channels
            .as_ref()
            .map(|channels| channels.count())
            .ok_or_else(|| MediaError::new(path, "audio track has no channel layout"))?;
        let duration_seconds = track
            .time_base
            .zip(track.duration)
            .and_then(|(time_base, duration)| time_base.calc_duration(duration))
            .map(|time| time.as_secs_f64())
            .or_else(|| {
                track
                    .num_frames
                    .map(|frames| frames as f64 / f64::from(sample_rate))
            });
        (
            track.id,
            codec_params,
            sample_rate,
            channels,
            duration_seconds,
        )
    };

    let (tag_title, artist, album) = format
        .metadata()
        .current()
        .map(extract_text_metadata)
        .unwrap_or_default();
    let title = tag_title.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Unknown track")
            .to_string()
    });

    Ok(ProbedMedia {
        format,
        codec_params,
        track_id,
        metadata: TrackMetadata {
            title,
            artist,
            album,
            sample_rate,
            channels,
            duration_seconds,
        },
    })
}

fn extract_text_metadata(
    revision: &MetadataRevision,
) -> (Option<String>, Option<String>, Option<String>) {
    let mut title = None;
    let mut artist = None;
    let mut album = None;
    for tag in &revision.media.tags {
        match &tag.std {
            Some(StandardTag::TrackTitle(value)) => title = Some(value.as_ref().clone()),
            Some(StandardTag::Artist(value)) => artist = Some(value.as_ref().clone()),
            Some(StandardTag::Album(value)) => album = Some(value.as_ref().clone()),
            _ => {}
        }
    }
    (title, artist, album)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::StreamingDecoder;

    #[test]
    fn local_wav_reports_technical_metadata_and_filename_title() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("fixture-tone.wav");
        write_stereo_wav(&path, 8_000, 1_024);

        let (_decoder, metadata) = StreamingDecoder::open(&path).unwrap();

        assert_eq!(metadata.title(), "fixture-tone");
        assert_eq!(metadata.artist(), None);
        assert_eq!(metadata.album(), None);
        assert_eq!(metadata.sample_rate(), 8_000);
        assert_eq!(metadata.channels(), 2);
        assert!((metadata.duration_seconds().unwrap() - 0.128).abs() < 0.001);
    }

    #[test]
    fn decoder_streams_bounded_stereo_chunks_until_end_of_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("bounded.wav");
        write_stereo_wav(&path, 8_000, 4_097);
        let (mut decoder, _metadata) = StreamingDecoder::open(&path).unwrap();
        let mut decoded_frames = 0;
        let mut chunk_count = 0;

        while let Some(chunk) = decoder.next_chunk(256).unwrap() {
            assert!(!chunk.frames().is_empty());
            assert!(chunk.frames().len() <= 256);
            assert_eq!(chunk.sample_rate(), 8_000);
            decoded_frames += chunk.frames().len();
            chunk_count += 1;
        }

        assert_eq!(decoded_frames, 4_097);
        assert!(chunk_count > 1);
    }

    #[test]
    fn corrupt_local_media_returns_an_honest_error() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("broken.wav");
        fs::write(&path, b"this is not audio").unwrap();

        let error = StreamingDecoder::open(&path).unwrap_err();

        assert!(error.to_string().contains("broken.wav"));
    }

    #[test]
    fn seek_rebuild_opens_a_fresh_decoder_at_the_requested_position() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("seek.wav");
        write_stereo_wav(&path, 8_000, 8_000);
        let (mut from_start, _) = StreamingDecoder::open(&path).unwrap();
        let (mut from_seek, _) = StreamingDecoder::open_at(&path, 0.25).unwrap();

        let start_sample = from_start.next_chunk(1).unwrap().unwrap().frames()[0][0];
        let seek_sample = from_seek.next_chunk(1).unwrap().unwrap().frames()[0][0];

        assert_eq!(start_sample, 0.0);
        assert!(seek_sample.abs() > 0.1);
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
