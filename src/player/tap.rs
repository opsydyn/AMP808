use std::sync::Mutex;

/// Ring buffer that captures mono-mixed audio samples for FFT visualization.
pub struct Tap {
    buf: Mutex<TapBuffer>,
}

struct TapBuffer {
    data: Vec<f64>,
    pos: usize,
    size: usize,
}

impl Tap {
    pub fn new(buf_size: usize) -> Self {
        Self {
            buf: Mutex::new(TapBuffer {
                data: vec![0.0; buf_size],
                pos: 0,
                size: buf_size,
            }),
        }
    }

    /// Capture stereo samples as mono mix into the ring buffer.
    pub fn write(&self, samples: &[[f32; 2]]) {
        let mut buf = self.buf.lock().unwrap();
        for sample in samples {
            let mono = ((sample[0] + sample[1]) / 2.0) as f64;
            let pos = buf.pos;
            buf.data[pos] = mono;
            buf.pos = (pos + 1) % buf.size;
        }
    }

    /// Return the last n samples in chronological order.
    pub fn samples(&self, n: usize) -> Vec<f64> {
        let buf = self.buf.lock().unwrap();
        let n = n.min(buf.size);
        let mut out = Vec::with_capacity(n);
        let start = (buf.pos + buf.size - n) % buf.size;
        for i in 0..n {
            out.push(buf.data[(start + i) % buf.size]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tap_write_and_read() {
        let tap = Tap::new(8);
        tap.write(&[[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]);

        let samples = tap.samples(3);
        assert_eq!(samples.len(), 3);
        assert!((samples[0] - 1.0).abs() < 1e-6);
        assert!((samples[1] - 2.0).abs() < 1e-6);
        assert!((samples[2] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_tap_ring_buffer_wrap() {
        let tap = Tap::new(4);
        // Write 6 samples into a buffer of 4
        tap.write(&[
            [1.0, 1.0],
            [2.0, 2.0],
            [3.0, 3.0],
            [4.0, 4.0],
            [5.0, 5.0],
            [6.0, 6.0],
        ]);

        let samples = tap.samples(4);
        // Should have the last 4 samples
        assert!((samples[0] - 3.0).abs() < 1e-6);
        assert!((samples[1] - 4.0).abs() < 1e-6);
        assert!((samples[2] - 5.0).abs() < 1e-6);
        assert!((samples[3] - 6.0).abs() < 1e-6);
    }

    #[test]
    fn test_tap_mono_mix() {
        let tap = Tap::new(4);
        tap.write(&[[1.0, 0.0], [0.0, 1.0]]);

        let samples = tap.samples(2);
        assert!((samples[0] - 0.5).abs() < 1e-6); // (1+0)/2
        assert!((samples[1] - 0.5).abs() < 1e-6); // (0+1)/2
    }

    #[test]
    fn test_tap_request_more_than_size() {
        let tap = Tap::new(4);
        tap.write(&[[1.0, 1.0], [2.0, 2.0]]);

        let samples = tap.samples(10); // request more than buffer size
        assert_eq!(samples.len(), 4); // capped at buffer size
    }

    #[test]
    fn test_tap_empty_read() {
        let tap = Tap::new(4);
        let samples = tap.samples(4);
        assert_eq!(samples.len(), 4);
        assert!(samples.iter().all(|&v| v == 0.0));
    }
}
