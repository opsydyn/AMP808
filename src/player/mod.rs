pub mod decode;
pub mod eq;
pub mod ffmpeg;
pub mod gapless;
pub mod source;
pub mod tap;
pub mod volume;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use self::eq::{Biquad, EQ_FREQS};
use self::gapless::GaplessSource;
use self::tap::Tap;

/// Default CD-quality sample rate.
pub const DEFAULT_SAMPLE_RATE: u32 = 44100;

/// Audio engine managing the playback pipeline:
/// [Gapless] -> [10x Biquad EQ] -> [Volume] -> [Tap] -> cpal output
pub struct Player {
    sample_rate: u32,
    gapless: Arc<GaplessSource>,
    tap: Arc<Tap>,
    // Shared state between audio callback and control thread
    state: Arc<Mutex<PlayerState>>,
    paused: Arc<AtomicBool>,
    _stream: Option<Stream>,
}

struct PlayerState {
    volume: f64,         // dB [-30, +6]
    eq_bands: [f64; 10], // dB [-12, +12]
    mono: bool,
    playing: bool,
}

impl Player {
    /// Create a new Player and start the cpal output stream.
    pub fn new() -> anyhow::Result<Self> {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        let gapless = Arc::new(GaplessSource::new());
        let tap = Arc::new(Tap::new(4096));
        let paused = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(PlayerState {
            volume: 0.0,
            eq_bands: [0.0; 10],
            mono: false,
            playing: false,
        }));

        // Build cpal output stream
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no audio output device available"))?;

        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let gapless_cb = Arc::clone(&gapless);
        let tap_cb = Arc::clone(&tap);
        let state_cb = Arc::clone(&state);
        let paused_cb = Arc::clone(&paused);

        // 10-band EQ filters, one per band
        let mut eq_filters: Vec<Biquad> = EQ_FREQS
            .iter()
            .map(|&freq| Biquad::new(freq, 1.4, sample_rate as f64))
            .collect();

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if paused_cb.load(Ordering::Relaxed) {
                    // Fill silence when paused
                    for s in data.iter_mut() {
                        *s = 0.0;
                    }
                    return;
                }

                // Convert flat f32 buffer to stereo frames
                let frame_count = data.len() / 2;
                let mut frames = vec![[0.0f32; 2]; frame_count];

                // Read from gapless source
                gapless_cb.read(&mut frames);

                // Read EQ gains and volume under lock (fast — just copies)
                let (eq_gains, vol, mono) = {
                    let st = state_cb.lock().unwrap();
                    (st.eq_bands, st.volume, st.mono)
                };

                // Apply 10-band EQ
                for (i, filter) in eq_filters.iter_mut().enumerate() {
                    filter.process(&mut frames, eq_gains[i]);
                }

                // Apply volume + mono
                volume::apply_volume(&mut frames, vol, mono);

                // Capture for visualizer
                tap_cb.write(&frames);

                // Write back to interleaved cpal buffer
                for (i, frame) in frames.iter().enumerate() {
                    data[i * 2] = frame[0];
                    data[i * 2 + 1] = frame[1];
                }
            },
            |err| {
                eprintln!("audio stream error: {err}");
            },
            None,
        )?;

        stream.play()?;

        Ok(Self {
            sample_rate,
            gapless,
            tap,
            state,
            paused,
            _stream: Some(stream),
        })
    }

    /// Start playing an audio file.
    pub fn play(&self, path: &str) -> anyhow::Result<()> {
        let source = decode::decode_file(path, self.sample_rate)?;
        self.gapless.replace(source);
        self.paused.store(false, Ordering::Release);

        let mut state = self.state.lock().unwrap();
        state.playing = true;

        Ok(())
    }

    /// Preload the next track for gapless transition.
    pub fn preload(&self, path: &str) -> anyhow::Result<()> {
        let source = decode::decode_file(path, self.sample_rate)?;
        self.gapless.set_next(source);
        Ok(())
    }

    /// Clear the preloaded next track.
    pub fn clear_preload(&self) {
        self.gapless.clear_next();
    }

    /// Toggle pause state.
    pub fn toggle_pause(&self) {
        let was_paused = self.paused.load(Ordering::Acquire);
        self.paused.store(!was_paused, Ordering::Release);
    }

    /// Stop playback and clear all sources.
    pub fn stop(&self) {
        self.gapless.clear();
        self.paused.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap();
        state.playing = false;
    }

    /// Check if a gapless transition occurred (consumes the flag).
    pub fn gapless_advanced(&self) -> bool {
        self.gapless.advanced()
    }

    /// Check if the current track ended with no next queued.
    pub fn drained(&self) -> bool {
        self.gapless.drained()
    }

    pub fn is_playing(&self) -> bool {
        self.state.lock().unwrap().playing
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    // --- Volume ---

    pub fn set_volume(&self, db: f64) {
        let mut state = self.state.lock().unwrap();
        state.volume = db.clamp(-30.0, 6.0);
    }

    pub fn volume(&self) -> f64 {
        self.state.lock().unwrap().volume
    }

    // --- EQ ---

    pub fn set_eq_band(&self, band: usize, db: f64) {
        if band < 10 {
            let mut state = self.state.lock().unwrap();
            state.eq_bands[band] = db.clamp(-12.0, 12.0);
        }
    }

    pub fn eq_bands(&self) -> [f64; 10] {
        self.state.lock().unwrap().eq_bands
    }

    // --- Mono ---

    pub fn toggle_mono(&self) {
        let mut state = self.state.lock().unwrap();
        state.mono = !state.mono;
    }

    pub fn mono(&self) -> bool {
        self.state.lock().unwrap().mono
    }

    // --- Visualizer ---

    pub fn samples(&self) -> Vec<f64> {
        self.tap.samples(2048)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    // --- Position / Seek ---

    /// Get (position_seconds, duration_seconds). Duration is 0 for streams.
    pub fn track_position(&self) -> (u64, u64) {
        let pos_frames = self.gapless.position();
        let dur_frames = self.gapless.duration_frames().unwrap_or(0);
        let sr = self.sample_rate as u64;
        if sr == 0 {
            return (0, 0);
        }
        (pos_frames as u64 / sr, dur_frames as u64 / sr)
    }

    /// Seek to an absolute position in seconds.
    pub fn seek_to(&self, seconds: f64) -> anyhow::Result<()> {
        let frame = (seconds * self.sample_rate as f64) as usize;
        self.gapless.seek(frame)
    }

    /// Seek relative to current position (positive = forward, negative = backward).
    pub fn seek_relative(&self, seconds: f64) -> anyhow::Result<()> {
        let pos = self.gapless.position();
        let dur = self.gapless.duration_frames().unwrap_or(usize::MAX);
        let delta = (seconds * self.sample_rate as f64) as i64;
        let new_pos = (pos as i64 + delta).clamp(0, dur as i64) as usize;
        self.gapless.seek(new_pos)
    }
}
