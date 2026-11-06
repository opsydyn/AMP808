#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkinColor(u32);

impl SkinColor {
    pub const fn new(hex: u32) -> Self {
        Self(hex)
    }

    pub const fn hex(self) -> u32 {
        self.0
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
    meter_low: SkinColor,
    meter_high: SkinColor,
    panel_opacity: SkinOpacity,
    raised_panel_opacity: SkinOpacity,
}

impl Amp808Skin {
    pub const fn built_in() -> Self {
        Self {
            canvas: SkinColor::new(0x11_12_14),
            panel: SkinColor::new(0x1c_1e_21),
            panel_raised: SkinColor::new(0x29_2c_31),
            text: SkinColor::new(0xee_ea_d8),
            text_muted: SkinColor::new(0x8d_8f_93),
            accent: SkinColor::new(0xf8_a1_25),
            meter_low: SkinColor::new(0xf1_f8_27),
            meter_high: SkinColor::new(0xe7_2e_2e),
            panel_opacity: SkinOpacity::new(204),
            raised_panel_opacity: SkinOpacity::new(230),
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
}
