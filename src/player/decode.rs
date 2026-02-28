use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::source::AudioSource;

/// Supported audio file extensions.
pub const SUPPORTED_EXTS: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "m4a", "aac", "m4b", "alac", "wma", "opus",
];

/// Extensions that require FFmpeg to decode.
const FFMPEG_EXTS: &[&str] = &["m4a", "aac", "m4b", "alac", "wma", "opus"];

/// Get the audio format extension for a path.
pub fn format_ext(path: &str) -> String {
    if crate::playlist::is_url(path) {
        if let Ok(url) = url::Url::parse(path) {
            let ext = Path::new(url.path())
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext.is_empty() || ext == "view" {
                if let Some(f) = url.query_pairs().find(|(k, _)| k == "format") {
                    return f.1.to_lowercase();
                }
                return "mp3".to_string();
            }
            return ext;
        }
        return "mp3".to_string();
    }
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp3")
        .to_lowercase()
}

/// Check if the extension needs FFmpeg.
pub fn needs_ffmpeg(ext: &str) -> bool {
    FFMPEG_EXTS.contains(&ext)
}

/// Decode a local audio file using Symphonia, returning an AudioSource.
pub fn decode_file(path: &str, target_sr: u32) -> anyhow::Result<Box<dyn AudioSource>> {
    let ext = format_ext(path);

    // Try FFmpeg first for formats it handles better
    if needs_ffmpeg(&ext) {
        return super::ffmpeg::decode_ffmpeg(path, target_sr);
    }

    // Try Symphonia
    match decode_symphonia(path, target_sr) {
        Ok(source) => Ok(source),
        Err(_) => {
            // Fallback to FFmpeg for anything Symphonia can't handle
            super::ffmpeg::decode_ffmpeg(path, target_sr)
        }
    }
}

/// Decode using Symphonia's native decoders.
fn decode_symphonia(path: &str, target_sr: u32) -> anyhow::Result<Box<dyn AudioSource>> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow::anyhow!("no audio track found"))?;

    let codec_params = track.codec_params.clone();
    let track_id = track.id;
    let source_sr = codec_params.sample_rate.unwrap_or(44100);
    let channels = codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let decoder =
        symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;

    // Decode entire file into memory for seekability
    let samples = decode_all_samples(format, decoder, track_id, channels)?;

    // Resample if needed
    let samples = if source_sr != target_sr {
        resample(&samples, source_sr, target_sr)
    } else {
        samples
    };

    Ok(Box::new(PcmSource::new(samples, target_sr)))
}

/// Decode all samples from a format reader into stereo f32 frames.
fn decode_all_samples(
    mut format: Box<dyn FormatReader>,
    mut decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    channels: usize,
) -> anyhow::Result<Vec<[f32; 2]>> {
    let mut samples = Vec::new();
    let mut sample_buf = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Initialize sample buffer on first decode
        if sample_buf.is_none() {
            let spec = *decoded.spec();
            let duration = decoded.capacity();
            sample_buf = Some(SampleBuffer::<f32>::new(duration as u64, spec));
        }

        let buf = sample_buf.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);
        let interleaved = buf.samples();

        // Convert interleaved samples to stereo frames
        let frame_count = interleaved.len() / channels.max(1);
        for i in 0..frame_count {
            let left = interleaved[i * channels];
            let right = if channels >= 2 {
                interleaved[i * channels + 1]
            } else {
                left
            };
            samples.push([left, right]);
        }
    }

    Ok(samples)
}

/// Simple linear resampler for changing sample rates.
fn resample(samples: &[[f32; 2]], source_sr: u32, target_sr: u32) -> Vec<[f32; 2]> {
    if source_sr == target_sr || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = target_sr as f64 / source_sr as f64;
    let out_len = (samples.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;

        if idx + 1 < samples.len() {
            let l = samples[idx][0] as f64 * (1.0 - frac) + samples[idx + 1][0] as f64 * frac;
            let r = samples[idx][1] as f64 * (1.0 - frac) + samples[idx + 1][1] as f64 * frac;
            out.push([l as f32, r as f32]);
        } else if idx < samples.len() {
            out.push(samples[idx]);
        }
    }
    out
}

