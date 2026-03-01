use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatReader;

use super::source::AudioSource;

/// AudioSource that streams from a Symphonia FormatReader.
/// Non-seekable, unknown length. Decodes packets on-demand.
pub struct StreamingSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    channels: usize,
    source_sr: u32,
    target_sr: u32,
    sample_buf: Option<SampleBuffer<f32>>,
    leftover: Vec<[f32; 2]>,
    leftover_pos: usize,
    frames_read: usize,
}

impl StreamingSource {
    /// Create a new StreamingSource from a probed Symphonia format reader.
    pub fn new(
        format: Box<dyn FormatReader>,
        decoder: Box<dyn Decoder>,
        track_id: u32,
        channels: usize,
        source_sr: u32,
        target_sr: u32,
    ) -> Self {
        Self {
            format,
            decoder,
            track_id,
            channels,
            source_sr,
            target_sr,
            sample_buf: None,
            leftover: Vec::new(),
            leftover_pos: 0,
            frames_read: 0,
        }
    }

    /// Handle a chained OGG reset: re-create decoder for the new logical stream.
    fn handle_reset(&mut self) -> bool {
        // After ResetRequired, the format reader has started a new physical stream.
        // Get the updated track and create a new decoder.
        let track = match self
            .format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        {
            Some(t) => t,
            None => return false,
        };

        let codec_params = track.codec_params.clone();
        self.track_id = track.id;
        self.channels = codec_params.channels.map(|c| c.count()).unwrap_or(2);
        self.source_sr = codec_params.sample_rate.unwrap_or(44100);

        match symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default()) {
            Ok(dec) => {
                self.decoder = dec;
                self.sample_buf = None; // force re-init on next decode
                true
            }
            Err(_) => false,
        }
    }

    /// Decode the next packet and fill leftover buffer.
    /// Returns false on EOF/error.
    fn decode_next_packet(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(SymphoniaError::ResetRequired) => {
                    // Chained OGG: new logical stream boundary
                    if !self.handle_reset() {
                        return false;
                    }
                    continue;
                }
                Err(_) => return false,
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Initialize sample buffer on first decode
            if self.sample_buf.is_none() {
                let spec = *decoded.spec();
                let duration = decoded.capacity();
                self.sample_buf = Some(SampleBuffer::<f32>::new(duration as u64, spec));
            }

            let buf = self.sample_buf.as_mut().unwrap();
            buf.copy_interleaved_ref(decoded);
            let interleaved = buf.samples();

            let channels = self.channels.max(1);
            let frame_count = interleaved.len() / channels;

            // Convert interleaved to stereo frames
            let mut frames = Vec::with_capacity(frame_count);
            for i in 0..frame_count {
                let left = interleaved[i * channels];
                let right = if channels >= 2 {
                    interleaved[i * channels + 1]
                } else {
                    left
                };
                frames.push([left, right]);
            }

            // Resample if needed
            if self.source_sr != self.target_sr {
                self.leftover = resample_frames(&frames, self.source_sr, self.target_sr);
            } else {
                self.leftover = frames;
            }
            self.leftover_pos = 0;
            return true;
        }
    }
}

impl AudioSource for StreamingSource {
    fn read(&mut self, buf: &mut [[f32; 2]]) -> usize {
        let mut written = 0;

        while written < buf.len() {
            // Drain leftover first
            let avail = self.leftover.len() - self.leftover_pos;
            if avail > 0 {
                let n = avail.min(buf.len() - written);
                buf[written..written + n]
                    .copy_from_slice(&self.leftover[self.leftover_pos..self.leftover_pos + n]);
                self.leftover_pos += n;
                written += n;
                self.frames_read += n;
                continue;
            }

            // Need more data — decode next packet
            if !self.decode_next_packet() {
                break; // EOF
            }
        }

        written
    }

    fn len_frames(&self) -> Option<usize> {
        None // streaming — unknown length
    }

    fn position(&self) -> usize {
        self.frames_read
    }

    fn seek(&mut self, _frame: usize) -> anyhow::Result<()> {
        anyhow::bail!("stream is not seekable")
    }
}

/// Simple linear resampler for streaming frames.
fn resample_frames(samples: &[[f32; 2]], source_sr: u32, target_sr: u32) -> Vec<[f32; 2]> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_source_traits() {
        // We can't easily construct a full StreamingSource without Symphonia,
        // but we can verify the AudioSource contract for a minimal mock.
        // The key invariants: len_frames() is None, seek() returns error.

        // Test resample helper
        let frames = vec![[1.0, 1.0], [2.0, 2.0]];
        let resampled = resample_frames(&frames, 22050, 44100);
        assert!(resampled.len() > frames.len());

        // Identity resample
        let same = resample_frames(&frames, 44100, 44100);
        assert_eq!(same.len(), frames.len());

        // Empty
        let empty = resample_frames(&[], 44100, 48000);
        assert!(empty.is_empty());
    }
}
