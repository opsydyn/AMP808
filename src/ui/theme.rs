use std::fs;

use ratatui::style::Color;
use serde::Deserialize;

/// A named color theme with hex color values.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub accent: Color,
    pub bright_fg: Color,
    pub fg: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
}

/// Default theme name (uses ANSI terminal colors).
pub const DEFAULT_NAME: &str = "Default - Terminal colors";

/// Raw TOML theme for deserialization.
#[derive(Deserialize)]
struct ThemeToml {
    accent: String,
    bright_fg: String,
    fg: String,
    green: String,
    yellow: String,
    red: String,
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

impl Theme {
    /// Returns true if this is the default ANSI theme.
    pub fn is_default(&self) -> bool {
        self.name == DEFAULT_NAME
    }

    fn from_toml(name: &str, toml: &ThemeToml) -> Option<Self> {
        Some(Self {
            name: name.to_string(),
            accent: parse_hex_color(&toml.accent)?,
            bright_fg: parse_hex_color(&toml.bright_fg)?,
            fg: parse_hex_color(&toml.fg)?,
            green: parse_hex_color(&toml.green)?,
            yellow: parse_hex_color(&toml.yellow)?,
            red: parse_hex_color(&toml.red)?,
        })
    }
}

/// Built-in theme definitions (embedded at compile time).
struct BuiltinTheme {
    name: &'static str,
    toml: &'static str,
}

const BUILTIN_THEMES: &[BuiltinTheme] = &[
    BuiltinTheme {
        name: "astro",
        toml: r##"accent = "#c3a6ff"
bright_fg = "#ffd580"
fg = "#a2aabc"
green = "#bae67e"
yellow = "#5ccfe6"
red = "#ffae57""##,
    },
    BuiltinTheme {
        name: "ayu-mirage-dark",
        toml: r##"accent = "#73d0ff"
bright_fg = "#f3f4f5"
fg = "#cccac2"
green = "#d5ff80"
yellow = "#ffad66"
red = "#f28779""##,
    },
    BuiltinTheme {
        name: "catppuccin",
        toml: r##"accent = "#89b4fa"
bright_fg = "#cdd6f4"
fg = "#9399b2"
green = "#a6e3a1"
yellow = "#f9e2af"
red = "#f38ba8""##,
    },
    BuiltinTheme {
        name: "catppuccin-latte",
        toml: r##"accent = "#1e66f5"
bright_fg = "#4c4f69"
fg = "#8c8fa1"
green = "#40a02b"
yellow = "#df8e1d"
red = "#d20f39""##,
    },
    BuiltinTheme {
        name: "ethereal",
        toml: r##"accent = "#7d82d9"
bright_fg = "#ffcead"
fg = "#9a96a8"
green = "#92a593"
yellow = "#E9BB4F"
red = "#ED5B5A""##,
    },
    BuiltinTheme {
        name: "everforest",
        toml: r##"accent = "#7fbbb3"
bright_fg = "#d3c6aa"
fg = "#7a8478"
green = "#a7c080"
yellow = "#dbbc7f"
red = "#e67e80""##,
    },
    BuiltinTheme {
        name: "flexoki-light",
        toml: r##"accent = "#205EA6"
bright_fg = "#100F0F"
fg = "#6F6E69"
green = "#879A39"
yellow = "#D0A215"
red = "#D14D41""##,
    },
    BuiltinTheme {
        name: "gruvbox",
        toml: r##"accent = "#7daea3"
bright_fg = "#d4be98"
fg = "#a89984"
green = "#a9b665"
yellow = "#d8a657"
red = "#ea6962""##,
    },
    BuiltinTheme {
        name: "hackerman",
        toml: r##"accent = "#82FB9C"
bright_fg = "#ddf7ff"
fg = "#8e95b8"
green = "#4fe88f"
yellow = "#50f7d4"
red = "#50f872""##,
    },
    BuiltinTheme {
        name: "kanagawa",
        toml: r##"accent = "#7e9cd8"
bright_fg = "#dcd7ba"
fg = "#938aa9"
green = "#76946a"
yellow = "#c0a36e"
red = "#c34043""##,
    },
    BuiltinTheme {
        name: "matte-black",
        toml: r##"accent = "#e68e0d"
bright_fg = "#bebebe"
fg = "#777777"
green = "#FFC107"
yellow = "#b91c1c"
red = "#D35F5F""##,
    },
    BuiltinTheme {
        name: "miasma",
        toml: r##"accent = "#78824b"
bright_fg = "#c2c2b0"
fg = "#666666"
green = "#5f875f"
yellow = "#b36d43"
red = "#685742""##,
    },
    BuiltinTheme {
        name: "nord",
        toml: r##"accent = "#81a1c1"
bright_fg = "#d8dee9"
fg = "#8690a0"
green = "#a3be8c"
yellow = "#ebcb8b"
red = "#bf616a""##,
    },
    BuiltinTheme {
        name: "osaka-jade",
        toml: r##"accent = "#509475"
bright_fg = "#F7E8B2"
fg = "#C1C497"
green = "#549e6a"
yellow = "#459451"
red = "#FF5345""##,
    },
    BuiltinTheme {
        name: "ristretto",
        toml: r##"accent = "#f38d70"
bright_fg = "#e6d9db"
fg = "#948a8b"
green = "#adda78"
yellow = "#f9cc6c"
red = "#fd6883""##,
    },
    BuiltinTheme {
        name: "rose-pine",
        toml: r##"accent = "#56949f"
bright_fg = "#575279"
fg = "#908caa"
green = "#286983"
yellow = "#ea9d34"
red = "#b4637a""##,
    },
    BuiltinTheme {
        name: "tokyo-night",
        toml: r##"accent = "#7aa2f7"
bright_fg = "#cfc9c2"
fg = "#737aa2"
green = "#9ece6a"
yellow = "#e0af68"
red = "#f7768e""##,
    },
    BuiltinTheme {
        name: "vantablack",
        toml: r##"accent = "#8d8d8d"
bright_fg = "#ffffff"
fg = "#8d8d8d"
green = "#b6b6b6"
yellow = "#cecece"
red = "#a4a4a4""##,
    },
];

/// Load all available themes: built-in + user custom themes from
/// `~/.config/cliamp/themes/*.toml`. Sorted by name.
pub fn load_all() -> Vec<Theme> {
    let mut themes = Vec::with_capacity(BUILTIN_THEMES.len() + 4);

    // Load built-in themes
    for builtin in BUILTIN_THEMES {
        if let Ok(toml) = toml::from_str::<ThemeToml>(builtin.toml)
            && let Some(theme) = Theme::from_toml(builtin.name, &toml)
        {
            themes.push(theme);
        }
    }

    // Load user custom themes from ~/.config/cliamp/themes/
    if let Some(home) = dirs::home_dir() {
        let user_dir = home.join(".config").join("cliamp").join("themes");
        if let Ok(entries) = fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                if let Ok(content) = fs::read_to_string(&path)
                    && let Ok(toml) = toml::from_str::<ThemeToml>(&content)
                    && let Some(theme) = Theme::from_toml(&name, &toml)
                {
                    // Override built-in with same name
                    if let Some(existing) = themes
                        .iter()
                        .position(|t| t.name.eq_ignore_ascii_case(&name))
                    {
                        themes[existing] = theme;
                    } else {
                        themes.push(theme);
                    }
                }
            }
        }
    }

    themes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    themes
}

