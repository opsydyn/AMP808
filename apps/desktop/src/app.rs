pub mod transport;

pub const PRODUCT_NAME: &str = "AMP808-R3TR0";

#[derive(Debug)]
pub struct DesktopApp {
    product_name: &'static str,
}

impl DesktopApp {
    pub fn new() -> Self {
        Self {
            product_name: PRODUCT_NAME,
        }
    }

    pub fn product_name(&self) -> &'static str {
        self.product_name
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowSpec {
    title: &'static str,
    initial_size: (u16, u16),
    minimum_size: (u16, u16),
}

impl WindowSpec {
    pub const fn main_player() -> Self {
        Self {
            title: PRODUCT_NAME,
            initial_size: (1120, 720),
            minimum_size: (880, 600),
        }
    }

    pub const fn title(self) -> &'static str {
        self.title
    }

    pub const fn initial_size(self) -> (u16, u16) {
        self.initial_size
    }

    pub const fn minimum_size(self) -> (u16, u16) {
        self.minimum_size
    }
}

#[cfg(test)]
mod tests {
    use super::transport::{ResumeAfterSeek, Transport, TransportEvent, TransportState};
    use crate::skin::Amp808Skin;

    use super::{DesktopApp, WindowSpec};

    #[test]
    fn new_desktop_app_exposes_the_product_identity() {
        let app = DesktopApp::new();

        assert_eq!(app.product_name(), "AMP808-R3TR0");
    }

    #[test]
    fn main_window_spec_preserves_the_branded_compact_player_shape() {
        let spec = WindowSpec::main_player();

        assert_eq!(spec.title(), "AMP808-R3TR0");
        assert_eq!(spec.initial_size(), (1120, 720));
        assert_eq!(spec.minimum_size(), (880, 600));
    }

    #[test]
    fn built_in_skin_keeps_the_amp808_hardware_palette_semantic() {
        let skin = Amp808Skin::built_in();

        assert_eq!(skin.canvas().hex(), 0x11_12_14);
        assert_eq!(skin.panel().hex(), 0x1c_1e_21);
        assert_eq!(skin.accent().hex(), 0xf8_a1_25);
        assert_eq!(skin.meter_low().hex(), 0xf1_f8_27);
        assert_eq!(skin.meter_high().hex(), 0xe7_2e_2e);
    }

    #[test]
    fn built_in_skin_uses_readable_translucent_hardware_surfaces() {
        let skin = Amp808Skin::built_in();

        assert!(skin.panel_opacity().unit() >= 0.72);
        assert!(skin.panel_opacity().unit() < 1.0);
        assert!(skin.raised_panel_opacity().unit() > skin.panel_opacity().unit());
        assert!(skin.raised_panel_opacity().unit() < 1.0);
    }

    #[test]
    fn built_in_skin_defines_translucent_tactile_button_states() {
        let skin = Amp808Skin::built_in();

        assert_eq!(skin.control().hex(), 0x24_25_2b);
        assert_eq!(skin.control_hover().hex(), 0x31_33_3b);
        assert_eq!(skin.control_active().hex(), 0x17_18_1c);
        assert_eq!(skin.control_border().hex(), 0x4a_4c_55);
        assert_ne!(skin.control().hex(), skin.canvas().hex());
        assert_ne!(skin.control().hex(), skin.control_hover().hex());
        assert_ne!(skin.control_hover().hex(), skin.control_active().hex());
        assert_eq!(skin.control_primary().hex(), skin.accent().hex());
        assert_eq!(skin.control_primary_foreground().hex(), skin.canvas().hex());
        assert_ne!(
            skin.control_primary().hex(),
            skin.control_primary_hover().hex()
        );
        assert_ne!(
            skin.control_primary_hover().hex(),
            skin.control_primary_active().hex()
        );

        for opacity in [
            skin.control_opacity(),
            skin.control_hover_opacity(),
            skin.control_active_opacity(),
            skin.control_primary_opacity(),
            skin.control_primary_hover_opacity(),
            skin.control_primary_active_opacity(),
        ] {
            assert!(opacity.unit() > 0.0);
            assert!(opacity.unit() < 1.0);
        }
    }

    #[test]
    fn built_in_skin_meets_enhanced_text_and_visible_field_contrast() {
        let skin = Amp808Skin::built_in();

        for surface in [skin.canvas(), skin.panel(), skin.panel_raised()] {
            assert!(skin.text_muted().contrast_ratio(surface) >= 7.0);
        }
        assert!(skin.text().contrast_ratio(skin.field()) >= 7.0);
        assert!(skin.text_muted().contrast_ratio(skin.field()) >= 7.0);
        assert!(skin.field_border().contrast_ratio(skin.panel_raised()) >= 3.0);
        assert_ne!(skin.field().hex(), 0xff_ff_ff);
    }

    #[test]
    fn transport_covers_load_play_pause_seek_resume_and_end() {
        let mut transport = Transport::new();
        assert_eq!(transport.state(), &TransportState::Empty);

        transport.apply(TransportEvent::LoadRequested).unwrap();
        assert_eq!(transport.state(), &TransportState::Loading);

        transport.apply(TransportEvent::Loaded).unwrap();
        assert_eq!(transport.state(), &TransportState::Ready);

        transport.apply(TransportEvent::PlayRequested).unwrap();
        assert_eq!(transport.state(), &TransportState::Playing);

        transport.apply(TransportEvent::PauseRequested).unwrap();
        assert_eq!(transport.state(), &TransportState::Paused);

        transport.apply(TransportEvent::SeekRequested).unwrap();
        assert_eq!(
            transport.state(),
            &TransportState::Buffering {
                resume_after_seek: ResumeAfterSeek::Paused,
            }
        );

        transport.apply(TransportEvent::SeekReady).unwrap();
        assert_eq!(transport.state(), &TransportState::Paused);

        transport.apply(TransportEvent::PlayRequested).unwrap();
        transport.apply(TransportEvent::EndReached).unwrap();
        assert_eq!(transport.state(), &TransportState::Ended);
    }

    #[test]
    fn transport_preserves_playing_intent_across_seek_buffering() {
        let mut transport = Transport::new();
        transport.apply(TransportEvent::LoadRequested).unwrap();
        transport.apply(TransportEvent::Loaded).unwrap();
        transport.apply(TransportEvent::PlayRequested).unwrap();
        transport.apply(TransportEvent::SeekRequested).unwrap();

        assert_eq!(
            transport.state(),
            &TransportState::Buffering {
                resume_after_seek: ResumeAfterSeek::Playing,
            }
        );

        transport.apply(TransportEvent::SeekReady).unwrap();
        assert_eq!(transport.state(), &TransportState::Playing);
    }

    #[test]
    fn transport_rejects_impossible_transitions_without_mutating_state() {
        let mut transport = Transport::new();

        let error = transport.apply(TransportEvent::PlayRequested).unwrap_err();

        assert_eq!(error.from(), &TransportState::Empty);
        assert_eq!(error.event(), TransportEvent::PlayRequested);
        assert_eq!(transport.state(), &TransportState::Empty);
    }

    #[test]
    fn transport_failure_is_explicit_and_a_new_load_can_recover() {
        let mut transport = Transport::new();
        transport.apply(TransportEvent::LoadRequested).unwrap();
        transport
            .apply(TransportEvent::Failed("unsupported audio format".into()))
            .unwrap();
        assert_eq!(
            transport.state(),
            &TransportState::Failed("unsupported audio format".into())
        );

        transport.apply(TransportEvent::LoadRequested).unwrap();
        assert_eq!(transport.state(), &TransportState::Loading);
    }
}
