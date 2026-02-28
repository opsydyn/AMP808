/// Trait for audio sources that feed the player pipeline.
/// All sources produce stereo f32 frames at the player's sample rate.
pub trait AudioSource: Send {
    /// Fill buffer with stereo samples. Returns number of frames written.
    /// Returns 0 when source is exhausted.
    fn read(&mut self, buf: &mut [[f32; 2]]) -> usize;

    /// Total length in frames, or None for streams.
    fn len_frames(&self) -> Option<usize>;

    /// Current position in frames.
    fn position(&self) -> usize;

    /// Seek to frame position.
    fn seek(&mut self, frame: usize) -> anyhow::Result<()>;

    /// Whether seeking is supported.
    fn seekable(&self) -> bool;

    /// Sample rate of this source.
    fn sample_rate(&self) -> u32;
}
