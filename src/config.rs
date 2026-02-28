use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::playlist::RepeatMode;

/// User preferences persisted to ~/.config/cliamp/config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Volume in dB, range [-30, +6].
    #[serde(default)]
    pub volume: f64,

    /// Repeat mode: "off", "all", or "one".
    #[serde(default = "default_repeat")]
    pub repeat: String,

    /// Start with shuffle enabled.
    #[serde(default)]
    pub shuffle: bool,

    /// Start with mono output (L+R downmix).
    #[serde(default)]
    pub mono: bool,

    /// Color theme name. Empty for default ANSI colors.
    #[serde(default)]
    pub theme: String,

    /// EQ preset name (e.g. "Rock", "Jazz"). Empty or "Custom" for manual EQ.
    #[serde(default)]
    pub eq_preset: String,

    /// 10-band EQ gains in dB, range [-12, +12].
    /// Bands: 70Hz, 180Hz, 320Hz, 600Hz, 1kHz, 3kHz, 6kHz, 12kHz, 14kHz, 16kHz.
    #[serde(default = "default_eq")]
    pub eq: Vec<f64>,
}

fn default_repeat() -> String {
    "off".to_string()
}

fn default_eq() -> Vec<f64> {
    vec![0.0; 10]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: 0.0,
            repeat: "off".to_string(),
            shuffle: false,
            mono: false,
            theme: String::new(),
            eq_preset: String::new(),
            eq: vec![0.0; 10],
        }
    }
}

impl Config {
    /// Path to the config file: ~/.config/cliamp/config.toml
    fn path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config").join("cliamp").join("config.toml"))
    }

    /// Load config from disk. Returns defaults if file doesn't exist.
    pub fn load() -> anyhow::Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut cfg: Config = toml::from_str(&content)?;
                cfg.clamp();
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Save config to disk, creating directories as needed.
    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Clamp values to valid ranges.
    fn clamp(&mut self) {
        self.volume = self.volume.clamp(-30.0, 6.0);
        self.repeat = match self.repeat.to_lowercase().as_str() {
            "all" => "all".to_string(),
            "one" => "one".to_string(),
            _ => "off".to_string(),
        };
        // Ensure exactly 10 EQ bands, clamped
        self.eq.resize(10, 0.0);
        for band in &mut self.eq {
            *band = band.clamp(-12.0, 12.0);
        }
    }

    /// Parse repeat string to RepeatMode enum.
    pub fn repeat_mode(&self) -> RepeatMode {
        match self.repeat.as_str() {
            "all" => RepeatMode::All,
            "one" => RepeatMode::One,
            _ => RepeatMode::Off,
        }
    }

    /// Load from a TOML string (for testing).
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut cfg: Config = toml::from_str(s)?;
        cfg.clamp();
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.volume, 0.0);
        assert_eq!(cfg.repeat, "off");
        assert!(!cfg.shuffle);
        assert!(!cfg.mono);
        assert_eq!(cfg.eq.len(), 10);
        assert!(cfg.eq.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
volume = -5.0
repeat = "all"
shuffle = true
mono = true
theme = "catppuccin"
eq_preset = "Rock"
eq = [3.0, 2.0, -1.0, 0.0, 1.0, 4.0, 5.0, 3.0, 2.0, 1.0]
"#;
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.volume, -5.0);
        assert_eq!(cfg.repeat, "all");
        assert!(cfg.shuffle);
        assert!(cfg.mono);
        assert_eq!(cfg.theme, "catppuccin");
        assert_eq!(cfg.eq_preset, "Rock");
        assert_eq!(cfg.eq[0], 3.0);
        assert_eq!(cfg.eq[5], 4.0);
    }

    #[test]
    fn test_parse_empty_config() {
        let cfg = Config::from_str("").unwrap();
        assert_eq!(cfg.volume, 0.0);
        assert_eq!(cfg.repeat, "off");
        assert!(!cfg.shuffle);
    }

    #[test]
    fn test_clamp_volume() {
        let toml = "volume = 100.0";
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.volume, 6.0);

        let toml = "volume = -100.0";
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.volume, -30.0);
    }

    #[test]
    fn test_clamp_eq_bands() {
        let toml = "eq = [20.0, -20.0, 0.0]";
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.eq.len(), 10);
        assert_eq!(cfg.eq[0], 12.0); // clamped
        assert_eq!(cfg.eq[1], -12.0); // clamped
        assert_eq!(cfg.eq[2], 0.0);
        assert_eq!(cfg.eq[3], 0.0); // padded
    }

    #[test]
    fn test_invalid_repeat_defaults_to_off() {
        let toml = r#"repeat = "banana""#;
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.repeat, "off");
    }

    #[test]
    fn test_repeat_mode_conversion() {
        let mut cfg = Config::default();
        assert_eq!(cfg.repeat_mode(), RepeatMode::Off);

        cfg.repeat = "all".to_string();
        assert_eq!(cfg.repeat_mode(), RepeatMode::All);

        cfg.repeat = "one".to_string();
        assert_eq!(cfg.repeat_mode(), RepeatMode::One);
    }

    #[test]
    fn test_roundtrip_serialize() {
        let cfg = Config {
            volume: -3.0,
            repeat: "all".to_string(),
            shuffle: true,
            mono: false,
            theme: "dracula".to_string(),
            eq_preset: "Jazz".to_string(),
            eq: vec![1.0, 2.0, 3.0, 4.0, 5.0, -1.0, -2.0, -3.0, -4.0, -5.0],
        };
        let serialized = toml::to_string_pretty(&cfg).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.volume, cfg.volume);
        assert_eq!(deserialized.repeat, cfg.repeat);
        assert_eq!(deserialized.shuffle, cfg.shuffle);
        assert_eq!(deserialized.eq, cfg.eq);
    }
}