/// In-memory PCM audio source — fully seekable.
pub struct PcmSource {
    samples: Vec<[f32; 2]>,
    pos: usize,
    sample_rate: u32,
}

impl PcmSource {
    pub fn new(samples: Vec<[f32; 2]>, sample_rate: u32) -> Self {
        Self {
            samples,
            pos: 0,
            sample_rate,
        }
    }
}

impl AudioSource for PcmSource {
    fn read(&mut self, buf: &mut [[f32; 2]]) -> usize {
        let remaining = self.samples.len() - self.pos;
        let n = buf.len().min(remaining);
        buf[..n].copy_from_slice(&self.samples[self.pos..self.pos + n]);
        self.pos += n;
        n
    }

    fn len_frames(&self) -> Option<usize> {
        Some(self.samples.len())
    }

    fn position(&self) -> usize {
        self.pos
    }

    fn seek(&mut self, frame: usize) -> anyhow::Result<()> {
        if frame > self.samples.len() {
            anyhow::bail!(
                "seek position {} out of range [0, {}]",
                frame,
                self.samples.len()
            );
        }
        self.pos = frame;
        Ok(())
    }

    fn seekable(&self) -> bool {
        true
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_ext_local() {
        assert_eq!(format_ext("/path/to/song.mp3"), "mp3");
        assert_eq!(format_ext("/path/to/song.FLAC"), "flac");
        assert_eq!(format_ext("/path/to/song.ogg"), "ogg");
    }

    #[test]
    fn test_format_ext_url() {
        assert_eq!(format_ext("https://example.com/audio/song.flac"), "flac");
        assert_eq!(
            format_ext("https://example.com/stream"),
            "mp3" // default
        );
    }

    #[test]
    fn test_needs_ffmpeg() {
        assert!(needs_ffmpeg("m4a"));
        assert!(needs_ffmpeg("aac"));
        assert!(needs_ffmpeg("opus"));
        assert!(!needs_ffmpeg("mp3"));
        assert!(!needs_ffmpeg("flac"));
        assert!(!needs_ffmpeg("ogg"));
    }

    #[test]
    fn test_pcm_source_basic() {
        let samples = vec![[0.5, -0.5], [0.3, -0.3], [0.1, -0.1]];
        let mut src = PcmSource::new(samples, 44100);

        assert_eq!(src.len_frames(), Some(3));
        assert_eq!(src.position(), 0);
        assert!(src.seekable());
        assert_eq!(src.sample_rate(), 44100);

        let mut buf = [[0.0f32; 2]; 2];
        let n = src.read(&mut buf);
        assert_eq!(n, 2);
        assert_eq!(buf[0], [0.5, -0.5]);
        assert_eq!(buf[1], [0.3, -0.3]);
        assert_eq!(src.position(), 2);

        let n = src.read(&mut buf);
        assert_eq!(n, 1);
        assert_eq!(buf[0], [0.1, -0.1]);
    }

    #[test]
    fn test_pcm_source_seek() {
        let samples = vec![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        let mut src = PcmSource::new(samples, 44100);

        src.seek(2).unwrap();
        assert_eq!(src.position(), 2);

        let mut buf = [[0.0f32; 2]; 2];
        let n = src.read(&mut buf);
        assert_eq!(n, 1);
        assert_eq!(buf[0], [3.0, 3.0]);
    }

    #[test]
    fn test_pcm_source_seek_out_of_range() {
        let samples = vec![[1.0, 1.0]];
        let mut src = PcmSource::new(samples, 44100);
        assert!(src.seek(5).is_err());
    }

    #[test]
    fn test_resample_identity() {
        let samples = vec![[1.0, 2.0], [3.0, 4.0]];
        let out = resample(&samples, 44100, 44100);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn test_resample_upsample() {
        let samples = vec![[0.0, 0.0], [1.0, 1.0]];
        let out = resample(&samples, 22050, 44100);
        assert!(out.len() > samples.len());
    }

    #[test]
    fn test_resample_empty() {
        let out = resample(&[], 44100, 48000);
        assert!(out.is_empty());
    }
}
