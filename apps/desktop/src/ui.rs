use gpui_kit::base::{Disableable, StyledExt, box_shadow};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    App, Context, ElementId, Entity, FocusHandle, Focusable, Hsla, IntoElement, KeyBinding,
    PathPromptOptions, Render, Role, Subscription, TestSupportExt, Window, canvas as gpui_canvas,
    deferred, div, prelude::*, px, rgb,
};
use std::time::{Duration, Instant};

use crate::app::DesktopApp;
use crate::app::transport::TransportState;
use crate::cosmic::{CosmicBackgroundHost, CosmicMotion, CosmicSignal, CosmicStatus};
use crate::library::{LibraryCatalogue, LibraryTrack, LocalPlaylist, LocalPlaylistId, ScanSummary};
use crate::milkdrop::{MilkdropPreset, MilkdropStatus, MilkdropViewHost};
use crate::playback::{DecodeWorkerState, PlaybackSession, PlaybackSessionError};
use crate::queue::{PlaybackQueue, QueueAdvance, QueuedTrack};
use crate::skin::Amp808Skin;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HardwareButtonTone {
    Neutral,
    Primary,
}

fn skin_hsla(color: crate::skin::SkinColor) -> Hsla {
    rgb(color.hex()).into()
}

pub fn init_skin_theme(cx: &mut App) {
    let skin = Amp808Skin::built_in();
    gpui_kit::component::Theme::update(cx, |theme| {
        theme.background = skin_hsla(skin.field());
        theme.input = skin_hsla(skin.field_border());
        theme.foreground = skin_hsla(skin.text());
        theme.muted_foreground = skin_hsla(skin.text_muted());
        theme.ring = skin_hsla(skin.accent());
        theme.caret = skin_hsla(skin.accent());
        theme.selection = skin_hsla(skin.accent()).opacity(0.35);
    });
}

fn hardware_input(state: &Entity<InputState>, id: impl Into<ElementId>, skin: Amp808Skin) -> Input {
    Input::new(state)
        .id(id)
        .w_full()
        .rounded(px(6.0))
        .border_color(skin_hsla(skin.field_border()))
        .bg(skin_hsla(skin.field()))
        .text_color(skin_hsla(skin.text()))
        .shadow(vec![box_shadow(
            0.0,
            2.0,
            5.0,
            -1.0,
            skin_hsla(crate::skin::SkinColor::new(0x00_00_00)).opacity(0.5),
        )])
}

fn hardware_button(
    id: impl Into<ElementId>,
    tone: HardwareButtonTone,
    skin: Amp808Skin,
    cx: &App,
) -> Button {
    let (surface, hover, active, foreground, border) = match tone {
        HardwareButtonTone::Neutral => (
            skin.control(),
            skin.control_hover(),
            skin.control_active(),
            skin.text(),
            skin.control_border(),
        ),
        HardwareButtonTone::Primary => (
            skin.control_primary(),
            skin.control_primary_hover(),
            skin.control_primary_active(),
            skin.text(),
            skin.accent(),
        ),
    };

    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(skin_hsla(surface))
                .hover(skin_hsla(hover))
                .active(skin_hsla(active))
                .foreground(skin_hsla(foreground))
                .shadow(true),
        )
        .outline()
        .rounded(px(6.0))
        .border_color(skin_hsla(border))
        .text_color(skin_hsla(foreground))
        .shadow(vec![box_shadow(
            0.0,
            2.0,
            5.0,
            -1.0,
            skin_hsla(crate::skin::SkinColor::new(0x00_00_00)).opacity(0.62),
        )])
}

gpui_kit::actions!(
    amp808,
    [
        OpenFile,
        TogglePlayback,
        SeekBack,
        SeekForward,
        VolumeDown,
        VolumeUp,
        ToggleVisualizer,
        ToggleLibrary,
    ]
);

pub fn init_keyboard_shortcuts(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-o", OpenFile, Some("Amp808Player")),
        KeyBinding::new(
            "space",
            TogglePlayback,
            Some("Amp808Player && !Amp808Control"),
        ),
        KeyBinding::new("left", SeekBack, Some("Amp808Player")),
        KeyBinding::new("right", SeekForward, Some("Amp808Player")),
        KeyBinding::new("down", VolumeDown, Some("Amp808Player")),
        KeyBinding::new("up", VolumeUp, Some("Amp808Player")),
        KeyBinding::new("v", ToggleVisualizer, Some("Amp808Player")),
        KeyBinding::new("l", ToggleLibrary, Some("Amp808Player")),
    ]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisualizerMode {
    Spectrum,
    Waveform,
    MilkDrop,
}

