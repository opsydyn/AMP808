/// Named 10-band EQ preset.
pub struct EqPreset {
    pub name: &'static str,
    pub bands: [f64; 10],
}

/// Built-in EQ presets.
/// Bands: 70Hz, 180Hz, 320Hz, 600Hz, 1kHz, 3kHz, 6kHz, 12kHz, 14kHz, 16kHz
pub const EQ_PRESETS: &[EqPreset] = &[
    EqPreset {
        name: "Flat",
        bands: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    EqPreset {
        name: "Rock",
        bands: [5.0, 4.0, 2.0, -1.0, -2.0, 2.0, 4.0, 5.0, 5.0, 5.0],
    },
    EqPreset {
        name: "Pop",
        bands: [-1.0, 2.0, 4.0, 5.0, 4.0, 1.0, -1.0, -1.0, 1.0, 2.0],
    },
    EqPreset {
        name: "Jazz",
        bands: [3.0, 4.0, 2.0, 1.0, -1.0, -1.0, 1.0, 2.0, 3.0, 4.0],
    },
    EqPreset {
        name: "Classical",
        bands: [3.0, 2.0, 1.0, 0.0, -1.0, -1.0, 0.0, 2.0, 3.0, 4.0],
    },
    EqPreset {
        name: "Bass Boost",
        bands: [8.0, 6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    EqPreset {
        name: "Treble Boost",
        bands: [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 6.0, 7.0],
    },
    EqPreset {
        name: "Vocal",
        bands: [-2.0, -1.0, 1.0, 4.0, 5.0, 4.0, 2.0, 0.0, -1.0, -2.0],
    },
    EqPreset {
        name: "Electronic",
        bands: [6.0, 4.0, 1.0, -1.0, -2.0, 1.0, 3.0, 4.0, 5.0, 6.0],
    },
    EqPreset {
        name: "Acoustic",
        bands: [3.0, 3.0, 2.0, 0.0, 1.0, 2.0, 3.0, 3.0, 2.0, 1.0],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_count() {
        assert_eq!(EQ_PRESETS.len(), 10);
    }

    #[test]
    fn test_flat_is_zero() {
        let flat = &EQ_PRESETS[0];
        assert_eq!(flat.name, "Flat");
        assert!(flat.bands.iter().all(|&b| b == 0.0));
    }

    #[test]
    fn test_bands_in_range() {
        for preset in EQ_PRESETS {
            for &band in &preset.bands {
                assert!(
                    (-12.0..=12.0).contains(&band),
                    "{}: band {band} out of range",
                    preset.name
                );
            }
        }
    }
}
