use std::path::Path;
use std::process::Command;

use super::decode::PcmSource;
use super::source::AudioSource;

/// Decode audio using FFmpeg, returning an in-memory seekable PCM source.
/// Command: `ffmpeg -i <path> -f f32le -ar <sr> -ac 2 -loglevel error pipe:1`
pub fn decode_ffmpeg(path: &str, target_sr: u32) -> anyhow::Result<Box<dyn AudioSource>> {
    // Check ffmpeg is available
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| {
            let ext = Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("audio");
            anyhow::anyhow!(
                "ffmpeg is required to play .{ext} files — install it with your package manager"
            )
        })?;

    let output = Command::new("ffmpeg")
        .args([
            "-i",
            path,
            "-f",
            "f32le",
            "-ar",
            &target_sr.to_string(),
            "-ac",
            "2",
            "-loglevel",
            "error",
            "pipe:1",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg decode: {}", stderr.trim());
    }

    let data = output.stdout;
    // f32le stereo: 8 bytes per frame (2 channels * 4 bytes)
    let frame_count = data.len() / 8;
    let mut samples = Vec::with_capacity(frame_count);

    for i in 0..frame_count {
        let off = i * 8;
        let left = f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        let right =
            f32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        samples.push([left, right]);
    }

    Ok(Box::new(PcmSource::new(samples, target_sr)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f32le_parsing() {
        // Manually construct f32le bytes for known values
        let left: f32 = 0.5;
        let right: f32 = -0.25;
        let mut data = Vec::new();
        data.extend_from_slice(&left.to_le_bytes());
        data.extend_from_slice(&right.to_le_bytes());

        let frame_count = data.len() / 8;
        assert_eq!(frame_count, 1);

        let off = 0;
        let l = f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        let r = f32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        assert!((l - 0.5).abs() < 1e-6);
        assert!((r - (-0.25)).abs() < 1e-6);
    }
}