const fn should_present_milkdrop(library_visible: bool, visualizer_mode: VisualizerMode) -> bool {
    !library_visible && matches!(visualizer_mode, VisualizerMode::MilkDrop)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackToggle {
    Pause,
    Play,
    Restart,
    Ignore,
}

#[derive(Debug)]
enum LibraryStatus {
    Unloaded,
    Loading,
    Ready(Vec<LibraryTrack>),
    Failed(String),
}

fn playback_toggle(state: &TransportState) -> PlaybackToggle {
    match state {
        TransportState::Playing => PlaybackToggle::Pause,
        TransportState::Paused | TransportState::Ready => PlaybackToggle::Play,
        TransportState::Ended => PlaybackToggle::Restart,
        TransportState::Empty
        | TransportState::Loading
        | TransportState::Buffering { .. }
        | TransportState::Failed(_) => PlaybackToggle::Ignore,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrackDisplay {
    title: String,
    artist: String,
    album: String,
}

impl TrackDisplay {
    fn new(title: &str, artist: Option<&str>, album: Option<&str>) -> Self {
        Self {
            title: title.to_owned(),
            artist: artist.unwrap_or("UNKNOWN ARTIST").to_owned(),
            album: album.unwrap_or("UNKNOWN ALBUM").to_owned(),
        }
    }

    fn empty() -> Self {
        Self {
            title: "NO TRACK LOADED".to_owned(),
            artist: "--".to_owned(),
            album: "--".to_owned(),
        }
    }

    fn accessibility_label(&self) -> String {
        if self.title == "NO TRACK LOADED" {
            format!(
                "No track loaded. Artist: {}. Album: {}.",
                self.artist, self.album
            )
        } else {
            format!(
                "Now playing: {}. Artist: {}. Album: {}.",
                self.title, self.artist, self.album
            )
        }
    }

    #[cfg(test)]
    fn title(&self) -> &str {
        &self.title
    }

    #[cfg(test)]
    fn artist(&self) -> &str {
        &self.artist
    }

    #[cfg(test)]
    fn album(&self) -> &str {
        &self.album
    }
}

pub struct PlayerView {
    app: DesktopApp,
    focus: FocusHandle,
    skin: Amp808Skin,
    session: Option<PlaybackSession>,
    cosmic: CosmicBackgroundHost,
    milkdrop: MilkdropViewHost,
    milkdrop_preset_notice: Option<String>,
    milkdrop_preset_error: Option<String>,
    error: Option<String>,
    seeking: bool,
    visualizer_mode: VisualizerMode,
    milkdrop_performance_probe: PerformanceProbe,
    cosmic_performance_probe: PerformanceProbe,
    library_visible: bool,
    library_catalogue: Option<LibraryCatalogue>,
    library_status: LibraryStatus,
    library_search: Option<Entity<InputState>>,
    library_search_subscription: Option<Subscription>,
    library_query_generation: u64,
    library_scan_summary: Option<ScanSummary>,
    playlists: Vec<LocalPlaylist>,
    selected_playlist: Option<LocalPlaylistId>,
    selected_playlist_tracks: Vec<LibraryTrack>,
    playlist_name: Option<Entity<InputState>>,
    playlist_error: Option<String>,
    queue: PlaybackQueue,
    queue_advance: QueueAdvance,
}

impl PlayerView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            app: DesktopApp::new(),
            focus: cx.focus_handle(),
            skin: Amp808Skin::built_in(),
            session: None,
            cosmic: CosmicBackgroundHost::new(),
            milkdrop: MilkdropViewHost::new(),
            milkdrop_preset_notice: None,
            milkdrop_preset_error: None,
            error: None,
            seeking: false,
            visualizer_mode: VisualizerMode::Spectrum,
            milkdrop_performance_probe: PerformanceProbe::from_env("AMP808_MILKDROP_METRICS"),
            cosmic_performance_probe: PerformanceProbe::from_env("AMP808_COSMIC_METRICS"),
            library_visible: false,
            library_catalogue: None,
            library_status: LibraryStatus::Unloaded,
            library_search: None,
            library_search_subscription: None,
            library_query_generation: 0,
            library_scan_summary: None,
            playlists: Vec::new(),
            selected_playlist: None,
            selected_playlist_tracks: Vec::new(),
            playlist_name: None,
            playlist_error: None,
            queue: PlaybackQueue::new(),
            queue_advance: QueueAdvance::new(),
        }
    }

    fn color(&self, value: crate::skin::SkinColor) -> gpui_kit::Rgba {
        rgb(value.hex())
    }

    fn install_session(&mut self, mut session: PlaybackSession) {
        self.milkdrop.replace_pcm(session.take_milkdrop_pcm());
        self.session = Some(session);
    }

    fn clear_session(&mut self) {
        self.session = None;
        self.milkdrop.replace_pcm(None);
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open in AMP808-R3TR0".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match selection.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |view, cx| {
                        view.error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            let result = PlaybackSession::open(path);
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(session) => {
                        view.queue.clear();
                        view.install_session(session);
                        view.error = None;
                        view.seeking = false;
                    }
                    Err(error) => {
                        view.clear_session();
                        view.error = Some(error.to_string());
                        view.seeking = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn choose_milkdrop_preset(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Load a .milk preset".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match selection.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.milkdrop_preset_notice = None;
                        view.milkdrop_preset_error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |view, cx| {
                        view.milkdrop_preset_notice = None;
                        view.milkdrop_preset_error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            let load = cx
                .background_executor()
                .spawn(async move { MilkdropPreset::read(&path) });
            let result = load.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(preset) => match view.milkdrop.install_preset(&preset) {
                        Ok(()) => {
                            view.milkdrop_preset_notice =
                                Some(format!("Loaded MilkDrop preset: {}", preset.name()));
                            view.milkdrop_preset_error = None;
                        }
                        Err(error) => {
                            view.milkdrop_preset_notice = None;
                            view.milkdrop_preset_error = Some(error.to_string());
                        }
                    },
                    Err(error) => {
                        view.milkdrop_preset_notice = None;
                        view.milkdrop_preset_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn ensure_library_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.library_search.is_none() {
            let search = cx.new(|cx| {
                InputState::new(window, cx).placeholder("Search title, artist, or album")
            });
            let subscription = cx.subscribe(&search, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_library(cx);
                }
            });
            self.library_search = Some(search);
            self.library_search_subscription = Some(subscription);
        }
        if self.playlist_name.is_none() {
            self.playlist_name =
                Some(cx.new(|cx| InputState::new(window, cx).placeholder("Playlist name")));
        }
    }

    fn toggle_library(&mut self, cx: &mut Context<Self>) {
        self.library_visible = !self.library_visible;
        if self.library_visible {
            self.milkdrop.hide();
        }
        if self.library_visible && matches!(self.library_status, LibraryStatus::Unloaded) {
            self.load_library(cx);
        } else {
            cx.notify();
        }
    }

    fn load_library(&mut self, cx: &mut Context<Self>) {
        self.library_query_generation = self.library_query_generation.wrapping_add(1);
        let generation = self.library_query_generation;
        self.library_status = LibraryStatus::Loading;
        let load = cx.background_executor().spawn(async move {
            let catalogue = LibraryCatalogue::open_default()?;
            let tracks = catalogue.search("")?;
            let playlists = catalogue.playlists()?;
            Ok::<_, crate::library::LibraryError>((catalogue, tracks, playlists))
        });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |view, cx| {
                if view.library_query_generation != generation {
                    return;
                }
                match result {
                    Ok((catalogue, tracks, playlists)) => {
                        view.library_catalogue = Some(catalogue);
                        view.library_status = LibraryStatus::Ready(tracks);
                        view.playlists = playlists;
                    }
                    Err(error) => view.library_status = LibraryStatus::Failed(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_library(&mut self, cx: &mut Context<Self>) {
        if !self.library_visible {
            return;
        }
        let Some(catalogue) = self.library_catalogue.clone() else {
            return;
        };
        let query = self
            .library_search
            .as_ref()
            .map_or_else(String::new, |search| search.read(cx).value().to_string());
        self.library_query_generation = self.library_query_generation.wrapping_add(1);
        let generation = self.library_query_generation;
        self.library_status = LibraryStatus::Loading;
        let search = cx
            .background_executor()
            .spawn(async move { catalogue.search(&query) });
        cx.spawn(async move |this, cx| {
            let result = search.await;
            let _ = this.update(cx, |view, cx| {
                if view.library_query_generation != generation {
                    return;
                }
                view.library_status = match result {
                    Ok(tracks) => LibraryStatus::Ready(tracks),
                    Err(error) => LibraryStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn choose_library_folder(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add a music folder to AMP808-R3TR0".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match selection.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.library_status = LibraryStatus::Failed(error.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |view, cx| {
                        view.library_status = LibraryStatus::Failed(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            let query = this
                .read_with(cx, |view, cx| {
                    view.library_search
                        .as_ref()
                        .map_or_else(String::new, |search| search.read(cx).value().to_string())
                })
                .unwrap_or_default();
            let generation = this
                .update(cx, |view, cx| {
                    view.library_query_generation = view.library_query_generation.wrapping_add(1);
                    view.library_status = LibraryStatus::Loading;
                    cx.notify();
                    view.library_query_generation
                })
                .unwrap_or_default();
            let scan = cx.background_spawn(async move {
                let catalogue = LibraryCatalogue::open_default()?;
                let summary = catalogue.scan_root(path)?;
                let tracks = catalogue.search(&query)?;
                let playlists = catalogue.playlists()?;
                Ok::<_, crate::library::LibraryError>((catalogue, summary, tracks, playlists))
            });
            let result = scan.await;
            let _ = this.update(cx, |view, cx| {
                if view.library_query_generation != generation {
                    return;
                }
                match result {
                    Ok((catalogue, summary, tracks, playlists)) => {
                        view.library_catalogue = Some(catalogue);
                        view.library_scan_summary = Some(summary);
                        view.library_status = LibraryStatus::Ready(tracks);
                        view.playlists = playlists;
                    }
                    Err(error) => view.library_status = LibraryStatus::Failed(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn create_playlist(&mut self, cx: &mut Context<Self>) {
        let Some(catalogue) = self.library_catalogue.clone() else {
            self.playlist_error = Some("Load the local library before creating a playlist".into());
            cx.notify();
            return;
        };
        let name = self
            .playlist_name
            .as_ref()
            .map_or_else(String::new, |input| input.read(cx).value().to_string());
        let work = cx.background_executor().spawn(async move {
            let playlist = catalogue.create_playlist(&name)?;
            let playlists = catalogue.playlists()?;
            Ok::<_, crate::library::LibraryError>((playlist, playlists))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((playlist, playlists)) => {
                        view.selected_playlist = Some(playlist.id());
                        view.selected_playlist_tracks.clear();
                        view.playlists = playlists;
                        view.playlist_error = None;
                    }
                    Err(error) => view.playlist_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn rename_selected_playlist(&mut self, cx: &mut Context<Self>) {
        let (Some(catalogue), Some(playlist_id)) =
            (self.library_catalogue.clone(), self.selected_playlist)
        else {
            return;
        };
        let name = self
            .playlist_name
            .as_ref()
            .map_or_else(String::new, |input| input.read(cx).value().to_string());
        let work = cx.background_executor().spawn(async move {
            catalogue.rename_playlist(playlist_id, &name)?;
            catalogue.playlists()
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(playlists) => {
                        view.playlists = playlists;
                        view.playlist_error = None;
                    }
                    Err(error) => view.playlist_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn delete_selected_playlist(&mut self, cx: &mut Context<Self>) {
        let (Some(catalogue), Some(playlist_id)) =
            (self.library_catalogue.clone(), self.selected_playlist)
        else {
            return;
        };
        let work = cx.background_executor().spawn(async move {
            catalogue.delete_playlist(playlist_id)?;
            catalogue.playlists()
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(playlists) => {
                        view.playlists = playlists;
                        view.selected_playlist = None;
                        view.selected_playlist_tracks.clear();
                        view.playlist_error = None;
                    }
                    Err(error) => view.playlist_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_playlist(&mut self, playlist_id: LocalPlaylistId, cx: &mut Context<Self>) {
        let Some(catalogue) = self.library_catalogue.clone() else {
            return;
        };
        let work = cx
            .background_executor()
            .spawn(async move { catalogue.playlist_tracks(playlist_id) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(tracks) => {
                        view.selected_playlist = Some(playlist_id);
                        view.selected_playlist_tracks = tracks;
                        view.playlist_error = None;
                    }
                    Err(error) => view.playlist_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn add_track_to_selected_playlist(&mut self, track: LibraryTrack, cx: &mut Context<Self>) {
        let (Some(catalogue), Some(playlist_id)) =
            (self.library_catalogue.clone(), self.selected_playlist)
        else {
            return;
        };
        let work = cx.background_executor().spawn(async move {
            catalogue.append_to_playlist(playlist_id, track.id())?;
            let tracks = catalogue.playlist_tracks(playlist_id)?;
            let playlists = catalogue.playlists()?;
            Ok::<_, crate::library::LibraryError>((tracks, playlists))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((tracks, playlists)) => {
                        view.selected_playlist_tracks = tracks;
                        view.playlists = playlists;
                        view.playlist_error = None;
                    }
                    Err(error) => view.playlist_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_selected_playlist_position(&mut self, position: usize, cx: &mut Context<Self>) {
        let (Some(catalogue), Some(playlist_id)) =
            (self.library_catalogue.clone(), self.selected_playlist)
        else {
            return;
        };
        let work = cx.background_executor().spawn(async move {
            let _ = catalogue.remove_playlist_position(playlist_id, position)?;
            let tracks = catalogue.playlist_tracks(playlist_id)?;
            let playlists = catalogue.playlists()?;
            Ok::<_, crate::library::LibraryError>((tracks, playlists))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((tracks, playlists)) => {
                        view.selected_playlist_tracks = tracks;
                        view.playlists = playlists;
                        view.playlist_error = None;
                    }
                    Err(error) => view.playlist_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn play_selected_playlist_position(&mut self, position: usize, cx: &mut Context<Self>) {
        let Some(track) = self.selected_playlist_tracks.get(position).cloned() else {
            return;
        };
        let following = self.selected_playlist_tracks[position + 1..].to_vec();
        self.queue.clear();
        for track in &following {
            self.queue.add(QueuedTrack::from(track));
        }
        self.open_queued_track(QueuedTrack::from(&track), cx);
    }

    fn play_library_track(&mut self, track: QueuedTrack, cx: &mut Context<Self>) {
        self.queue.clear();
        self.open_queued_track(track, cx);
    }

    fn play_track_next(&mut self, track: QueuedTrack, cx: &mut Context<Self>) {
        self.queue.play_next(track);
        cx.notify();
    }

    fn add_track_to_queue(&mut self, track: QueuedTrack, cx: &mut Context<Self>) {
        self.queue.add(track);
        cx.notify();
    }

    fn remove_queued_track(&mut self, position: usize, cx: &mut Context<Self>) {
        let _ = self.queue.remove(position);
        cx.notify();
    }

    fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.queue.clear();
        cx.notify();
    }

    fn open_queued_track(&mut self, track: QueuedTrack, cx: &mut Context<Self>) {
        let path = track.path().to_path_buf();
        let open = cx
            .background_executor()
            .spawn(async move { PlaybackSession::open(path) });
        cx.spawn(async move |this, cx| {
            let result = open.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(session) => {
                        view.install_session(session);
                        view.error = None;
                        view.seeking = false;
                        view.library_visible = false;
                    }
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn advance_queue_if_ended(&mut self, cx: &mut Context<Self>) {
        let transport = self
            .session
            .as_ref()
            .map_or(TransportState::Empty, |session| {
                session.transport_state().clone()
            });
        if self.queue_advance.observe(&transport)
            && let Some(track) = self.queue.take_next()
        {
            self.open_queued_track(track, cx);
        }
    }

    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.seeking {
            return;
        }
        let Some(action) = self
            .session
            .as_ref()
            .map(|session| playback_toggle(session.transport_state()))
        else {
            return;
        };
        match action {
            PlaybackToggle::Restart => {
                let position = self
                    .session
                    .as_ref()
                    .map_or(0.0, PlaybackSession::position_seconds);
                self.seek_by(-position, cx);
            }
            PlaybackToggle::Pause | PlaybackToggle::Play => {
                let session = self
                    .session
                    .as_mut()
                    .expect("playback action requires an active session");
                let result = match action {
                    PlaybackToggle::Pause => session.pause(),
                    PlaybackToggle::Play => session.play(),
                    PlaybackToggle::Restart | PlaybackToggle::Ignore => unreachable!(
                        "restart and ignore playback actions are handled before session mutation"
                    ),
                };
                if let Err(error) = result {
                    self.error = Some(error.to_string());
                }
                cx.notify();
            }
            PlaybackToggle::Ignore => {}
        }
    }

    fn adjust_volume(&mut self, delta: f32, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        session.set_volume(session.volume() + delta);
        cx.notify();
    }

    fn seek_by(&mut self, delta_seconds: f64, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let duration = session.metadata().duration_seconds().unwrap_or(f64::MAX);
        let target = (session.position_seconds() + delta_seconds).clamp(0.0, duration);
        let path = session.path().to_path_buf();
        let resume_paused = session.transport_state() == &TransportState::Paused;
        self.seeking = true;
        self.error = None;
        cx.notify();

        let rebuild = cx.background_executor().spawn(async move {
            let mut session = PlaybackSession::open_at(path, target)?;
            if resume_paused {
                session.pause()?;
            }
            Ok::<PlaybackSession, PlaybackSessionError>(session)
        });
        cx.spawn(async move |this, cx| {
            let result = rebuild.await;
            let _ = this.update(cx, |view, cx| {
                view.seeking = false;
                match result {
                    Ok(session) => view.install_session(session),
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_visualizer(&mut self, cx: &mut Context<Self>) {
        self.visualizer_mode = match self.visualizer_mode {
            VisualizerMode::Spectrum => VisualizerMode::Waveform,
            VisualizerMode::Waveform => VisualizerMode::MilkDrop,
            VisualizerMode::MilkDrop => VisualizerMode::Spectrum,
        };
        cx.notify();
    }
}

impl Focusable for PlayerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn format_time(seconds: f64) -> String {
    let total_seconds = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

fn signal_status(
    seeking: bool,
    transport: Option<&TransportState>,
    decode: Option<&DecodeWorkerState>,
) -> &'static str {
    if seeking {
        return "PCM / BUFFERING";
    }
    if transport == Some(&TransportState::Ended) {
        return "PCM / ENDED";
    }
    match decode {
        None => "PCM / IDLE",
        Some(DecodeWorkerState::Starting) => "PCM / BUFFERING",
        Some(DecodeWorkerState::Running) => "PCM / LIVE",
        Some(DecodeWorkerState::Ended) => "PCM / DRAINING",
        Some(DecodeWorkerState::Failed(_)) => "PCM / ERROR",
        Some(DecodeWorkerState::Stopped) => "PCM / STOPPED",
    }
}

fn milkdrop_status_label(status: &MilkdropStatus, is_playing: bool) -> String {
    match status {
        MilkdropStatus::Unavailable => "MilkDrop unavailable",
        MilkdropStatus::Initializing => "MilkDrop initializing",
        MilkdropStatus::Running if is_playing => "MilkDrop active",
        MilkdropStatus::Running => "MilkDrop paused",
        MilkdropStatus::Failed(error) => return format!("MilkDrop error: {error}"),
    }
    .into()
}

const PERFORMANCE_REPORT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq)]
struct PerformanceSample {
    elapsed: Duration,
    rendered_frames: u64,
    average_fps: f64,
    underflow_frames: u64,
}

#[derive(Debug)]
struct PerformanceProbe {
    enabled: bool,
    report_interval: Duration,
    baseline: Option<(Instant, u64, u64)>,
    last_report_at: Option<Instant>,
}

impl PerformanceProbe {
    fn from_env(variable: &str) -> Self {
        Self {
            enabled: std::env::var(variable).is_ok_and(|value| value == "1"),
            report_interval: PERFORMANCE_REPORT_INTERVAL,
            baseline: None,
            last_report_at: None,
        }
    }

    #[cfg(test)]
    const fn enabled(report_interval: Duration) -> Self {
        Self {
            enabled: true,
            report_interval,
            baseline: None,
            last_report_at: None,
        }
    }

    fn observe(
        &mut self,
        now: Instant,
        active: bool,
        rendered_frames: u64,
        underflow_frames: u64,
    ) -> Option<PerformanceSample> {
        if !self.enabled {
            return None;
        }
        if !active {
            self.baseline = None;
            self.last_report_at = None;
            return None;
        }

        let (started_at, initial_frames, initial_underflows) =
            *self.baseline.get_or_insert_with(|| {
                self.last_report_at = Some(now);
                (now, rendered_frames, underflow_frames)
            });
        let last_report_at = self.last_report_at.unwrap_or(started_at);
        if now.saturating_duration_since(last_report_at) < self.report_interval {
            return None;
        }

        self.last_report_at = Some(now);
        let elapsed = now.saturating_duration_since(started_at);
        let completed_frames = rendered_frames.saturating_sub(initial_frames);
        Some(PerformanceSample {
            elapsed,
            rendered_frames: completed_frames,
            average_fps: completed_frames as f64 / elapsed.as_secs_f64(),
            underflow_frames: underflow_frames.saturating_sub(initial_underflows),
        })
    }
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_library_search(window, cx);
        let canvas = self.color(self.skin.canvas());
        let panel = self
            .color(self.skin.panel())
            .opacity(self.skin.panel_opacity().unit());
        let panel_raised = self
            .color(self.skin.panel_raised())
            .opacity(self.skin.raised_panel_opacity().unit());
        let text = self.color(self.skin.text());
        let text_muted = self.color(self.skin.text_muted());
        let accent = self.color(self.skin.accent());
        let meter_low = self.color(self.skin.meter_low());
        let meter_high = self.color(self.skin.meter_high());
        let reduce_motion = cx.reduce_motion();
        let motion_label = if reduce_motion {
            "Reduced motion enabled; essential audio levels remain active"
        } else {
            "Standard motion enabled; essential audio levels remain active"
        };
        let status_opacity = if reduce_motion { 0.3 } else { 0.45 };
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.poll()
        {
            self.error = Some(error.to_string());
        }
        self.advance_queue_if_ended(cx);
        let snapshot = self
            .session
            .as_ref()
            .and_then(PlaybackSession::try_snapshot);
        let cosmic_signal =
            CosmicSignal::from_snapshot(snapshot.as_ref().map(|snapshot| snapshot.analysis()));
        let cosmic_motion = if reduce_motion {
            CosmicMotion::Reduced
        } else {
            CosmicMotion::Standard
        };
        if let Some(snapshot) = &snapshot {
            if let Some(error) = snapshot.output_error() {
                self.error = Some(error.to_owned());
            } else if let DecodeWorkerState::Failed(error) = snapshot.decode() {
                self.error = Some(error.clone());
            }
        }
        let has_session = self.session.is_some();
        let is_playing = self
            .session
            .as_ref()
            .is_some_and(|session| session.transport_state() == &TransportState::Playing);
        let milkdrop_active = self.visualizer_mode == VisualizerMode::MilkDrop
            && is_playing
            && matches!(self.milkdrop.status(), MilkdropStatus::Running);
        let underflow_frames = self
            .session
            .as_ref()
            .map_or(0, PlaybackSession::underflow_frames);
        if let Some(sample) = self.milkdrop_performance_probe.observe(
            Instant::now(),
            milkdrop_active,
            self.milkdrop.rendered_frames(),
            underflow_frames,
        ) {
            eprintln!(
                "AMP808_MILKDROP_METRICS elapsed_s={:.3} rendered_frames={} average_fps={:.3} underflow_frames={}",
                sample.elapsed.as_secs_f64(),
                sample.rendered_frames,
                sample.average_fps,
                sample.underflow_frames
            );
        }
        let cosmic_active = !reduce_motion
            && matches!(
                self.cosmic.status(),
                CosmicStatus::Running | CosmicStatus::Initializing
            );
        if let Some(sample) = self.cosmic_performance_probe.observe(
            Instant::now(),
            cosmic_active,
            self.cosmic.rendered_frames(),
            underflow_frames,
        ) {
            eprintln!(
                "AMP808_COSMIC_METRICS elapsed_s={:.3} rendered_frames={} average_fps={:.3} underflow_frames={}",
                sample.elapsed.as_secs_f64(),
                sample.rendered_frames,
                sample.average_fps,
                sample.underflow_frames
            );
        }
        if !should_present_milkdrop(self.library_visible, self.visualizer_mode) {
            self.milkdrop.hide();
        }
        let (visualizer_label, visualizer_action_label) = match self.visualizer_mode {
            VisualizerMode::Spectrum => ("SPECTRUM", "Show waveform visualizer"),
            VisualizerMode::Waveform => ("WAVEFORM", "Show MilkDrop visualizer"),
            VisualizerMode::MilkDrop => ("MILKDROP", "Show spectrum visualizer"),
        };
        let visualizer = match self.visualizer_mode {
            VisualizerMode::Spectrum => {
                let spectrum = snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.analysis().spectrum().to_vec())
                    .unwrap_or_else(|| vec![0.0; 24]);
                div()
                    .flex()
                    .flex_1()
                    .items_end()
                    .gap_1()
                    .rounded_md()
                    .bg(canvas.opacity(0.64))
                    .p_4()
                    .children(spectrum.into_iter().map(|magnitude| {
                        div()
                            .flex_1()
                            .h(px(3.0 + magnitude * 220.0))
                            .rounded_sm()
                            .bg(meter_low.opacity(0.18 + magnitude * 0.82))
                    }))
                    .into_any_element()
            }
            VisualizerMode::Waveform => {
                let waveform = snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.analysis().waveform().to_vec())
                    .unwrap_or_else(|| vec![0.0; 128]);
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .rounded_md()
                    .bg(canvas.opacity(0.64))
                    .p_4()
                    .children(waveform.into_iter().map(|sample| {
                        let positive = sample.max(0.0);
                        let negative = (-sample).max(0.0);
                        let opacity = 0.18 + sample.abs() * 0.82;
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .child(
                                div().flex().flex_1().items_end().child(
                                    div()
                                        .w_full()
                                        .h(px(1.0 + positive * 100.0))
                                        .bg(meter_high.opacity(opacity)),
                                ),
                            )
                            .child(
                                div().flex().flex_1().items_start().child(
                                    div()
                                        .w_full()
                                        .h(px(1.0 + negative * 100.0))
                                        .bg(meter_low.opacity(opacity)),
                                ),
                            )
                    }))
                    .into_any_element()
            }
            VisualizerMode::MilkDrop => {
                let status = self.milkdrop.status();
                let status_label = milkdrop_status_label(&status, is_playing);
                let status_message = match &status {
                    MilkdropStatus::Unavailable if !has_session => {
                        Some("LOAD A TRACK FOR MILKDROP")
                    }
                    MilkdropStatus::Unavailable => Some("MILKDROP UNAVAILABLE"),
                    MilkdropStatus::Initializing => None,
                    MilkdropStatus::Failed(_) => Some("MILKDROP RENDERER ERROR"),
                    MilkdropStatus::Running => None,
                };
                let mut visualizer = div()
                    .id("milkdrop-status")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(status_label)
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .bg(canvas.opacity(0.64))
                    .text_sm()
                    .text_color(text_muted);
                if matches!(
                    status,
                    MilkdropStatus::Initializing | MilkdropStatus::Running
                ) {
                    let host = self.milkdrop.clone();
                    let surface = gpui_canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _| host.present(bounds, window, is_playing),
                    )
                    .size_full();
                    visualizer = visualizer.child(surface);
                }
                if let Some(message) = status_message {
                    visualizer = visualizer.child(message);
                }
                visualizer.into_any_element()
            }
        };
        let play_label = if is_playing { "PAUSE" } else { "PLAY" };
        let play_accessibility_label = if is_playing { "Pause" } else { "Play" };
        let position = self.session.as_ref().map_or_else(
            || "00:00".to_owned(),
            |session| format_time(session.position_seconds()),
        );
        let duration = self.session.as_ref().map_or_else(
            || "00:00".to_owned(),
            |session| {
                session
                    .metadata()
                    .duration_seconds()
                    .map_or_else(|| "--:--".to_owned(), format_time)
            },
        );
        let progress_value = self.session.as_ref().and_then(|session| {
            let duration = session.metadata().duration_seconds()?;
            (duration.is_finite() && duration > 0.0)
                .then(|| (session.position_seconds() / duration * 100.0).clamp(0.0, 100.0))
        });
        let progress_accessibility_label =
            format!("Playback progress: {position} elapsed of {duration}");
        let volume = self
            .session
            .as_ref()
            .map_or(100, |session| (session.volume() * 100.0).round() as u32);
        let track_display = self
            .session
            .as_ref()
            .map_or_else(TrackDisplay::empty, |session| {
                let metadata = session.metadata();
                TrackDisplay::new(metadata.title(), metadata.artist(), metadata.album())
            });
        let track_accessibility_label = track_display.accessibility_label();
        let TrackDisplay {
            title,
            artist,
            album,
        } = track_display;
        let signal_status = signal_status(
            self.seeking,
            self.session.as_ref().map(PlaybackSession::transport_state),
            snapshot.as_ref().map(|snapshot| snapshot.decode()),
        );
        let playback_status_label = format!("Playback status: {signal_status}. {motion_label}");
        let detail = self.session.as_ref().map_or_else(
            || "SELECT A LOCAL FILE TO BEGIN".to_owned(),
            |session| {
                let metadata = session.metadata();
                format!("{} HZ / {} CH", metadata.sample_rate(), metadata.channels())
            },
        );
        let library_visible = self.library_visible;
        let library_tracks = match &self.library_status {
            LibraryStatus::Ready(tracks) => tracks.clone(),
            LibraryStatus::Unloaded | LibraryStatus::Loading | LibraryStatus::Failed(_) => {
                Vec::new()
            }
        };
        let queue_entries = self.queue.entries().to_vec();
        let queue_is_empty = self.queue.is_empty();
        let queue_status = match self.queue.len() {
            0 => "Up next: empty".to_owned(),
            1 => "Up next: 1 track".to_owned(),
            count => format!("Up next: {count} tracks"),
        };
        let playlists = self.playlists.clone();
        let selected_playlist = self.selected_playlist;
        let selected_playlist_tracks = self.selected_playlist_tracks.clone();
        let selected_playlist_name = selected_playlist.and_then(|selected| {
            playlists
                .iter()
                .find(|playlist| playlist.id() == selected)
                .map(|playlist| playlist.name().to_owned())
        });
        let playlist_status = selected_playlist_name.as_ref().map_or_else(
            || format!("Local playlists: {}. None selected", playlists.len()),
            |name| format!("Local playlists: {}. Selected: {name}", playlists.len()),
        );
        let library_selected_playlist_name = selected_playlist_name.clone();
        let library_status = match &self.library_status {
            LibraryStatus::Unloaded => "Local library has not been loaded".to_owned(),
            LibraryStatus::Loading => "Loading local library".to_owned(),
            LibraryStatus::Ready(tracks) if tracks.is_empty() => {
                "No matching local tracks. Add a folder or change the search.".to_owned()
            }
            LibraryStatus::Ready(tracks) => format!("{} local tracks", tracks.len()),
            LibraryStatus::Failed(error) => format!("Library error: {error}"),
        };
        let scan_detail = self.library_scan_summary.map(|summary| {
            format!(
                "LAST SCAN  {} FOUND  /  {} SKIPPED  /  {} REMOVED",
                summary.discovered(),
                summary.skipped(),
                summary.removed()
            )
        });
        let library_search = self
            .library_search
            .as_ref()
            .expect("library search is initialized before rendering")
            .clone();
        let playlist_name = self
            .playlist_name
            .as_ref()
            .expect("playlist name input is initialized before rendering")
            .clone();
        let playlist_panel = div()
            .id("playlist-panel")
            .test_support()
            .role(Role::Group)
            .aria_label("Local playlists")
            .flex()
            .flex_col()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(panel_raised)
            .bg(panel_raised)
            .p_3()
            .child(hardware_input(&playlist_name, "playlist-name", self.skin))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        hardware_button(
                            "playlist-create",
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .key_context("Amp808Control")
                        .label("CREATE")
                        .accessibility_label("Create local playlist")
                        .on_click(cx.listener(|this, _, _, cx| this.create_playlist(cx))),
                    )
                    .child(
                        hardware_button(
                            "playlist-rename",
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .key_context("Amp808Control")
                        .label("RENAME")
                        .accessibility_label("Rename selected local playlist")
                        .disabled(selected_playlist.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.rename_selected_playlist(cx))),
                    )
                    .child(
                        hardware_button(
                            "playlist-delete",
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .key_context("Amp808Control")
                        .label("DELETE")
                        .accessibility_label("Delete selected local playlist")
                        .disabled(selected_playlist.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.delete_selected_playlist(cx))),
                    ),
            )
            .child(
                div()
                    .id("playlist-status")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(playlist_status.clone())
                    .text_xs()
                    .text_color(text_muted)
                    .child(playlist_status),
            )
            .when_some(self.playlist_error.clone(), |panel, error| {
                panel.child(
                    div()
                        .id("playlist-error")
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(format!("Playlist error: {error}"))
                        .text_xs()
                        .text_color(accent)
                        .child(error),
                )
            })
            .children(playlists.into_iter().enumerate().map(|(index, playlist)| {
                let playlist_id = playlist.id();
                let name = playlist.name().to_owned();
                let count = playlist.track_count();
                hardware_button(
                    ("playlist-select", index),
                    if selected_playlist == Some(playlist_id) {
                        HardwareButtonTone::Primary
                    } else {
                        HardwareButtonTone::Neutral
                    },
                    self.skin,
                    cx,
                )
                .key_context("Amp808Control")
                .label(format!("{}  ({count})", playlist.name()))
                .accessibility_label(format!("Select playlist {name}, {count} tracks"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_playlist(playlist_id, cx);
                }))
            }))
            .children(
                selected_playlist_tracks
                    .into_iter()
                    .enumerate()
                    .map(|(position, track)| {
                        let playlist_name = selected_playlist_name
                            .clone()
                            .unwrap_or_else(|| "selected playlist".to_owned());
                        let remove_name = playlist_name.clone();
                        let title = track.title().to_owned();
                        let remove_title = title.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                hardware_button(
                                    ("playlist-track", position),
                                    HardwareButtonTone::Neutral,
                                    self.skin,
                                    cx,
                                )
                                .key_context("Amp808Control")
                                .label(track.title().to_owned())
                                .accessibility_label(format!(
                                    "Play {title} from {playlist_name} position {}",
                                    position + 1
                                ))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.play_selected_playlist_position(position, cx);
                                    },
                                )),
                            )
                            .child(
                                hardware_button(
                                    ("playlist-remove", position),
                                    HardwareButtonTone::Neutral,
                                    self.skin,
                                    cx,
                                )
                                .key_context("Amp808Control")
                                .label("REMOVE")
                                .accessibility_label(format!(
                                    "Remove {remove_title} from {remove_name} position {}",
                                    position + 1
                                ))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.remove_selected_playlist_position(position, cx);
                                    },
                                )),
                            )
                    }),
            );
        let queue_panel = div()
            .id("queue-panel")
            .test_support()
            .role(Role::Group)
            .aria_label("Up next queue")
            .flex()
            .flex_col()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(panel_raised)
            .bg(panel_raised)
            .p_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("queue-status")
                            .test_support()
                            .role(Role::Status)
                            .aria_label(queue_status.clone())
                            .text_xs()
                            .text_color(text_muted)
                            .child(queue_status),
                    )
                    .child(
                        hardware_button("clear-queue", HardwareButtonTone::Neutral, self.skin, cx)
                            .key_context("Amp808Control")
                            .label("CLEAR")
                            .accessibility_label("Clear up next queue")
                            .disabled(queue_is_empty)
                            .on_click(cx.listener(|this, _, _, cx| this.clear_queue(cx))),
                    ),
            )
            .children(
                queue_entries
                    .into_iter()
                    .enumerate()
                    .map(|(position, track)| {
                        let title = track.title().to_owned();
                        let artist = track.artist().unwrap_or("Unknown artist");
                        let album = track.album().unwrap_or("Unknown album");
                        hardware_button(
                            ("queue-track", position),
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .key_context("Amp808Control")
                        .label(format!(
                            "{}  —  {artist}  /  {album}  —  REMOVE",
                            track.title()
                        ))
                        .accessibility_label(format!(
                            "Remove {title} from queue position {}",
                            position + 1
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_queued_track(position, cx);
                        }))
                    }),
            );
        let library_panel = div()
            .id("library-panel")
            .test_support()
            .role(Role::Group)
            .aria_label("Local music library")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .rounded_lg()
            .border_1()
            .border_color(panel_raised)
            .bg(panel)
            .p_4()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child("LOCAL LIBRARY"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child("ON-DEVICE / PERSISTENT / SEARCHABLE"),
                            ),
                    )
                    .child(
                        hardware_button(
                            "add-library-folder",
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .key_context("Amp808Control")
                        .label("ADD FOLDER")
                        .accessibility_label("Add a folder to the local music library")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.choose_library_folder(cx);
                        })),
                    ),
            )
            .child(hardware_input(&library_search, "library-search", self.skin))
            .child(
                div()
                    .id("library-status")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(library_status.clone())
                    .text_xs()
                    .text_color(text_muted)
                    .child(library_status),
            )
            .when_some(scan_detail, |panel, detail| {
                panel.child(div().text_xs().text_color(text_muted).child(detail))
            })
            .child(playlist_panel)
            .child(queue_panel)
            .child(
                div()
                    .id("library-track-list")
                    .test_support()
                    .role(Role::Group)
                    .aria_label("Local library tracks")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_2()
                    .children(
                        library_tracks
                            .into_iter()
                            .enumerate()
                            .map(|(index, track)| {
                                let artist = track.artist().unwrap_or("Unknown artist");
                                let album = track.album().unwrap_or("Unknown album");
                                let label =
                                    format!("{}  —  {}  /  {}", track.title(), artist, album);
                                let title = track.title().to_owned();
                                let technical = format!(
                                    "{} hertz, {} channels, duration {}",
                                    track.sample_rate(),
                                    track.channels(),
                                    track
                                        .duration_seconds()
                                        .map_or_else(|| "unknown".to_owned(), format_time)
                                );
                                let queued_track = QueuedTrack::from(&track);
                                let play_now = queued_track.clone();
                                let play_next = queued_track.clone();
                                let next_title = title.clone();
                                let playlist_track = track.clone();
                                let playlist_name = library_selected_playlist_name
                                    .clone()
                                    .unwrap_or_else(|| "selected playlist".to_owned());
                                let can_add_to_playlist = selected_playlist.is_some();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        hardware_button(
                                            ("library-track", index),
                                            HardwareButtonTone::Primary,
                                            self.skin,
                                            cx,
                                        )
                                        .key_context("Amp808Control")
                                        .label(label.clone())
                                        .accessibility_label(format!("Play {label}. {technical}"))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.play_library_track(play_now.clone(), cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        hardware_button(
                                            ("library-play-next", index),
                                            HardwareButtonTone::Neutral,
                                            self.skin,
                                            cx,
                                        )
                                        .key_context("Amp808Control")
                                        .label("NEXT")
                                        .accessibility_label(format!("Play {next_title} next"))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.play_track_next(play_next.clone(), cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        hardware_button(
                                            ("library-add-queue", index),
                                            HardwareButtonTone::Neutral,
                                            self.skin,
                                            cx,
                                        )
                                        .key_context("Amp808Control")
                                        .label("QUEUE")
                                        .accessibility_label(format!("Add {title} to queue"))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.add_track_to_queue(queued_track.clone(), cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        hardware_button(
                                            ("library-add-playlist", index),
                                            HardwareButtonTone::Neutral,
                                            self.skin,
                                            cx,
                                        )
                                        .key_context("Amp808Control")
                                        .label("ADD")
                                        .accessibility_label(format!(
                                            "Add {title} to playlist {playlist_name}"
                                        ))
                                        .disabled(!can_add_to_playlist)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.add_track_to_selected_playlist(
                                                    playlist_track.clone(),
                                                    cx,
                                                );
                                            }),
                                        ),
                                    )
                            }),
                    ),
            );
        if !reduce_motion && !matches!(self.cosmic.status(), CosmicStatus::Failed(_)) {
            window.request_animation_frame();
        } else if self.visualizer_mode == VisualizerMode::MilkDrop
            && has_session
            && matches!(self.milkdrop.status(), MilkdropStatus::Initializing)
        {
            // The native view attaches during canvas paint. Request one follow-up frame so GPUI's
            // status text reflects the result without animating indefinitely while paused.
            window.request_animation_frame();
        }

        let player = div()
            .id("player-root")
            .test_support()
            .track_focus(&self.focus)
            .tab_group()
            .key_context("Amp808Player")
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| this.choose_file(cx)))
            .on_action(cx.listener(|this, _: &TogglePlayback, _, cx| {
                this.toggle_playback(cx);
            }))
            .on_action(cx.listener(|this, _: &SeekBack, _, cx| this.seek_by(-10.0, cx)))
            .on_action(cx.listener(|this, _: &SeekForward, _, cx| this.seek_by(10.0, cx)))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| {
                this.adjust_volume(-0.05, cx);
            }))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| {
                this.adjust_volume(0.05, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleVisualizer, _, cx| {
                this.toggle_visualizer(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleLibrary, _, cx| {
                this.toggle_library(cx);
            }))
            .flex()
            .flex_col()
            .size_full()
            .text_color(text)
            .p_5()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded_md()
                    .bg(panel)
                    .px_3()
                    .pt_2()
                    .border_b_1()
                    .border_color(accent)
                    .pb_3()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_3()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_semibold()
                                    .text_color(accent)
                                    .child(self.app.product_name()),
                            )
                            .child(
                                div()
                                    .pb_1()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child("MACOS PLAYER / LOCAL SIGNAL"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                hardware_button(
                                    "toggle-library",
                                    HardwareButtonTone::Neutral,
                                    self.skin,
                                    cx,
                                )
                                    .key_context("Amp808Control")
                                    .label(if library_visible { "PLAYER" } else { "LIBRARY" })
                                    .accessibility_label(if library_visible {
                                        "Show player"
                                    } else {
                                        "Show local music library"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_library(cx);
                                    })),
                            )
                            .child(
                                hardware_button(
                                    "open-file",
                                    HardwareButtonTone::Neutral,
                                    self.skin,
                                    cx,
                                )
                                    .key_context("Amp808Control")
                                    .label("OPEN FILE")
                                    .accessibility_label("Open a local audio file")
                                    .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |root, error| {
                root.child(
                    div()
                        .id("playback-error")
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(format!("Playback error: {error}"))
                        .flex()
                        .items_center()
                        .gap_3()
                        .rounded_md()
                        .border_1()
                        .border_color(accent)
                        .bg(panel_raised)
                        .px_4()
                        .py_3()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(accent)
                                .child("PLAYBACK ERROR"),
                        )
                        .child(div().text_sm().text_color(text).child(error)),
                )
            })
            .when_some(self.milkdrop_preset_error.clone(), |root, error| {
                root.child(
                    div()
                        .id("milkdrop-preset-error")
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(format!("MilkDrop preset error: {error}"))
                        .flex()
                        .items_center()
                        .gap_3()
                        .rounded_md()
                        .border_1()
                        .border_color(accent)
                        .bg(panel_raised)
                        .px_4()
                        .py_3()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(accent)
                                .child("PRESET ERROR"),
                        )
                        .child(div().text_sm().text_color(text).child(error)),
                )
            })
            .when_some(self.milkdrop_preset_notice.clone(), |root, notice| {
                root.child(
                    div()
                        .id("milkdrop-preset-status")
                        .test_support()
                        .role(Role::Status)
                        .aria_label(notice.clone())
                        .rounded_md()
                        .border_1()
                        .border_color(panel_raised)
                        .bg(panel)
                        .px_4()
                        .py_2()
                        .text_sm()
                        .text_color(text)
                        .child(notice),
                )
            })
            .when(library_visible, |root| root.child(library_panel))
            .when(!library_visible, |root| root.child(
                div()
                    .flex()
                    .flex_1()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(310.0))
                            .min_w(px(310.0))
                            .rounded_lg()
                            .border_1()
                            .border_color(panel_raised)
                            .bg(panel)
                            .p_4()
                            .gap_4()
                            .child(div().text_xs().text_color(text_muted).child("NOW PLAYING"))
                            .child(
                                div()
                                    .id("now-playing-summary")
                                    .test_support()
                                    .role(Role::Group)
                                    .aria_label(track_accessibility_label)
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .aspect_square()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(panel_raised)
                                    .bg(canvas.opacity(0.68))
                                    .text_color(text_muted)
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_semibold()
                                            .text_color(text)
                                            .child(title),
                                    )
                                    .child(div().text_sm().child(artist))
                                    .child(div().text_xs().text_color(text_muted).child(album)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        hardware_button(
                                            "seek-back",
                                            HardwareButtonTone::Neutral,
                                            self.skin,
                                            cx,
                                        )
                                            .key_context("Amp808Control")
                                            .label("-10S")
                                            .accessibility_label("Seek back 10 seconds")
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.seek_by(-10.0, cx);
                                            })),
                                    )
                                    .child(
                                        hardware_button(
                                            "play-pause",
                                            HardwareButtonTone::Primary,
                                            self.skin,
                                            cx,
                                        )
                                            .key_context("Amp808Control")
                                            .label(play_label)
                                            .accessibility_label(play_accessibility_label)
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_playback(cx);
                                            })),
                                    )
                                    .child(
                                        hardware_button(
                                            "seek-forward",
                                            HardwareButtonTone::Neutral,
                                            self.skin,
                                            cx,
                                        )
                                            .key_context("Amp808Control")
                                            .label("+10S")
                                            .accessibility_label("Seek forward 10 seconds")
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.seek_by(10.0, cx);
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        hardware_button(
                                            "volume-down",
                                            HardwareButtonTone::Neutral,
                                            self.skin,
                                            cx,
                                        )
                                            .key_context("Amp808Control")
                                            .label("VOL-")
                                            .accessibility_label("Decrease volume")
                                            .disabled(!has_session)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.adjust_volume(-0.05, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("volume-status")
                                            .test_support()
                                            .role(Role::Status)
                                            .aria_label(format!("Volume {volume} percent"))
                                            .flex_1()
                                            .text_center()
                                            .text_xs()
                                            .text_color(text_muted)
                                            .child(format!("VOL {volume:03}%")),
                                    )
                                    .child(
                                        hardware_button(
                                            "volume-up",
                                            HardwareButtonTone::Neutral,
                                            self.skin,
                                            cx,
                                        )
                                            .key_context("Amp808Control")
                                            .label("VOL+")
                                            .accessibility_label("Increase volume")
                                            .disabled(!has_session)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.adjust_volume(0.05, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .rounded_lg()
                            .border_1()
                            .border_color(panel_raised)
                            .bg(panel)
                            .p_4()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                hardware_button(
                                                    "visualizer-mode",
                                                    HardwareButtonTone::Neutral,
                                                    self.skin,
                                                    cx,
                                                )
                                                    .key_context("Amp808Control")
                                                    .accessibility_label(visualizer_action_label)
                                                    .px_2()
                                                    .py_1()
                                                    .label(visualizer_label)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.toggle_visualizer(cx);
                                                    })),
                                            )
                                            .when(
                                                self.visualizer_mode == VisualizerMode::MilkDrop,
                                                |controls| {
                                                    controls.child(
                                                        hardware_button(
                                                            "load-milkdrop-preset",
                                                            HardwareButtonTone::Neutral,
                                                            self.skin,
                                                            cx,
                                                        )
                                                            .key_context("Amp808Control")
                                                            .label("LOAD PRESET")
                                                            .accessibility_label(
                                                                "Load a local MilkDrop preset",
                                                            )
                                                            .px_2()
                                                            .py_1()
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.choose_milkdrop_preset(cx);
                                                                },
                                                            )),
                                                    )
                                                },
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("motion-policy")
                                            .test_support()
                                            .role(Role::Status)
                                            .aria_label(playback_status_label)
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .size_2()
                                                    .rounded_full()
                                                    .bg(meter_high.opacity(status_opacity)),
                                            )
                                            .child(signal_status),
                                    ),
                            )
                            .child(visualizer)
                            .child(
                                div()
                                    .id("playback-progress")
                                    .test_support()
                                    .role(Role::ProgressIndicator)
                                    .aria_label(progress_accessibility_label)
                                    .aria_min_numeric_value(0.0)
                                    .aria_max_numeric_value(100.0)
                                    .when_some(progress_value, |progress, value| {
                                        progress.aria_numeric_value(value)
                                    })
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(position)
                                    .child(detail)
                                    .child(duration),
                            ),
                    ),
            ))
            .child(
                div()
                    .id("keyboard-help")
                    .test_support()
                    .aria_label(
                        "Keyboard shortcuts: Command O open file, L switch library and player, Space play or pause, Left and Right seek, Up and Down adjust volume, V switch visualizer",
                    )
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .bg(panel)
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(text_muted)
                    .child(
                        "⌘O OPEN  ·  L LIBRARY  ·  SPACE PLAY/PAUSE  ·  ←/→ SEEK  ·  ↑/↓ VOLUME  ·  V VISUALIZER",
                    ),
            );

        let cosmic = self.cosmic.clone();
        let background = div()
            .id("cosmic-background")
            .test_support()
            .absolute()
            .inset_0()
            .child(
                gpui_canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        cosmic.present(bounds, window, cosmic_motion, cosmic_signal);
                    },
                )
                .size_full(),
            );

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(canvas)
            .child(background)
            .child(deferred(player))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, Focusable, Point, Role, TestAppContext, WindowBounds, WindowOptions,
        px, size,
    };
    use tempfile::tempdir;

    use crate::app::transport::TransportState;
    use crate::milkdrop::MilkdropStatus;
    use crate::playback::DecodeWorkerState;

    use super::{
        LibraryCatalogue, LibraryStatus, PerformanceProbe, PlaybackToggle, PlayerView, QueuedTrack,
        TrackDisplay, VisualizerMode, init_skin_theme, milkdrop_status_label, playback_toggle,
        should_present_milkdrop, signal_status, skin_hsla,
    };

    #[gpui_kit::test]
    fn skin_theme_projects_field_text_placeholder_and_focus_colors(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init_skin_theme(cx);

            let skin = crate::skin::Amp808Skin::built_in();
            let theme = gpui_kit::component::Theme::global(cx);
            assert_eq!(theme.background, skin_hsla(skin.field()));
            assert_eq!(theme.input, skin_hsla(skin.field_border()));
            assert_eq!(theme.foreground, skin_hsla(skin.text()));
            assert_eq!(theme.muted_foreground, skin_hsla(skin.text_muted()));
            assert_eq!(theme.ring, skin_hsla(skin.accent()));
        });
    }

    #[test]
    fn milkdrop_visibility_policy_hides_native_surface_in_library() {
        assert!(should_present_milkdrop(
            false,
            super::VisualizerMode::MilkDrop
        ));
        assert!(!should_present_milkdrop(
            true,
            super::VisualizerMode::MilkDrop
        ));
        assert!(!should_present_milkdrop(
            false,
            super::VisualizerMode::Spectrum
        ));
    }

    #[test]
    fn performance_probe_reports_average_fps_and_underflow_delta_from_its_active_baseline() {
        let mut probe = PerformanceProbe::enabled(Duration::from_secs(5));
        let started_at = Instant::now();

        assert_eq!(probe.observe(started_at, true, 100, 7), None);
        let sample = probe
            .observe(started_at + Duration::from_secs(5), true, 400, 9)
            .expect("five active seconds produce a performance sample");

        assert_eq!(sample.elapsed, Duration::from_secs(5));
        assert_eq!(sample.rendered_frames, 300);
        assert!((sample.average_fps - 60.0).abs() < f64::EPSILON);
        assert_eq!(sample.underflow_frames, 2);
    }

    #[test]
    fn performance_probe_restarts_qualification_after_milkdrop_becomes_inactive() {
        let mut probe = PerformanceProbe::enabled(Duration::from_secs(5));
        let started_at = Instant::now();

        assert_eq!(probe.observe(started_at, true, 100, 3), None);
        assert_eq!(
            probe.observe(started_at + Duration::from_secs(3), false, 280, 4),
            None
        );
        assert_eq!(
            probe.observe(started_at + Duration::from_secs(4), true, 300, 5),
            None
        );
        let sample = probe
            .observe(started_at + Duration::from_secs(9), true, 450, 5)
            .expect("the restarted active interval produces its own sample");

        assert_eq!(sample.elapsed, Duration::from_secs(5));
        assert_eq!(sample.rendered_frames, 150);
        assert_eq!(sample.underflow_frames, 0);
    }

    #[test]
    fn play_from_ended_requests_a_restart() {
        assert_eq!(
            playback_toggle(&TransportState::Ended),
            PlaybackToggle::Restart
        );
    }

    #[test]
    fn drained_transport_is_presented_as_ended() {
        assert_eq!(
            signal_status(
                false,
                Some(&TransportState::Ended),
                Some(&DecodeWorkerState::Ended)
            ),
            "PCM / ENDED"
        );
    }

    #[test]
    fn milkdrop_status_distinguishes_initializing_active_paused_and_failed() {
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Initializing, true),
            "MilkDrop initializing"
        );
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Running, true),
            "MilkDrop active"
        );
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Running, false),
            "MilkDrop paused"
        );
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Failed("preset rejected".into()), true),
            "MilkDrop error: preset rejected"
        );
    }

    #[test]
    fn track_display_preserves_available_title_artist_and_album_tags() {
        let display = TrackDisplay::new("Halcyon", Some("Orbital"), Some("Brown Album"));

        assert_eq!(display.title(), "Halcyon");
        assert_eq!(display.artist(), "Orbital");
        assert_eq!(display.album(), "Brown Album");
    }

    #[test]
    fn track_display_labels_missing_optional_tags_honestly() {
        let display = TrackDisplay::new("Untitled", None, None);

        assert_eq!(display.title(), "Untitled");
        assert_eq!(display.artist(), "UNKNOWN ARTIST");
        assert_eq!(display.album(), "UNKNOWN ALBUM");
    }

    #[gpui_kit::test]
    fn now_playing_metadata_is_exposed_to_assistive_technology(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let summary = window.find("now-playing-summary");
            assert_eq!(summary.role(), Some(Role::Group));
            assert_eq!(
                summary.label(),
                Some("No track loaded. Artist: --. Album: --.")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn volume_value_is_exposed_to_assistive_technology(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let volume = window.find("volume-status");
            assert_eq!(volume.role(), Some(Role::Status));
            assert_eq!(volume.label(), Some("Volume 100 percent"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn cosmic_background_keeps_gpui_overlay_controls_observable(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let view = cx.new(PlayerView::new);
                window.focus(&view.read(cx).focus_handle(cx), cx);
                view
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);

            assert!(window.find("cosmic-background").visible());
            assert_eq!(
                window.find("open-file").label(),
                Some("Open a local audio file")
            );
            assert_eq!(window.find("player-root").focused(), Some(true));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn pcm_status_is_exposed_with_the_motion_policy(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let status = window.find("motion-policy");
            assert_eq!(status.role(), Some(Role::Status));
            assert_eq!(
                status.label(),
                Some(
                    "Playback status: PCM / IDLE. Standard motion enabled; essential audio levels remain active"
                )
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn playback_time_is_exposed_as_progress(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let progress = window.find("playback-progress");
            assert_eq!(progress.role(), Some(Role::ProgressIndicator));
            assert_eq!(
                progress.label(),
                Some("Playback progress: 00:00 elapsed of 00:00")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn open_button_requests_one_local_file(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("open-file", cx);
        })
        .unwrap();

        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|options| {
            assert!(options.files);
            assert!(!options.directories);
            assert!(!options.multiple);
            None
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn empty_library_is_accessible_and_add_folder_requests_one_directory(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.library_visible = true;
                view.library_status = LibraryStatus::Ready(Vec::new());
                cx.notify();
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("library-panel").role(), Some(Role::Group));
            assert_eq!(
                window.find("library-panel").label(),
                Some("Local music library")
            );
            assert_eq!(
                window.find("library-status").label(),
                Some("No matching local tracks. Add a folder or change the search.")
            );
            assert_eq!(
                window.find("add-library-folder").label(),
                Some("Add a folder to the local music library")
            );
            window.click("add-library-folder", cx);
        })
        .unwrap();

        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|options| {
            assert!(!options.files);
            assert!(options.directories);
            assert!(!options.multiple);
            None
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn library_track_exposes_explicit_up_next_actions(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Signal Bloom.wav"), 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music fixture scans");
        let tracks = catalogue.search("").expect("library tracks load");

        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.library_visible = true;
                view.library_status = LibraryStatus::Ready(tracks);
                cx.notify();
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("queue-status").role(), Some(Role::Status));
            assert_eq!(window.find("queue-status").label(), Some("Up next: empty"));
            assert_eq!(
                window.find(("library-play-next", 0_usize)).label(),
                Some("Play Signal Bloom next")
            );
            assert_eq!(
                window.find(("library-add-queue", 0_usize)).label(),
                Some("Add Signal Bloom to queue")
            );
            window.click(("library-add-queue", 0_usize), cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("queue-status").label(),
                Some("Up next: 1 track")
            );
            assert_eq!(
                window.find(("queue-track", 0_usize)).label(),
                Some("Remove Signal Bloom from queue position 1")
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn failed_queued_track_exposes_an_error_without_discarding_the_tail(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary queue directory is created");
        let failed_path = directory.path().join("Missing Signal.wav");
        let failed = QueuedTrack::new(failed_path.clone(), "Missing Signal".to_owned(), None, None);
        let later = QueuedTrack::new(
            directory.path().join("Later Orbit.wav"),
            "Later Orbit".to_owned(),
            None,
            None,
        );

        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.queue.add(later);
                view.open_queued_track(failed, cx);
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(view.queue.entries().len(), 1);
            assert_eq!(view.queue.entries()[0].title(), "Later Orbit");
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("playback-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            assert!(
                alert
                    .label()
                    .is_some_and(|label| label.contains(failed_path.to_string_lossy().as_ref())),
                "failed queued path is exposed without consuming later entries"
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn starting_a_playlist_replaces_up_next_with_its_following_entries(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        for title in ["First Light", "Second Sun", "Third Orbit"] {
            write_stereo_wav(&music.join(format!("{title}.wav")), 8_000, 8_000);
        }
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music fixture scans");
        let tracks = catalogue.search("").expect("library tracks load");
        for track in &tracks {
            fs::remove_file(track.path()).expect("indexed fixture is removed before playback");
        }

        cx.update(gpui_kit::init);
        let (_window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.queue.add(QueuedTrack::new(
                    directory.path().join("Old Intent.wav"),
                    "Old Intent".to_owned(),
                    None,
                    None,
                ));
                view.selected_playlist_tracks = tracks;
                view.play_selected_playlist_position(1, cx);
                assert_eq!(view.queue.entries().len(), 1);
                assert_eq!(view.queue.entries()[0].title(), "Third Orbit");
            });
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn selected_playlist_accepts_a_library_track_and_exposes_ordered_playback(
        cx: &mut TestAppContext,
    ) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Signal Bloom.wav"), 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music fixture scans");
        let tracks = catalogue.search("").expect("library tracks load");
        let playlist = catalogue
            .create_playlist("Night Run")
            .expect("playlist is created");

        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.library_visible = true;
                view.library_catalogue = Some(catalogue);
                view.library_status = LibraryStatus::Ready(tracks);
                view.playlists = vec![playlist.clone()];
                view.selected_playlist = Some(playlist.id());
                cx.notify();
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("playlist-status").label(),
                Some("Local playlists: 1. Selected: Night Run")
            );
            assert_eq!(
                window.find(("library-add-playlist", 0_usize)).label(),
                Some("Add Signal Bloom to playlist Night Run")
            );
            window.click(("library-add-playlist", 0_usize), cx);
        })
        .expect("test window remains available");
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(("playlist-track", 0_usize)).label(),
                Some("Play Signal Bloom from Night Run position 1")
            );
            assert_eq!(
                window.find(("playlist-remove", 0_usize)).label(),
                Some("Remove Signal Bloom from Night Run position 1")
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn named_playlists_can_be_created_renamed_and_deleted_from_the_library(
        cx: &mut TestAppContext,
    ) {
        let directory = tempdir().expect("temporary library directory is created");
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");

        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.library_visible = true;
                view.library_catalogue = Some(catalogue);
                view.library_status = LibraryStatus::Ready(Vec::new());
                cx.notify();
            });
        });
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .expect("test window remains available");
        let playlist_name = cx.update(|cx| {
            view.read(cx)
                .playlist_name
                .clone()
                .expect("playlist name input is initialized")
        });

        cx.update_window(window, |_, window, cx| {
            playlist_name.update(cx, |input, cx| input.set_value("Night Run", window, cx));
            window.click("playlist-create", cx);
        })
        .expect("test window remains available");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("playlist-status").label(),
                Some("Local playlists: 1. Selected: Night Run")
            );
            assert_eq!(
                window.find(("playlist-select", 0_usize)).label(),
                Some("Select playlist Night Run, 0 tracks")
            );
            playlist_name.update(cx, |input, cx| input.set_value("Deep Orbit", window, cx));
            window.click("playlist-rename", cx);
        })
        .expect("test window remains available");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("playlist-status").label(),
                Some("Local playlists: 1. Selected: Deep Orbit")
            );
            window.click("playlist-delete", cx);
        })
        .expect("test window remains available");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("playlist-status").label(),
                Some("Local playlists: 0. None selected")
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn focused_library_track_accepts_keyboard_activation(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        let track_path = music.join("Signal Bloom.wav");
        write_stereo_wav(&track_path, 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music fixture scans");
        let tracks = catalogue.search("").expect("library tracks load");
        fs::remove_file(&track_path).expect("track is removed after indexing");

        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init_keyboard_shortcuts(cx);
        });
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let view = cx.new(PlayerView::new);
                window.focus(&view.read(cx).focus_handle(cx), cx);
                view
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.library_visible = true;
                view.library_status = LibraryStatus::Ready(tracks);
                cx.notify();
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("player-root").focused(), Some(true));
        })
        .expect("test window remains available");
        let mut track_reached = false;
        for _ in 0..32 {
            cx.update_window(window, |_, window, cx| window.press("tab", cx))
                .expect("test window remains available");
            cx.run_until_parked();
            track_reached = cx
                .update_window(window, |_, window, _| {
                    window.find(("library-track", 0_usize)).focused() == Some(true)
                })
                .expect("test window remains available");
            if track_reached {
                break;
            }
        }
        assert!(
            track_reached,
            "library track is reachable with Tab navigation"
        );

        cx.update_window(window, |_, window, cx| {
            let track = window.find(("library-track", 0_usize));
            assert_eq!(track.focused(), Some(true));
            assert!(
                track
                    .label()
                    .is_some_and(|label| label.starts_with("Play Signal Bloom")),
                "focused row exposes its playback action"
            );
            window.press("enter", cx);
        })
        .expect("test window remains available");
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("playback-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            assert!(
                alert
                    .label()
                    .is_some_and(|label| label.contains("Signal Bloom.wav")),
                "Enter activates the focused row through the production playback path"
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn corrupt_file_failure_is_exposed_as_an_accessible_alert(cx: &mut TestAppContext) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("broken.wav");
        fs::write(&path, b"not audio").unwrap();
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("open-file", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(move |_| Some(vec![path]));
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("playback-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            assert!(
                alert
                    .label()
                    .is_some_and(|label| label.contains("broken.wav"))
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn transport_controls_expose_accessible_desktop_actions(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("seek-back").label(),
                Some("Seek back 10 seconds")
            );
            assert_eq!(window.find("play-pause").label(), Some("Play"));
            assert_eq!(
                window.find("seek-forward").label(),
                Some("Seek forward 10 seconds")
            );
            assert_eq!(window.find("volume-down").label(), Some("Decrease volume"));
            assert_eq!(window.find("volume-up").label(), Some("Increase volume"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn visualizer_control_cycles_through_milkdrop_with_honest_unavailable_state(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show waveform visualizer")
            );
            window.click("visualizer-mode", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show MilkDrop visualizer")
            );
            window.click("visualizer-mode", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show spectrum visualizer")
            );
            assert_eq!(
                window.find("milkdrop-status").label(),
                Some("MilkDrop unavailable")
            );
            assert_eq!(
                window.find("load-milkdrop-preset").label(),
                Some("Load a local MilkDrop preset")
            );
            window.click("load-milkdrop-preset", cx);
        })
        .unwrap();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|options| {
            assert!(options.files);
            assert!(!options.directories);
            assert!(!options.multiple);
            assert_eq!(options.prompt.as_deref(), Some("Load a .milk preset"));
            None
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.click("visualizer-mode", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show waveform visualizer")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn invalid_local_milkdrop_file_is_reported_as_an_accessible_alert(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary preset directory is created");
        let path = directory.path().join("not-a-preset.txt");
        fs::write(&path, b"[preset00]").expect("invalid preset fixture is written");

        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.visualizer_mode = VisualizerMode::MilkDrop;
                cx.notify();
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("load-milkdrop-preset", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(move |_| Some(vec![path]));
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("milkdrop-preset-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            assert_eq!(
                alert.label(),
                Some("MilkDrop preset error: select a file with the .milk extension")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn focused_visualizer_control_accepts_keyboard_activation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init_keyboard_shortcuts(cx);
        });
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let view = cx.new(PlayerView::new);
                    window.focus(&view.read(cx).focus_handle(cx), cx);
                    view
                },
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("player-root").focused(), Some(true));
            window.press("tab", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("toggle-library").focused(), Some(true));
            window.press("tab", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("open-file").focused(), Some(true));
            window.press("tab", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("visualizer-mode").focused(), Some(true));

            window.press("enter", cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show MilkDrop visualizer")
            );
            window.press("space", cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show spectrum visualizer")
            );
            window.press("enter", cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show waveform visualizer")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn player_keyboard_shortcuts_open_files_and_switch_visualizers(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init_keyboard_shortcuts(cx);
        });
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let view = cx.new(PlayerView::new);
                    window.focus(&view.read(cx).focus_handle(cx), cx);
                    view
                },
            )
            .unwrap()
        });
        cx.update(|cx| {
            view.update(cx, |view, _| {
                view.library_status = LibraryStatus::Ready(Vec::new());
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.press("cmd-o", cx);
        })
        .unwrap();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.press("v", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show MilkDrop visualizer")
            );
            window.press("l", cx);
            window.render_frame(cx);
            assert!(window.find("library-panel").visible());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn keyboard_shortcuts_are_visible_and_accessibly_described(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let help = window.find("keyboard-help");
            assert_eq!(
                help.label(),
                Some(
                    "Keyboard shortcuts: Command O open file, L switch library and player, Space play or pause, Left and Right seek, Up and Down adjust volume, V switch visualizer"
                )
            );
            assert!(help.visible());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn reduced_motion_keeps_essential_audio_visualization_available(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let policy = window.find("motion-policy");
            assert_eq!(policy.role(), Some(Role::Status));
            assert_eq!(
                policy.label(),
                Some(
                    "Playback status: PCM / IDLE. Reduced motion enabled; essential audio levels remain active"
                )
            );
            assert!(window.find("visualizer-mode").visible());
        })
        .unwrap();
    }

    fn write_stereo_wav(path: &std::path::Path, sample_rate: u32, frame_count: u32) {
        let channel_count = 2_u16;
        let bits_per_sample = 16_u16;
        let bytes_per_sample = u32::from(bits_per_sample / 8);
        let data_size = frame_count * u32::from(channel_count) * bytes_per_sample;
        let mut bytes = Vec::with_capacity(44 + data_size as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channel_count.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(
            &(sample_rate * u32::from(channel_count) * bytes_per_sample).to_le_bytes(),
        );
        bytes.extend_from_slice(&(channel_count * (bits_per_sample / 8)).to_le_bytes());
        bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());
        bytes.resize(44 + data_size as usize, 0);

        fs::write(path, bytes).expect("WAV fixture is written");
    }
}
