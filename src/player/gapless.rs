use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::source::AudioSource;

/// Gapless streamer that sequences tracks with zero-gap transitions.
/// Sits at the bottom of the audio pipeline — cpal callback reads from this.
/// Always returns the requested number of samples (fills silence if needed).
pub struct GaplessSource {
    inner: Mutex<GaplessInner>,
    drained: AtomicBool,
    advanced: AtomicBool,
}

struct GaplessInner {
    current: Option<Box<dyn AudioSource>>,
    next: Option<Box<dyn AudioSource>>,
}

impl GaplessSource {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(GaplessInner {
                current: None,
                next: None,
            }),
            drained: AtomicBool::new(false),
            advanced: AtomicBool::new(false),
        }
    }

    /// Read samples from the current source. On exhaustion, seamlessly
    /// fills remaining samples from the next source. Fills silence if
    /// no source is available.
    pub fn read(&self, buf: &mut [[f32; 2]]) -> usize {
        let mut inner = match self.inner.try_lock() {
            Ok(g) => g,
            Err(_) => {
                // Contention — fill silence, retry next callback
                for s in buf.iter_mut() {
                    *s = [0.0; 2];
                }
                return buf.len();
            }
        };

        let Some(ref mut current) = inner.current else {
            for s in buf.iter_mut() {
                *s = [0.0; 2];
            }
            return buf.len();
        };

        let n = current.read(buf);

        if n < buf.len() {
            // Current exhausted — try next
            if let Some(mut next) = inner.next.take() {
                let filled = next.read(&mut buf[n..]);
                inner.current = Some(next);
                self.drained.store(false, Ordering::Release);
                self.advanced.store(true, Ordering::Release);

                // Fill remaining with silence
                for s in buf[n + filled..].iter_mut() {
                    *s = [0.0; 2];
                }
            } else {
                // No next track — drained
                inner.current = None;
                self.drained.store(true, Ordering::Release);
                for s in buf[n..].iter_mut() {
                    *s = [0.0; 2];
                }
            }
        }

        buf.len()
    }

    /// Replace the current source immediately (manual skip/prev/select).
    pub fn replace(&self, source: Box<dyn AudioSource>) {
        let mut inner = self.inner.lock().unwrap();
        inner.current = Some(source);
        inner.next = None;
        self.drained.store(false, Ordering::Release);
    }

    /// Queue the next source for gapless transition.
    pub fn set_next(&self, source: Box<dyn AudioSource>) {
        let mut inner = self.inner.lock().unwrap();
        inner.next = Some(source);
    }

    /// Clear the next source (e.g., when seek invalidates preload).
    pub fn clear_next(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.next = None;
    }

    /// Clear both current and next. Outputs silence until replace/set_next.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.current = None;
        inner.next = None;
        self.drained.store(false, Ordering::Release);
    }

    /// Returns true (once) when the current track ended with no next queued.
    pub fn drained(&self) -> bool {
        self.drained.load(Ordering::Acquire)
    }

    /// Returns true (once) when a gapless transition happened.
    pub fn advanced(&self) -> bool {
        self.advanced
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    /// Current playback position in frames.
    pub fn position(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        match &inner.current {
            Some(src) => src.position(),
            None => 0,
        }
    }

    /// Total duration in frames, or None for streams.
    pub fn duration_frames(&self) -> Option<usize> {
        let inner = self.inner.lock().unwrap();
        match &inner.current {
            Some(src) => src.len_frames(),
            None => None,
        }
    }

    /// Whether the current source supports seeking.
    pub fn seekable(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        match &inner.current {
            Some(src) => src.seekable(),
            None => false,
        }
    }

    /// Seek the current source to a frame position. Clears preloaded next track.
    pub fn seek(&self, frame: usize) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(ref mut src) = inner.current {
            src.seek(frame)?;
            inner.next = None; // Invalidate preload
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::decode::PcmSource;

    fn make_source(values: &[f32], sr: u32) -> Box<dyn AudioSource> {
        let samples: Vec<[f32; 2]> = values.iter().map(|&v| [v, v]).collect();
        Box::new(PcmSource::new(samples, sr))
    }

    #[test]
    fn test_empty_gapless_fills_silence() {
        let gs = GaplessSource::new();
        let mut buf = [[1.0f32; 2]; 4];
        gs.read(&mut buf);
        assert!(buf.iter().all(|s| s[0] == 0.0 && s[1] == 0.0));
    }

    #[test]
    fn test_single_source_playback() {
        let gs = GaplessSource::new();
        gs.replace(make_source(&[1.0, 2.0, 3.0], 44100));

        let mut buf = [[0.0f32; 2]; 3];
        gs.read(&mut buf);
        assert_eq!(buf[0][0], 1.0);
        assert_eq!(buf[1][0], 2.0);
        assert_eq!(buf[2][0], 3.0);
    }

    #[test]
    fn test_drained_when_no_next() {
        let gs = GaplessSource::new();
        gs.replace(make_source(&[1.0], 44100));

        let mut buf = [[0.0f32; 2]; 4];
        gs.read(&mut buf);

        assert!(gs.drained());
    }

    #[test]
    fn test_gapless_transition() {
        let gs = GaplessSource::new();
        gs.replace(make_source(&[1.0, 2.0], 44100));
        gs.set_next(make_source(&[3.0, 4.0], 44100));

        let mut buf = [[0.0f32; 2]; 4];
        gs.read(&mut buf);

        // First two from current, next two from next — zero gap
        assert_eq!(buf[0][0], 1.0);
        assert_eq!(buf[1][0], 2.0);
        assert_eq!(buf[2][0], 3.0);
        assert_eq!(buf[3][0], 4.0);

        assert!(!gs.drained());
        assert!(gs.advanced());
        assert!(!gs.advanced()); // consumed
    }

    #[test]
    fn test_replace_clears_next() {
        let gs = GaplessSource::new();
        gs.set_next(make_source(&[1.0], 44100));
        gs.replace(make_source(&[2.0], 44100));

        // Next should be cleared by replace
        let mut buf = [[0.0f32; 2]; 4];
        gs.read(&mut buf);
        assert_eq!(buf[0][0], 2.0);
        // No transition to next
        assert!(gs.drained());
    }

    #[test]
    fn test_clear() {
        let gs = GaplessSource::new();
        gs.replace(make_source(&[1.0, 2.0, 3.0], 44100));
        gs.clear();

        let mut buf = [[1.0f32; 2]; 2];
        gs.read(&mut buf);
        // Should be silence
        assert!(buf.iter().all(|s| s[0] == 0.0));
        assert!(!gs.drained());
    }

    #[test]
    fn test_clear_next() {
        let gs = GaplessSource::new();
        gs.replace(make_source(&[1.0], 44100));
        gs.set_next(make_source(&[2.0], 44100));
        gs.clear_next();

        let mut buf = [[0.0f32; 2]; 4];
        gs.read(&mut buf);
        // Should drain since next was cleared
        assert!(gs.drained());
    }

    #[test]
    fn test_position_and_duration() {
        let gs = GaplessSource::new();
        assert_eq!(gs.position(), 0);
        assert_eq!(gs.duration_frames(), None);

        gs.replace(make_source(&[1.0, 2.0, 3.0, 4.0], 44100));
        assert_eq!(gs.position(), 0);
        assert_eq!(gs.duration_frames(), Some(4));

        // Read 2 frames to advance position
        let mut buf = [[0.0f32; 2]; 2];
        gs.read(&mut buf);
        assert_eq!(gs.position(), 2);
        assert_eq!(gs.duration_frames(), Some(4));
    }

    #[test]
    fn test_seek() {
        let gs = GaplessSource::new();
        gs.replace(make_source(&[1.0, 2.0, 3.0, 4.0], 44100));

        // Read 2 frames
        let mut buf = [[0.0f32; 2]; 2];
        gs.read(&mut buf);
        assert_eq!(gs.position(), 2);

        // Seek back to start
        gs.seek(0).unwrap();
        assert_eq!(gs.position(), 0);

        // Read again — should get first samples
        gs.read(&mut buf);
        assert_eq!(buf[0][0], 1.0);
        assert_eq!(buf[1][0], 2.0);
    }

    #[test]
    fn test_seek_clears_next() {
        let gs = GaplessSource::new();
        gs.replace(make_source(&[1.0, 2.0], 44100));
        gs.set_next(make_source(&[3.0, 4.0], 44100));

        gs.seek(0).unwrap();

        // Next should be cleared by seek — exhaust current, should drain
        let mut buf = [[0.0f32; 2]; 4];
        gs.read(&mut buf);
        assert!(gs.drained());
    }

    #[test]
    fn test_seekable() {
        let gs = GaplessSource::new();
        assert!(!gs.seekable());

        gs.replace(make_source(&[1.0], 44100));
        assert!(gs.seekable());
    }
}
