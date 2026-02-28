/// Apply dB volume gain and optional mono downmix to stereo samples.
pub fn apply_volume(samples: &mut [[f32; 2]], volume_db: f64, mono: bool) {
    let gain = 10.0_f64.powf(volume_db / 20.0) as f32;
    for sample in samples.iter_mut() {
        sample[0] *= gain;
        sample[1] *= gain;
        if mono {
            let mid = (sample[0] + sample[1]) / 2.0;
            sample[0] = mid;
            sample[1] = mid;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_db_unity_gain() {
        let mut samples = vec![[0.5f32, -0.5], [0.3, -0.3]];
        apply_volume(&mut samples, 0.0, false);
        assert!((samples[0][0] - 0.5).abs() < 1e-6);
        assert!((samples[0][1] - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn test_positive_db_boost() {
        let mut samples = vec![[0.5f32, 0.5]];
        apply_volume(&mut samples, 6.0, false);
        // +6dB ≈ 2x gain
        assert!(samples[0][0] > 0.9);
    }

    #[test]
    fn test_negative_db_attenuate() {
        let mut samples = vec![[1.0f32, 1.0]];
        apply_volume(&mut samples, -20.0, false);
        // -20dB ≈ 0.1x gain
        assert!(samples[0][0] < 0.15);
        assert!(samples[0][0] > 0.05);
    }

    #[test]
    fn test_mono_downmix() {
        let mut samples = vec![[1.0f32, 0.0], [0.0, 1.0]];
        apply_volume(&mut samples, 0.0, true);
        assert!((samples[0][0] - 0.5).abs() < 1e-6);
        assert!((samples[0][1] - 0.5).abs() < 1e-6);
        assert!((samples[1][0] - 0.5).abs() < 1e-6);
        assert!((samples[1][1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_mono_with_volume() {
        let mut samples = vec![[1.0f32, 0.0]];
        apply_volume(&mut samples, 0.0, true);
        // Mono downmix of [1.0, 0.0] = 0.5 for both channels
        assert!((samples[0][0] - 0.5).abs() < 1e-6);
    }
}
