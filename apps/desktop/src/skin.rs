#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkinColor(u32);

impl SkinColor {
    pub const fn new(hex: u32) -> Self {
        Self(hex)
    }

    pub const fn hex(self) -> u32 {
        self.0
    }

    #[cfg(test)]
    pub fn contrast_ratio(self, other: Self) -> f32 {
        let lighter = self.relative_luminance().max(other.relative_luminance());
        let darker = self.relative_luminance().min(other.relative_luminance());
        (lighter + 0.05) / (darker + 0.05)
    }

    #[cfg(test)]
    fn relative_luminance(self) -> f32 {
        let channel = |shift: u32| {
            let value = ((self.0 >> shift) & 0xff_u32) as f32 / 255.0;
            if value <= 0.040_45 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkinOpacity(u8);

impl SkinOpacity {
    pub const fn new(alpha: u8) -> Self {
        Self(alpha)
    }

    pub const fn unit(self) -> f32 {
        self.0 as f32 / u8::MAX as f32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Amp808Skin {
    canvas: SkinColor,
    panel: SkinColor,
    panel_raised: SkinColor,
    text: SkinColor,
    text_muted: SkinColor,
    accent: SkinColor,
    control: SkinColor,
    control_hover: SkinColor,
    control_active: SkinColor,
    control_border: SkinColor,
    control_primary: SkinColor,
    control_primary_hover: SkinColor,
    control_primary_active: SkinColor,
    control_primary_foreground: SkinColor,
    field: SkinColor,
    field_border: SkinColor,
    meter_low: SkinColor,
    meter_high: SkinColor,
    panel_opacity: SkinOpacity,
    raised_panel_opacity: SkinOpacity,
    control_opacity: SkinOpacity,
    control_hover_opacity: SkinOpacity,
    control_active_opacity: SkinOpacity,
    control_primary_opacity: SkinOpacity,
    control_primary_hover_opacity: SkinOpacity,
    control_primary_active_opacity: SkinOpacity,
}

impl Amp808Skin {
    pub const fn built_in() -> Self {
        Self {
            canvas: SkinColor::new(0x11_12_14),
            panel: SkinColor::new(0x1c_1e_21),
            panel_raised: SkinColor::new(0x29_2c_31),
            text: SkinColor::new(0xee_ea_d8),
            text_muted: SkinColor::new(0xc4_c0_b5),
            accent: SkinColor::new(0xf8_a1_25),
            control: SkinColor::new(0x24_25_2b),
            control_hover: SkinColor::new(0x31_33_3b),
            control_active: SkinColor::new(0x17_18_1c),
            control_border: SkinColor::new(0x4a_4c_55),
            control_primary: SkinColor::new(0xf8_a1_25),
            control_primary_hover: SkinColor::new(0xff_b5_44),
            control_primary_active: SkinColor::new(0xef_a1_37),
            control_primary_foreground: SkinColor::new(0x11_12_14),
            field: SkinColor::new(0x17_18_1c),
            field_border: SkinColor::new(0x74_77_82),
            meter_low: SkinColor::new(0xf1_f8_27),
            meter_high: SkinColor::new(0xe7_2e_2e),
            panel_opacity: SkinOpacity::new(204),
            raised_panel_opacity: SkinOpacity::new(230),
            control_opacity: SkinOpacity::new(224),
            control_hover_opacity: SkinOpacity::new(235),
            control_active_opacity: SkinOpacity::new(245),
            control_primary_opacity: SkinOpacity::new(235),
            control_primary_hover_opacity: SkinOpacity::new(230),
            control_primary_active_opacity: SkinOpacity::new(242),
        }
    }

    pub const fn canvas(self) -> SkinColor {
        self.canvas
    }

    pub const fn panel(self) -> SkinColor {
        self.panel
    }

    pub const fn panel_raised(self) -> SkinColor {
        self.panel_raised
    }

    pub const fn text(self) -> SkinColor {
        self.text
    }

    pub const fn text_muted(self) -> SkinColor {
        self.text_muted
    }

    pub const fn accent(self) -> SkinColor {
        self.accent
    }

    pub const fn control(self) -> SkinColor {
        self.control
    }

    pub const fn control_hover(self) -> SkinColor {
        self.control_hover
    }

    pub const fn control_active(self) -> SkinColor {
        self.control_active
    }

    pub const fn control_border(self) -> SkinColor {
        self.control_border
    }

    pub const fn control_primary(self) -> SkinColor {
        self.control_primary
    }

    pub const fn control_primary_hover(self) -> SkinColor {
        self.control_primary_hover
    }

    pub const fn control_primary_active(self) -> SkinColor {
        self.control_primary_active
    }

    pub const fn control_primary_foreground(self) -> SkinColor {
        self.control_primary_foreground
    }

    pub const fn field(self) -> SkinColor {
        self.field
    }

    pub const fn field_border(self) -> SkinColor {
        self.field_border
    }

    pub const fn meter_low(self) -> SkinColor {
        self.meter_low
    }

    pub const fn meter_high(self) -> SkinColor {
        self.meter_high
    }

    pub const fn panel_opacity(self) -> SkinOpacity {
        self.panel_opacity
    }

    pub const fn raised_panel_opacity(self) -> SkinOpacity {
        self.raised_panel_opacity
    }

    pub const fn control_opacity(self) -> SkinOpacity {
        self.control_opacity
    }

    pub const fn control_hover_opacity(self) -> SkinOpacity {
        self.control_hover_opacity
    }

    pub const fn control_active_opacity(self) -> SkinOpacity {
        self.control_active_opacity
    }

    pub const fn control_primary_opacity(self) -> SkinOpacity {
        self.control_primary_opacity
    }

    pub const fn control_primary_hover_opacity(self) -> SkinOpacity {
        self.control_primary_hover_opacity
    }

    pub const fn control_primary_active_opacity(self) -> SkinOpacity {
        self.control_primary_active_opacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AAA_NORMAL_TEXT_CONTRAST: f32 = 7.0;

    fn composite(surface: SkinColor, opacity: SkinOpacity, backdrop: SkinColor) -> SkinColor {
        let alpha = opacity.unit();
        let blend = |shift: u32| {
            let foreground = ((surface.0 >> shift) & 0xff) as f32;
            let background = ((backdrop.0 >> shift) & 0xff) as f32;
            (foreground.mul_add(alpha, background * (1.0 - alpha))).round() as u32
        };

        SkinColor::new((blend(16) << 16) | (blend(8) << 8) | blend(0))
    }

    #[test]
    fn enabled_button_labels_keep_aaa_contrast_over_extreme_backdrops() {
        let skin = Amp808Skin::built_in();
        let backdrops = [
            ("black", SkinColor::new(0x00_00_00)),
            ("white", SkinColor::new(0xff_ff_ff)),
        ];
        let states = [
            (
                "neutral",
                skin.control(),
                skin.control_opacity(),
                skin.text(),
            ),
            (
                "neutral hover",
                skin.control_hover(),
                skin.control_hover_opacity(),
                skin.text(),
            ),
            (
                "neutral active",
                skin.control_active(),
                skin.control_active_opacity(),
                skin.text(),
            ),
            (
                "primary",
                skin.control_primary(),
                skin.control_primary_opacity(),
                skin.control_primary_foreground(),
            ),
            (
                "primary hover",
                skin.control_primary_hover(),
                skin.control_primary_hover_opacity(),
                skin.control_primary_foreground(),
            ),
            (
                "primary active",
                skin.control_primary_active(),
                skin.control_primary_active_opacity(),
                skin.control_primary_foreground(),
            ),
        ];

        for (state, surface, opacity, foreground) in states {
            for (backdrop_name, backdrop) in backdrops {
                let rendered_surface = composite(surface, opacity, backdrop);
                let contrast = foreground.contrast_ratio(rendered_surface);
                assert!(
                    contrast >= AAA_NORMAL_TEXT_CONTRAST,
                    "{state} on {backdrop_name} has {contrast:.2}:1 contrast"
                );
            }
        }
    }
}