/// Find a theme by name (case-insensitive). Returns None if not found.
pub fn find_by_name(themes: &[Theme], name: &str) -> Option<usize> {
    themes
        .iter()
        .position(|t| t.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(
            parse_hex_color("#89b4fa"),
            Some(Color::Rgb(0x89, 0xb4, 0xfa))
        );
        assert_eq!(
            parse_hex_color("89b4fa"),
            Some(Color::Rgb(0x89, 0xb4, 0xfa))
        );
        assert_eq!(parse_hex_color("#fff"), None); // too short
        assert_eq!(parse_hex_color(""), None);
    }

    #[test]
    fn test_default_name() {
        // No loaded theme should have the default name
        let themes = load_all();
        assert!(themes.iter().all(|t| !t.is_default()));
    }

    #[test]
    fn test_load_all_builtin() {
        let themes = load_all();
        assert_eq!(themes.len(), 18);

        // Check sorted order
        let names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|a| a.to_lowercase());
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_find_by_name() {
        let themes = load_all();
        assert!(find_by_name(&themes, "catppuccin").is_some());
        assert!(find_by_name(&themes, "Catppuccin").is_some());
        assert!(find_by_name(&themes, "nonexistent").is_none());
    }

    #[test]
    fn test_catppuccin_colors() {
        let themes = load_all();
        let idx = find_by_name(&themes, "catppuccin").unwrap();
        let t = &themes[idx];
        assert_eq!(t.accent, Color::Rgb(0x89, 0xb4, 0xfa));
        assert_eq!(t.green, Color::Rgb(0xa6, 0xe3, 0xa1));
    }

    #[test]
    fn test_astro_colors() {
        let themes = load_all();
        let idx = find_by_name(&themes, "astro").unwrap();
        let t = &themes[idx];
        assert_eq!(t.accent, Color::Rgb(0xc3, 0xa6, 0xff));
        assert_eq!(t.bright_fg, Color::Rgb(0xff, 0xd5, 0x80));
        assert_eq!(t.fg, Color::Rgb(0xa2, 0xaa, 0xbc));
        assert_eq!(t.green, Color::Rgb(0xba, 0xe6, 0x7e));
        assert_eq!(t.yellow, Color::Rgb(0x5c, 0xcf, 0xe6));
        assert_eq!(t.red, Color::Rgb(0xff, 0xae, 0x57));
    }
}
