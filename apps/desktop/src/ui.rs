use gpui_kit::base::{Disableable, StyledExt, box_shadow};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::{
    App, Context, ElementId, Entity, FocusHandle, Focusable, Hsla, Image, ImageFormat, IntoElement,
    KeyBinding, ObjectFit, PathPromptOptions, Render, Role, ScrollStrategy, SharedString,
    StyledImage, Subscription, TestSupportExt, UniformListScrollHandle, WeakEntity, Window,
    canvas as gpui_canvas, deferred, div, img, prelude::*, px, relative, rgb, uniform_list,
};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use amp808_core::bpm::BpmEstimate;

use crate::app::DesktopApp;
use crate::app::transport::TransportState;
use crate::apple_music::{
    AppleMusicCoordinator, AppleMusicId, AppleMusicSession, AppleMusicState, ApplePlaybackState,
    ApplePlaylistTrack, ApplePlaylistTracksState, AppleRepeatMode, AppleShuffleMode,
    CatalogPlayback, SubscriptionState,
};
use crate::cosmic::{CosmicBackgroundHost, CosmicMotion, CosmicSignal, CosmicStatus};
use crate::library::{LibraryCatalogue, LibraryTrack, LocalPlaylist, LocalPlaylistId, ScanSummary};
use crate::media::{CoverArt, TrackMetadata, probe_cover_art};
use crate::milkdrop::{MilkdropPreset, MilkdropStatus, MilkdropViewHost};
use crate::playback::{DecodeWorkerState, PlaybackSession, PlaybackSessionError};
use crate::preset_catalog::PresetCatalog;
use crate::preset_folder::{
    PresetFolderAccess, PresetFolderBookmarkStore, create_bookmark, restore_bookmark,
};
use crate::queue::{PlaybackQueue, QueueAdvance, QueuedTrack};
use crate::remote::RemoteSourceState;
use crate::remote_transfer::RemoteTransferCoordinator;
use crate::skin::Amp808Skin;
use crate::yt_dlp::{YtDlpError, YtDlpEvent, YtDlpRequest, YtDlpTransfer};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HardwareButtonTone {
    Neutral,
    Primary,
}

const APPLE_MUSIC_POLL_INTERVAL: Duration = Duration::from_millis(100);

struct AppleMusicPresentation {
    button_label: &'static str,
    accessibility_label: &'static str,
    status: String,
    disabled: bool,
    is_error: bool,
}

fn apple_music_presentation(
    session: &AppleMusicSession,
    error: Option<&str>,
) -> AppleMusicPresentation {
    if let Some(error) = error {
        return AppleMusicPresentation {
            button_label: "RETRY APPLE",
            accessibility_label: "Retry Apple Music connection",
            status: format!("APPLE MUSIC ERROR / {error}"),
            disabled: false,
            is_error: true,
        };
    }

    match session.state() {
        AppleMusicState::Unavailable => AppleMusicPresentation {
            button_label: "RETRY APPLE",
            accessibility_label: "Retry Apple Music connection",
            status: "APPLE MUSIC UNAVAILABLE".to_owned(),
            disabled: false,
            is_error: true,
        },
        AppleMusicState::Unauthorised => AppleMusicPresentation {
            button_label: "APPLE MUSIC",
            accessibility_label: "Connect Apple Music",
            status: "APPLE MUSIC / NOT CONNECTED".to_owned(),
            disabled: false,
            is_error: false,
        },
        AppleMusicState::Authorising { .. } => AppleMusicPresentation {
            button_label: "CONNECTING",
            accessibility_label: "Connecting to Apple Music",
            status: "APPLE MUSIC / WAITING FOR AUTHORISATION".to_owned(),
            disabled: true,
            is_error: false,
        },
        AppleMusicState::AccessDenied => AppleMusicPresentation {
            button_label: "RETRY APPLE",
            accessibility_label: "Retry Apple Music connection after access was denied",
            status: "APPLE MUSIC / ACCESS DENIED".to_owned(),
            disabled: false,
            is_error: true,
        },
        AppleMusicState::AccessRestricted => AppleMusicPresentation {
            button_label: "RETRY APPLE",
            accessibility_label: "Retry Apple Music connection after access was restricted",
            status: "APPLE MUSIC / ACCESS RESTRICTED".to_owned(),
            disabled: false,
            is_error: true,
        },
        AppleMusicState::AccessRevoked => AppleMusicPresentation {
            button_label: "RECONNECT APPLE",
            accessibility_label: "Reconnect Apple Music after access was revoked",
            status: "APPLE MUSIC / ACCESS REVOKED".to_owned(),
            disabled: false,
            is_error: true,
        },
        AppleMusicState::Ready
        | AppleMusicState::LoadingPlaylists { .. }
        | AppleMusicState::LibraryEmpty
        | AppleMusicState::LibraryLoaded { .. } => {
            let status = match session.subscription() {
                SubscriptionState::Available(capabilities)
                    if capabilities.catalog_playback() == CatalogPlayback::Available =>
                {
                    "APPLE MUSIC CONNECTED / CATALOG + LIBRARY"
                }
                SubscriptionState::Available(_) => "APPLE MUSIC CONNECTED / LIBRARY",
                SubscriptionState::Unavailable => {
                    "APPLE MUSIC CONNECTED / SUBSCRIPTION STATUS UNAVAILABLE"
                }
                SubscriptionState::NotChecked | SubscriptionState::Checking { .. } => {
                    "APPLE MUSIC CONNECTED / CHECKING SUBSCRIPTION"
                }
            };
            AppleMusicPresentation {
                button_label: "CONNECTED",
                accessibility_label: "Apple Music connected",
                status: status.to_owned(),
                disabled: true,
                is_error: false,
            }
        }
    }
}

fn apple_track_matches(track: &ApplePlaylistTrack, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let fields = [track.title(), track.artist(), track.album().unwrap_or("")];
    query.split_whitespace().all(|term| {
        fields
            .iter()
            .any(|field| library_fuzzy_score(term, field).is_some())
    })
}

fn apple_track_duration(duration_ms: Option<u64>) -> String {
    let Some(duration_ms) = duration_ms else {
        return "--:--".to_owned();
    };
    let total_seconds = duration_ms / 1_000;
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
}

fn apple_music_account_status(session: &AppleMusicSession, error: Option<&str>) -> String {
    if let Some(error) = error {
        return format!("Apple Music connection failed: {error}");
    }

    match session.state() {
        AppleMusicState::Unavailable => "Apple Music is unavailable on this Mac".to_owned(),
        AppleMusicState::Unauthorised => "Apple Music account is not connected".to_owned(),
        AppleMusicState::Authorising { .. } => {
            "Apple Music authorization is in progress".to_owned()
        }
        AppleMusicState::AccessDenied => "Apple Music access was denied".to_owned(),
        AppleMusicState::AccessRestricted => "Apple Music access is restricted".to_owned(),
        AppleMusicState::AccessRevoked => "Apple Music access was revoked".to_owned(),
        AppleMusicState::Ready
        | AppleMusicState::LoadingPlaylists { .. }
        | AppleMusicState::LibraryEmpty
        | AppleMusicState::LibraryLoaded { .. } => match session.subscription() {
            SubscriptionState::Available(capabilities)
                if capabilities.catalog_playback() == CatalogPlayback::Available =>
            {
                "Apple Music is connected with catalog and library access".to_owned()
            }
            SubscriptionState::Available(_) => {
                "Apple Music is connected with library access".to_owned()
            }
            SubscriptionState::Unavailable => {
                "Apple Music is connected; subscription status is unavailable".to_owned()
            }
            SubscriptionState::NotChecked | SubscriptionState::Checking { .. } => {
                "Apple Music is connected; checking subscription".to_owned()
            }
        },
    }
}

fn apple_music_poll_requires_render(
    previous_state: &AppleMusicState,
    previous_error: Option<&str>,
    current_state: &AppleMusicState,
    current_error: Option<&str>,
) -> bool {
    previous_state != current_state || previous_error != current_error
}

fn skin_hsla(color: crate::skin::SkinColor) -> Hsla {
    rgb(color.hex()).into()
}

fn skin_hsla_with_opacity(
    color: crate::skin::SkinColor,
    opacity: crate::skin::SkinOpacity,
) -> Hsla {
    skin_hsla(color).opacity(opacity.unit())
}

fn cover_image_from_art(art: Option<&CoverArt>) -> Option<Arc<Image>> {
    let art = art?;
    let format = ImageFormat::from_mime_type(art.media_type())?;
    Some(Arc::new(Image::from_bytes(format, art.data().to_vec())))
}

fn library_fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }

    let candidate = candidate.to_lowercase();
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let mut cursor = 0;
    let mut previous_match = None;
    let mut score = 0_i64;

    for needle in query.chars() {
        let offset = candidate_chars[cursor..]
            .iter()
            .position(|character| *character == needle)?;
        let matched = cursor + offset;
        score += 10;
        if previous_match.is_some_and(|previous| matched == previous + 1) {
            score += 12;
        }
        if matched == 0
            || candidate_chars
                .get(matched.wrapping_sub(1))
                .is_some_and(|character| !character.is_alphanumeric())
        {
            score += 8;
        }
        score -= i64::try_from(offset).unwrap_or(i64::MAX);
        previous_match = Some(matched);
        cursor = matched + 1;
    }

    if candidate.contains(&query) {
        score += 40;
    }
    Some(score)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LibrarySearchScope {
    All,
    Title,
    Artist,
    Album,
}

impl LibrarySearchScope {
    const ALL: [Self; 4] = [Self::All, Self::Title, Self::Artist, Self::Album];

    const fn label(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Title => "TITLE",
            Self::Artist => "ARTIST",
            Self::Album => "ALBUM",
        }
    }
}

fn library_metadata_score(
    query: &str,
    scope: LibrarySearchScope,
    title: &str,
    artist: Option<&str>,
    album: Option<&str>,
) -> Option<i64> {
    let fields = match scope {
        LibrarySearchScope::All => [Some(title), artist, album],
        LibrarySearchScope::Title => [Some(title), None, None],
        LibrarySearchScope::Artist => [artist, None, None],
        LibrarySearchScope::Album => [album, None, None],
    };

    query.split_whitespace().try_fold(0_i64, |total, term| {
        fields
            .iter()
            .flatten()
            .filter_map(|field| library_fuzzy_score(term, field))
            .max()
            .map(|score| total.saturating_add(score))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LibrarySortField {
    Title,
    Artist,
    Album,
    Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LibrarySortDirection {
    Ascending,
    Descending,
}

#[cfg(test)]
fn sort_library_tracks(
    tracks: &mut [LibraryTrack],
    field: LibrarySortField,
    direction: LibrarySortDirection,
) {
    tracks.sort_by(|left, right| compare_library_tracks(left, right, field, direction));
}

fn compare_library_tracks(
    left: &LibraryTrack,
    right: &LibraryTrack,
    field: LibrarySortField,
    direction: LibrarySortDirection,
) -> Ordering {
    let field_order = match field {
        LibrarySortField::Title => compare_text(left.title(), right.title()),
        LibrarySortField::Artist => compare_text(
            left.artist().unwrap_or_default(),
            right.artist().unwrap_or_default(),
        ),
        LibrarySortField::Album => compare_text(
            left.album().unwrap_or_default(),
            right.album().unwrap_or_default(),
        ),
        LibrarySortField::Duration => left
            .duration_seconds()
            .unwrap_or(f64::INFINITY)
            .total_cmp(&right.duration_seconds().unwrap_or(f64::INFINITY)),
    };
    let order = field_order
        .then_with(|| compare_text(left.title(), right.title()))
        .then_with(|| left.path().cmp(right.path()));
    match direction {
        LibrarySortDirection::Ascending => order,
        LibrarySortDirection::Descending => order.reverse(),
    }
}

fn compare_text(left: &str, right: &str) -> Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ArtworkKey {
    path: PathBuf,
    file_size: i64,
    modified_nanos: i64,
}

impl ArtworkKey {
    #[cfg(test)]
    fn for_test(path: &str, file_size: i64, modified_nanos: i64) -> Self {
        Self {
            path: PathBuf::from(path),
            file_size,
            modified_nanos,
        }
    }
}

impl From<&LibraryTrack> for ArtworkKey {
    fn from(track: &LibraryTrack) -> Self {
        let (file_size, modified_nanos) = track.file_fingerprint();
        Self {
            path: track.path().to_path_buf(),
            file_size,
            modified_nanos,
        }
    }
}

#[derive(Debug)]
enum ArtworkState<T> {
    Loading,
    Ready(T),
    Missing,
}

#[derive(Debug)]
struct ArtworkEntry<T> {
    serial: u64,
    state: ArtworkState<T>,
}

#[derive(Debug)]
struct ArtworkCache<T> {
    entries: HashMap<ArtworkKey, ArtworkEntry<T>>,
    next_serial: u64,
    capacity: usize,
}

impl<T> ArtworkCache<T> {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            next_serial: 0,
            capacity,
        }
    }

    fn request(
        &mut self,
        keys: impl IntoIterator<Item = ArtworkKey>,
        limit: usize,
    ) -> Vec<ArtworkKey> {
        let mut requested = Vec::new();
        for key in keys {
            if requested.len() >= limit || self.entries.contains_key(&key) {
                continue;
            }
            if self.entries.len() >= self.capacity && !self.evict_oldest_resolved() {
                break;
            }
            let serial = self.take_serial();
            self.entries.insert(
                key.clone(),
                ArtworkEntry {
                    serial,
                    state: ArtworkState::Loading,
                },
            );
            requested.push(key);
        }
        requested
    }

    fn resolve(&mut self, key: ArtworkKey, value: Option<T>) {
        let serial = self.take_serial();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.serial = serial;
            entry.state = value.map_or(ArtworkState::Missing, ArtworkState::Ready);
        }
    }

    fn value(&self, key: &ArtworkKey) -> Option<&T> {
        match self.entries.get(key).map(|entry| &entry.state) {
            Some(ArtworkState::Ready(value)) => Some(value),
            Some(ArtworkState::Loading | ArtworkState::Missing) | None => None,
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn evict_oldest_resolved(&mut self) -> bool {
        let oldest = self
            .entries
            .iter()
            .filter(|(_, entry)| !matches!(entry.state, ArtworkState::Loading))
            .min_by_key(|(_, entry)| entry.serial)
            .map(|(key, _)| key.clone());
        oldest.is_some_and(|key| self.entries.remove(&key).is_some())
    }

    fn take_serial(&mut self) -> u64 {
        let serial = self.next_serial;
        self.next_serial = self.next_serial.wrapping_add(1);
        serial
    }
}

struct LibraryTableDelegate {
    owner: WeakEntity<PlayerView>,
    skin: Amp808Skin,
    tracks: Vec<LibraryTrack>,
    visible: Vec<usize>,
    query: String,
    scope: LibrarySearchScope,
    sort_field: LibrarySortField,
    sort_direction: LibrarySortDirection,
    sort_active: bool,
    selected_playlist_name: Option<String>,
}

impl LibraryTableDelegate {
    fn new(owner: WeakEntity<PlayerView>, skin: Amp808Skin) -> Self {
        Self {
            owner,
            skin,
            tracks: Vec::new(),
            visible: Vec::new(),
            query: String::new(),
            scope: LibrarySearchScope::All,
            sort_field: LibrarySortField::Title,
            sort_direction: LibrarySortDirection::Ascending,
            sort_active: false,
            selected_playlist_name: None,
        }
    }

    fn set_tracks(&mut self, tracks: Vec<LibraryTrack>) {
        self.tracks = tracks;
        self.rebuild_visible();
    }

    fn set_query(&mut self, query: String) {
        self.query = query;
        self.rebuild_visible();
    }

    fn set_scope(&mut self, scope: LibrarySearchScope) {
        self.scope = scope;
        self.rebuild_visible();
    }

    fn set_selected_playlist_name(&mut self, name: Option<String>) {
        self.selected_playlist_name = name;
    }

    fn visible_count(&self) -> usize {
        self.visible.len()
    }

    fn is_current(
        &self,
        tracks: &[LibraryTrack],
        query: &str,
        scope: LibrarySearchScope,
        selected_playlist_name: Option<&str>,
    ) -> bool {
        self.tracks == tracks
            && self.query == query
            && self.scope == scope
            && self.selected_playlist_name.as_deref() == selected_playlist_name
    }

    fn track(&self, row_ix: usize) -> Option<&LibraryTrack> {
        self.visible
            .get(row_ix)
            .and_then(|track_ix| self.tracks.get(*track_ix))
    }

    fn rebuild_visible(&mut self) {
        let mut matches = self
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(track_ix, track)| {
                library_metadata_score(
                    &self.query,
                    self.scope,
                    track.title(),
                    track.artist(),
                    track.album(),
                )
                .map(|score| (track_ix, score))
            })
            .collect::<Vec<_>>();
        let tracks = &self.tracks;
        let field = self.sort_field;
        let direction = self.sort_direction;
        if !self.query.trim().is_empty() && !self.sort_active {
            matches.sort_by(|(left_ix, left_score), (right_ix, right_score)| {
                right_score.cmp(left_score).then_with(|| {
                    compare_library_tracks(
                        &tracks[*left_ix],
                        &tracks[*right_ix],
                        LibrarySortField::Title,
                        LibrarySortDirection::Ascending,
                    )
                })
            });
        } else {
            matches.sort_by(|(left_ix, _), (right_ix, _)| {
                compare_library_tracks(&tracks[*left_ix], &tracks[*right_ix], field, direction)
            });
        }
        self.visible = matches.into_iter().map(|(track_ix, _)| track_ix).collect();
    }

    fn column_sort(&self, field: LibrarySortField) -> ColumnSort {
        if !self.sort_active || self.sort_field != field {
            return ColumnSort::Default;
        }
        match self.sort_direction {
            LibrarySortDirection::Ascending => ColumnSort::Ascending,
            LibrarySortDirection::Descending => ColumnSort::Descending,
        }
    }

    fn artwork_for(&self, track: &LibraryTrack, cx: &App) -> Option<Arc<Image>> {
        let key = ArtworkKey::from(track);
        self.owner
            .read_with(cx, |view, _| view.library_artwork.value(&key).cloned())
            .ok()
            .flatten()
    }
}

impl TableDelegate for LibraryTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        6
    }

    fn rows_count(&self, _: &App) -> usize {
        self.visible.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        match col_ix {
            0 => Column::new("artwork", "ART")
                .width(px(64.0))
                .resizable(false)
                .movable(false)
                .selectable(false),
            1 => Column::new("title", "TITLE")
                .sort(self.column_sort(LibrarySortField::Title))
                .width(px(240.0))
                .fixed_left(),
            2 => Column::new("artist", "ARTIST")
                .sort(self.column_sort(LibrarySortField::Artist))
                .width(px(170.0)),
            3 => Column::new("album", "ALBUM")
                .sort(self.column_sort(LibrarySortField::Album))
                .width(px(190.0)),
            4 => Column::new("duration", "TIME")
                .sort(self.column_sort(LibrarySortField::Duration))
                .width(px(82.0))
                .text_right(),
            5 => Column::new("actions", "ACTIONS")
                .width(px(278.0))
                .selectable(false),
            _ => Column::new("unknown", ""),
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let field = match col_ix {
            1 => LibrarySortField::Title,
            2 => LibrarySortField::Artist,
            3 => LibrarySortField::Album,
            4 => LibrarySortField::Duration,
            _ => return,
        };
        self.sort_field = field;
        self.sort_active = true;
        self.sort_direction = match sort {
            ColumnSort::Descending => LibrarySortDirection::Descending,
            ColumnSort::Default | ColumnSort::Ascending => LibrarySortDirection::Ascending,
        };
        self.rebuild_visible();
        cx.notify();
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(track) = self.track(row_ix).cloned() else {
            return div().into_any_element();
        };
        let title = track.title().to_owned();
        let artist = track.artist().unwrap_or("Unknown artist").to_owned();
        let album = track.album().unwrap_or("Unknown album").to_owned();
        match col_ix {
            0 => self.artwork_for(&track, cx).map_or_else(
                || {
                    div()
                        .id(("library-artwork-placeholder", row_ix))
                        .test_support()
                        .flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .text_color(skin_hsla(self.skin.text_muted()))
                        .child("♪")
                        .into_any_element()
                },
                |image| {
                    div()
                        .id(("library-artwork", row_ix))
                        .test_support()
                        .role(Role::Image)
                        .aria_label(format!("Album artwork for {title}"))
                        .size_full()
                        .overflow_hidden()
                        .rounded_md()
                        .child(img(image).size_full().object_fit(ObjectFit::Cover))
                        .into_any_element()
                },
            ),
            1 => div()
                .size_full()
                .flex()
                .items_center()
                .font_semibold()
                .child(title)
                .into_any_element(),
            2 => div()
                .size_full()
                .flex()
                .items_center()
                .child(artist)
                .into_any_element(),
            3 => div()
                .size_full()
                .flex()
                .items_center()
                .child(album)
                .into_any_element(),
            4 => div()
                .size_full()
                .flex()
                .items_center()
                .justify_end()
                .child(
                    track
                        .duration_seconds()
                        .map_or_else(|| "--:--".to_owned(), format_time),
                )
                .into_any_element(),
            5 => {
                let play_label = format!(
                    "Play {title}  —  {artist}  /  {album}. {} hertz, {} channels, duration {}",
                    track.sample_rate(),
                    track.channels(),
                    track
                        .duration_seconds()
                        .map_or_else(|| "unknown".to_owned(), format_time)
                );
                let queued = QueuedTrack::from(&track);
                let play_now = queued.clone();
                let play_next = queued.clone();
                let queue_track = queued;
                let playlist_track = track;
                let next_title = title.clone();
                let queue_title = title.clone();
                let playlist_name = self
                    .selected_playlist_name
                    .clone()
                    .unwrap_or_else(|| "selected playlist".to_owned());
                let can_add_to_playlist = self.selected_playlist_name.is_some();
                let play_owner = self.owner.clone();
                let next_owner = self.owner.clone();
                let queue_owner = self.owner.clone();
                let playlist_owner = self.owner.clone();
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        hardware_button(
                            ("library-track", row_ix),
                            HardwareButtonTone::Primary,
                            self.skin,
                            cx,
                        )
                        .label("PLAY")
                        .accessibility_label(play_label)
                        .on_click(move |_, _, cx| {
                            let _ = play_owner.update(cx, |view, cx| {
                                view.play_library_track(play_now.clone(), cx);
                            });
                        }),
                    )
                    .child(
                        hardware_button(
                            ("library-play-next", row_ix),
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .label("NEXT")
                        .accessibility_label(format!("Play {next_title} next"))
                        .on_click(move |_, _, cx| {
                            let _ = next_owner.update(cx, |view, cx| {
                                view.play_track_next(play_next.clone(), cx);
                            });
                        }),
                    )
                    .child(
                        hardware_button(
                            ("library-add-queue", row_ix),
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .label("QUEUE")
                        .accessibility_label(format!("Add {queue_title} to queue"))
                        .on_click(move |_, _, cx| {
                            let _ = queue_owner.update(cx, |view, cx| {
                                view.add_track_to_queue(queue_track.clone(), cx);
                            });
                        }),
                    )
                    .child(
                        hardware_button(
                            ("library-add-playlist", row_ix),
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .label("ADD")
                        .accessibility_label(format!("Add {title} to playlist {playlist_name}"))
                        .disabled(!can_add_to_playlist)
                        .on_click(move |_, _, cx| {
                            let track = playlist_track.clone();
                            let _ = playlist_owner.update(cx, |view, cx| {
                                view.add_track_to_selected_playlist(track, cx);
                            });
                        }),
                    )
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, _: &App) -> Option<SharedString> {
        let track = self.track(row_ix)?;
        let label = match col_ix {
            0 => format!("Artwork for {}", track.title()),
            1 => format!(
                "Title: {}. {} hertz, {} channels",
                track.title(),
                track.sample_rate(),
                track.channels()
            ),
            2 => format!("Artist: {}", track.artist().unwrap_or("Unknown artist")),
            3 => format!("Album: {}", track.album().unwrap_or("Unknown album")),
            4 => format!(
                "Duration: {}",
                track
                    .duration_seconds()
                    .map_or_else(|| "unknown".to_owned(), format_time)
            ),
            5 => format!("Actions for {}", track.title()),
            _ => return None,
        };
        Some(label.into())
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child("NO MATCHING LOCAL TRACKS")
    }
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
        theme.border = skin_hsla(skin.control_border());
        theme.button = skin_hsla_with_opacity(skin.control(), skin.control_opacity());
        theme.button_hover =
            skin_hsla_with_opacity(skin.control_hover(), skin.control_hover_opacity());
        theme.button_active =
            skin_hsla_with_opacity(skin.control_active(), skin.control_active_opacity());
        theme.button_foreground = skin_hsla(skin.text());
        theme.button_primary =
            skin_hsla_with_opacity(skin.control_primary(), skin.control_primary_opacity());
        theme.button_primary_hover = skin_hsla_with_opacity(
            skin.control_primary_hover(),
            skin.control_primary_hover_opacity(),
        );
        theme.button_primary_active = skin_hsla_with_opacity(
            skin.control_primary_active(),
            skin.control_primary_active_opacity(),
        );
        theme.button_primary_foreground = skin_hsla(skin.control_primary_foreground());
        theme.secondary_foreground = skin_hsla(skin.text());
        theme.table = skin_hsla(skin.panel());
        theme.table_even = skin_hsla(skin.panel_raised());
        theme.table_head = skin_hsla(skin.field());
        theme.table_head_foreground = skin_hsla(skin.text());
        theme.table_foot = skin_hsla(skin.field());
        theme.table_foot_foreground = skin_hsla(skin.text());
        theme.table_hover = skin_hsla(skin.control_hover());
        theme.table_active = skin_hsla(skin.control_active());
        theme.table_active_border = skin_hsla(skin.accent());
        theme.table_row_border = skin_hsla(skin.control_border());
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
    _cx: &App,
) -> Button {
    let mut shadows = vec![
        box_shadow(
            0.0,
            3.0,
            8.0,
            -2.0,
            skin_hsla(crate::skin::SkinColor::new(0x00_00_00)).opacity(0.72),
        ),
        box_shadow(0.0, -1.0, 2.0, -1.0, skin_hsla(skin.text()).opacity(0.28)),
    ];
    if tone == HardwareButtonTone::Primary {
        shadows.push(box_shadow(
            0.0,
            0.0,
            12.0,
            -3.0,
            skin_hsla(skin.accent()).opacity(0.52),
        ));
    }

    let button = Button::new(id);
    let button = match tone {
        HardwareButtonTone::Neutral => button,
        HardwareButtonTone::Primary => button.primary(),
    };
    let border = match tone {
        HardwareButtonTone::Neutral => skin.control_border(),
        HardwareButtonTone::Primary => skin.accent(),
    };

    button
        .border_1()
        .rounded(px(8.0))
        .border_color(skin_hsla(border))
        .shadow(shadows)
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
        ActivateLibraryTrack,
        SelectPreviousPreset,
        SelectNextPreset,
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
        KeyBinding::new("down", VolumeDown, Some("Amp808Player && !PresetGallery")),
        KeyBinding::new("up", VolumeUp, Some("Amp808Player && !PresetGallery")),
        KeyBinding::new("v", ToggleVisualizer, Some("Amp808Player")),
        KeyBinding::new("l", ToggleLibrary, Some("Amp808Player")),
        KeyBinding::new("enter", ActivateLibraryTrack, Some("DataTable")),
        KeyBinding::new("up", SelectPreviousPreset, Some("PresetGallery")),
        KeyBinding::new("down", SelectNextPreset, Some("PresetGallery")),
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct TechnicalMetadataDisplay {
    bpm: String,
    bpm_accessibility: String,
    container: String,
    codec: String,
    sample_rate: String,
    channels: String,
    bit_depth: String,
    duration: String,
    bitrate: String,
    accessibility_label: String,
}

impl TechnicalMetadataDisplay {
    fn new(metadata: Option<&TrackMetadata>, bpm: BpmEstimate) -> Self {
        let bpm = match bpm {
            BpmEstimate::Unavailable => ("--".to_owned(), "unavailable".to_owned()),
            BpmEstimate::Estimating => ("EST".to_owned(), "estimating".to_owned()),
            BpmEstimate::Locked(bpm) => (bpm.to_string(), bpm.to_string()),
        };
        let container = metadata.map_or_else(
            || "--".to_owned(),
            |metadata| metadata.container().to_ascii_uppercase(),
        );
        let codec = metadata.and_then(TrackMetadata::codec).map_or_else(
            || "--".to_owned(),
            |codec| codec.replace('_', " ").to_ascii_uppercase(),
        );
        let sample_rate = metadata.map_or_else(
            || "--".to_owned(),
            |metadata| format!("{} HZ", metadata.sample_rate()),
        );
        let channels = metadata.map_or_else(
            || "--".to_owned(),
            |metadata| format!("{} CH", metadata.channels()),
        );
        let bit_depth = metadata
            .and_then(TrackMetadata::bits_per_sample)
            .map_or_else(|| "--".to_owned(), |bits| format!("{bits} BIT"));
        let duration = metadata
            .and_then(TrackMetadata::duration_seconds)
            .map_or_else(|| "--:--".to_owned(), format_time);
        let bitrate = "--".to_owned();
        let accessibility_label = metadata.map_or_else(
            || {
                "Audio details. BPM unavailable. Container unavailable. Codec unavailable. Sample rate unavailable. Channels unavailable. Bit depth unavailable. Duration unavailable. Bitrate unavailable."
                    .to_owned()
            },
            |metadata| {
                let codec = metadata.codec().map_or("unavailable", |value| value);
                let bits = metadata.bits_per_sample().map_or_else(
                    || "Bit depth unavailable".to_owned(),
                    |value| format!("{value} bit"),
                );
                let duration = metadata
                    .duration_seconds()
                    .map_or_else(|| "unavailable".to_owned(), format_time);
                format!(
                    "Audio details. BPM {}. {} container. {} codec. {} hertz. {} channels. {}. Duration {}. Bitrate unavailable.",
                    bpm.1,
                    metadata.container().to_ascii_uppercase(),
                    codec.replace('_', " ").to_ascii_uppercase(),
                    metadata.sample_rate(),
                    metadata.channels(),
                    bits,
                    duration,
                )
            },
        );
        Self {
            bpm: bpm.0,
            bpm_accessibility: bpm.1,
            container,
            codec,
            sample_rate,
            channels,
            bit_depth,
            duration,
            bitrate,
            accessibility_label,
        }
    }

    #[cfg(test)]
    fn bpm(&self) -> &str {
        &self.bpm
    }

    #[cfg(test)]
    fn codec(&self) -> &str {
        &self.codec
    }

    #[cfg(test)]
    fn container(&self) -> &str {
        &self.container
    }

    #[cfg(test)]
    fn bit_depth(&self) -> &str {
        &self.bit_depth
    }

    #[cfg(test)]
    fn bitrate(&self) -> &str {
        &self.bitrate
    }

    #[cfg(test)]
    fn accessibility_label(&self) -> &str {
        &self.accessibility_label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PresetPreviewState {
    applied: MilkdropPreset,
    previewed: Option<MilkdropPreset>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PresetSelectionDirection {
    Previous,
    Next,
}

impl PresetPreviewState {
    fn new(applied: MilkdropPreset) -> Self {
        Self {
            applied,
            previewed: None,
        }
    }

    #[cfg(test)]
    fn applied(&self) -> &MilkdropPreset {
        &self.applied
    }

    fn preview_succeeded(&mut self, preset: MilkdropPreset) {
        self.previewed = Some(preset);
    }

    fn use_preview(&mut self) {
        if let Some(previewed) = self.previewed.take() {
            self.applied = previewed;
        }
    }

    fn previewed(&self) -> Option<&MilkdropPreset> {
        self.previewed.as_ref()
    }

    fn restore_target(&self) -> Option<&MilkdropPreset> {
        self.previewed.as_ref().map(|_| &self.applied)
    }

    fn restore_succeeded(&mut self) {
        self.previewed = None;
    }

    fn apply_explicit(&mut self, preset: MilkdropPreset) {
        self.applied = preset;
        self.previewed = None;
    }
}

pub struct PlayerView {
    app: DesktopApp,
    focus: FocusHandle,
    skin: Amp808Skin,
    session: Option<PlaybackSession>,
    cosmic: CosmicBackgroundHost,
    milkdrop: MilkdropViewHost,
    cover_image: Option<Arc<Image>>,
    milkdrop_preset_notice: Option<String>,
    milkdrop_preset_error: Option<String>,
    preset_catalog: PresetCatalog,
    preset_visible_indices: Vec<usize>,
    preset_search: Option<Entity<InputState>>,
    preset_search_subscription: Option<Subscription>,
    preset_category: Option<String>,
    preset_selected_id: Option<String>,
    preset_focus: FocusHandle,
    preset_scroll: UniformListScrollHandle,
    preset_preview: PresetPreviewState,
    preset_folder_loading: bool,
    preset_folder_access: Option<PresetFolderAccess>,
    preset_bookmark_store: Option<PresetFolderBookmarkStore>,
    error: Option<String>,
    seeking: bool,
    visualizer_mode: VisualizerMode,
    milkdrop_performance_probe: PerformanceProbe,
    cosmic_performance_probe: PerformanceProbe,
    library_visible: bool,
    settings_visible: bool,
    library_catalogue: Option<LibraryCatalogue>,
    library_status: LibraryStatus,
    library_search: Option<Entity<InputState>>,
    library_search_subscription: Option<Subscription>,
    library_table: Option<Entity<TableState<LibraryTableDelegate>>>,
    library_search_scope: LibrarySearchScope,
    library_query_generation: u64,
    library_scan_summary: Option<ScanSummary>,
    library_artwork: ArtworkCache<Arc<Image>>,
    playlists: Vec<LocalPlaylist>,
    selected_playlist: Option<LocalPlaylistId>,
    selected_playlist_tracks: Vec<LibraryTrack>,
    playlist_name: Option<Entity<InputState>>,
    playlist_error: Option<String>,
    queue: PlaybackQueue,
    queue_advance: QueueAdvance,
    remote_source_form_visible: bool,
    remote_source_input: Option<Entity<InputState>>,
    remote_source_error: Option<String>,
    remote_source: Option<RemoteTransferCoordinator>,
    remote_transfer: Option<YtDlpTransfer>,
    apple_music: AppleMusicCoordinator,
    apple_music_polling: bool,
    selected_apple_playlist: Option<AppleMusicId>,
    apple_track_search: Option<Entity<InputState>>,
    apple_track_search_subscription: Option<Subscription>,
    apple_track_scroll: UniformListScrollHandle,
}

impl PlayerView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let preset_catalog = PresetCatalog::built_in();
        let preset_visible_indices = preset_catalog.search("", None);
        let preset_selected_id = preset_catalog
            .entries()
            .first()
            .map(|entry| entry.id().to_owned());
        #[cfg(not(test))]
        let (preset_bookmark_store, preset_store_error) =
            match PresetFolderBookmarkStore::open_default() {
                Ok(store) => (Some(store), None),
                Err(error) => (None, Some(error.to_string())),
            };
        #[cfg(test)]
        let (preset_bookmark_store, preset_store_error) = (None, None);
        let mut view = Self {
            app: DesktopApp::new(),
            focus: cx.focus_handle(),
            skin: Amp808Skin::built_in(),
            session: None,
            cosmic: CosmicBackgroundHost::new(),
            milkdrop: MilkdropViewHost::new(),
            cover_image: None,
            milkdrop_preset_notice: None,
            milkdrop_preset_error: preset_store_error,
            preset_catalog,
            preset_visible_indices,
            preset_search: None,
            preset_search_subscription: None,
            preset_category: None,
            preset_selected_id,
            preset_focus: cx.focus_handle().tab_stop(true),
            preset_scroll: UniformListScrollHandle::new(),
            preset_preview: PresetPreviewState::new(MilkdropPreset::first_light()),
            preset_folder_loading: false,
            preset_folder_access: None,
            preset_bookmark_store,
            error: None,
            seeking: false,
            visualizer_mode: VisualizerMode::Spectrum,
            milkdrop_performance_probe: PerformanceProbe::from_env("AMP808_MILKDROP_METRICS"),
            cosmic_performance_probe: PerformanceProbe::from_env("AMP808_COSMIC_METRICS"),
            library_visible: false,
            settings_visible: false,
            library_catalogue: None,
            library_status: LibraryStatus::Unloaded,
            library_search: None,
            library_search_subscription: None,
            library_table: None,
            library_search_scope: LibrarySearchScope::All,
            library_query_generation: 0,
            library_scan_summary: None,
            library_artwork: ArtworkCache::new(96),
            playlists: Vec::new(),
            selected_playlist: None,
            selected_playlist_tracks: Vec::new(),
            playlist_name: None,
            playlist_error: None,
            queue: PlaybackQueue::new(),
            queue_advance: QueueAdvance::new(),
            remote_source_form_visible: false,
            remote_source_input: None,
            remote_source_error: None,
            remote_source: None,
            remote_transfer: None,
            apple_music: AppleMusicCoordinator::new(),
            apple_music_polling: false,
            selected_apple_playlist: None,
            apple_track_search: None,
            apple_track_search_subscription: None,
            apple_track_scroll: UniformListScrollHandle::new(),
        };
        if let Some(store) = view.preset_bookmark_store.clone() {
            view.restore_preset_folder(store, cx);
        }
        view
    }

    fn open_remote_source_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.remote_source_input.get_or_insert_with(|| {
            cx.new(|cx| {
                InputState::new(window, cx).placeholder("Paste an HTTP(S) audio source URL")
            })
        });
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.remote_source_form_visible = true;
        cx.notify();
    }

    fn choose_remote_destination(&mut self, cx: &mut Context<Self>) {
        let source_url = self
            .remote_source_input
            .as_ref()
            .map_or_else(String::new, |input| input.read(cx).value().to_string());
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose where yt-dlp stores the completed file".into()),
        });
        cx.spawn(async move |this, cx| {
            let destination = match selection.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.remote_source_error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |view, cx| {
                        view.remote_source_error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(destination) = destination else {
                return;
            };
            let transfer =
                YtDlpRequest::new(source_url, destination).and_then(YtDlpTransfer::start);
            let _ = this.update(cx, |view, cx| match transfer {
                Ok(transfer) => {
                    view.remote_source = Some(RemoteTransferCoordinator::new("yt-dlp source"));
                    view.remote_transfer = Some(transfer);
                    view.remote_source_form_visible = false;
                    view.remote_source_error = None;
                    view.schedule_remote_poll(cx);
                    cx.notify();
                }
                Err(error) => {
                    view.remote_source_error = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn cancel_remote_source_form(&mut self, cx: &mut Context<Self>) {
        self.remote_source_form_visible = false;
        self.remote_source_error = None;
        cx.notify();
    }

    fn schedule_remote_poll(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(Duration::from_millis(50)).await;
                let active = this
                    .update(cx, |view, cx| view.poll_remote_transfer(cx))
                    .unwrap_or(false);
                if !active {
                    break;
                }
            }
        })
        .detach();
    }

    fn poll_remote_transfer(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        let mut terminal = false;
        loop {
            let event = match self.remote_transfer.as_ref().map(YtDlpTransfer::try_recv) {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Empty)) => break,
                Some(Err(TryRecvError::Disconnected)) => YtDlpEvent::Failed(
                    YtDlpError::ReadOutput("yt-dlp event channel disconnected".to_owned()),
                ),
                None => break,
            };
            terminal = matches!(
                event,
                YtDlpEvent::Completed { .. } | YtDlpEvent::Cancelled | YtDlpEvent::Failed(_)
            );
            let Some(source) = self.remote_source.as_mut() else {
                self.remote_source_error = Some("yt-dlp transfer state is unavailable".to_owned());
                terminal = true;
                break;
            };
            if let Err(error) = source.apply(event) {
                self.remote_source_error = Some(error.to_string());
                terminal = true;
            }
            changed = true;
            if terminal {
                break;
            }
        }
        if terminal {
            self.remote_transfer.take();
        }
        if changed || terminal {
            cx.notify();
        }
        self.remote_transfer.is_some()
    }

    fn cancel_remote_transfer(&mut self, cx: &mut Context<Self>) {
        if let Some(transfer) = self.remote_transfer.as_ref() {
            transfer.cancellation_handle().cancel();
            cx.notify();
        }
    }

    fn retry_remote_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remote_transfer.take();
        self.remote_source = None;
        self.remote_source_error = None;
        self.open_remote_source_form(window, cx);
    }

    fn dismiss_remote_transfer(&mut self, cx: &mut Context<Self>) {
        self.remote_transfer.take();
        self.remote_source = None;
        self.remote_source_error = None;
        cx.notify();
    }

    fn connect_apple_music(&mut self, cx: &mut Context<Self>) {
        self.apple_music.connect();
        if matches!(
            self.apple_music.session().state(),
            AppleMusicState::Authorising { .. }
        ) {
            self.schedule_apple_music_poll(cx);
        }
        cx.notify();
    }

    fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_visible = !self.settings_visible;
        if self.settings_visible {
            self.library_visible = false;
            self.milkdrop.hide();
        }
        cx.notify();
    }

    fn select_apple_playlist(&mut self, id: AppleMusicId, cx: &mut Context<Self>) {
        self.selected_apple_playlist = Some(id.clone());
        self.apple_music.load_playlist_tracks(id);
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn play_apple_track(
        &mut self,
        id: AppleMusicId,
        playlist_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(playlist_id) = self.selected_apple_playlist.clone() else {
            return;
        };
        self.apple_music.play_track(playlist_id, id, playlist_index);
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn play_selected_apple_playlist(&mut self, cx: &mut Context<Self>) {
        if !self.apple_music.can_play_loaded_playlist() {
            return;
        }
        self.apple_music.play_loaded_playlist();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn pause_apple_playback(&mut self, cx: &mut Context<Self>) {
        self.apple_music.pause();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn resume_apple_playback(&mut self, cx: &mut Context<Self>) {
        self.apple_music.resume();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn seek_apple_playback_by(&mut self, delta_ms: i64, cx: &mut Context<Self>) {
        self.apple_music.seek_by(delta_ms);
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn cycle_apple_repeat(&mut self, cx: &mut Context<Self>) {
        self.apple_music.cycle_repeat();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn toggle_apple_shuffle(&mut self, cx: &mut Context<Self>) {
        self.apple_music.toggle_shuffle();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn stop_apple_playback(&mut self, cx: &mut Context<Self>) {
        self.apple_music.stop();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn play_next_apple_track(&mut self, cx: &mut Context<Self>) {
        self.apple_music.skip_to_next();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn play_previous_apple_track(&mut self, cx: &mut Context<Self>) {
        self.apple_music.skip_to_previous();
        self.schedule_apple_music_poll(cx);
        cx.notify();
    }

    fn show_apple_playlists(&mut self, cx: &mut Context<Self>) {
        self.selected_apple_playlist = None;
        cx.notify();
    }

    fn ensure_apple_track_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.apple_track_search.is_some() {
            return;
        }
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search playlist title, artist, or album")
        });
        let subscription = cx.subscribe(&search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        self.apple_track_search = Some(search);
        self.apple_track_search_subscription = Some(subscription);
    }

    fn schedule_apple_music_poll(&mut self, cx: &mut Context<Self>) {
        if self.apple_music_polling {
            return;
        }
        self.apple_music_polling = true;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(APPLE_MUSIC_POLL_INTERVAL).await;
                let active = this
                    .update(cx, |view, cx| {
                        let active = view.poll_apple_music_connection(cx);
                        if !active {
                            view.apple_music_polling = false;
                        }
                        active
                    })
                    .unwrap_or(false);
                if !active {
                    break;
                }
            }
        })
        .detach();
    }

    fn poll_apple_music_connection(&mut self, cx: &mut Context<Self>) -> bool {
        let previous_state = self.apple_music.session().state().clone();
        let previous_error = self.apple_music.error().map(str::to_owned);
        let previous_tracks = self.apple_music.playlist_tracks().clone();
        let previous_track_error = self.apple_music.track_error().map(str::to_owned);
        let previous_playback = self.apple_music.playback().clone();
        let previous_playback_error = self.apple_music.playback_error().map(str::to_owned);
        let previous_position = self.apple_music.playback_position();
        let previous_position_error = self
            .apple_music
            .playback_position_error()
            .map(str::to_owned);
        let active = self.apple_music.poll();
        if apple_music_poll_requires_render(
            &previous_state,
            previous_error.as_deref(),
            self.apple_music.session().state(),
            self.apple_music.error(),
        ) || &previous_tracks != self.apple_music.playlist_tracks()
            || previous_track_error.as_deref() != self.apple_music.track_error()
            || &previous_playback != self.apple_music.playback()
            || previous_playback_error.as_deref() != self.apple_music.playback_error()
            || previous_position != self.apple_music.playback_position()
            || previous_position_error.as_deref() != self.apple_music.playback_position_error()
        {
            cx.notify();
        }
        active
    }

    fn color(&self, value: crate::skin::SkinColor) -> gpui_kit::Rgba {
        rgb(value.hex())
    }

    fn install_session(&mut self, mut session: PlaybackSession) {
        self.cover_image = cover_image_from_art(session.metadata().cover_art());
        self.milkdrop.replace_pcm(session.take_milkdrop_pcm());
        self.session = Some(session);
    }

    fn clear_session(&mut self) {
        self.session = None;
        self.cover_image = None;
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
                            view.preset_preview.apply_explicit(preset.clone());
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

    fn ensure_preset_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preset_search.is_some() {
            return;
        }
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search title or preset type"));
        let subscription = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.refresh_preset_gallery(cx);
            }
        });
        self.preset_search = Some(search);
        self.preset_search_subscription = Some(subscription);
    }

    fn refresh_preset_gallery(&mut self, cx: &mut Context<Self>) {
        let query = self
            .preset_search
            .as_ref()
            .map_or_else(String::new, |search| search.read(cx).value().to_string());
        self.preset_visible_indices = self
            .preset_catalog
            .search(&query, self.preset_category.as_deref());
        if self.preset_selected_id.as_ref().is_none_or(|selected| {
            !self
                .preset_visible_indices
                .iter()
                .any(|index| self.preset_catalog.entries()[*index].id() == selected.as_str())
        }) {
            self.preset_selected_id = self
                .preset_visible_indices
                .first()
                .map(|index| self.preset_catalog.entries()[*index].id().to_owned());
        }
        cx.notify();
    }

    fn set_preset_category(&mut self, category: Option<String>, cx: &mut Context<Self>) {
        self.preset_category = category;
        self.refresh_preset_gallery(cx);
    }

    fn select_preset(&mut self, id: String, cx: &mut Context<Self>) {
        self.preset_selected_id = Some(id);
        cx.notify();
    }

    fn move_preset_selection(
        &mut self,
        direction: PresetSelectionDirection,
        cx: &mut Context<Self>,
    ) {
        let Some(last_row) = self.preset_visible_indices.len().checked_sub(1) else {
            return;
        };
        let current_row = self
            .preset_selected_id
            .as_deref()
            .and_then(|selected| {
                self.preset_visible_indices.iter().position(|entry_index| {
                    self.preset_catalog.entries()[*entry_index].id() == selected
                })
            })
            .unwrap_or(0);
        let next_row = match direction {
            PresetSelectionDirection::Previous => current_row.saturating_sub(1),
            PresetSelectionDirection::Next => current_row.saturating_add(1).min(last_row),
        };
        let entry_index = self.preset_visible_indices[next_row];
        self.preset_selected_id = Some(self.preset_catalog.entries()[entry_index].id().to_owned());
        self.preset_scroll
            .scroll_to_item(next_row, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn restore_preset_folder(&mut self, store: PresetFolderBookmarkStore, cx: &mut Context<Self>) {
        self.preset_folder_loading = true;
        self.milkdrop_preset_notice = Some("Restoring local MilkDrop presets…".to_owned());
        self.milkdrop_preset_error = None;
        let restore = cx.background_executor().spawn(async move {
            let Some(bookmark) = store.load().map_err(|error| error.to_string())? else {
                return Ok::<_, String>(None);
            };
            let restored = restore_bookmark(&bookmark).map_err(|error| error.to_string())?;
            if let Some(refreshed) = restored.refreshed_bookmark() {
                store.save(refreshed).map_err(|error| error.to_string())?;
            }
            let path = restored.access().path().to_path_buf();
            let entries =
                PresetCatalog::discover_local(&path).map_err(|error| error.to_string())?;
            Ok(Some((restored.into_access(), entries, path)))
        });
        cx.spawn(async move |this, cx| {
            let result = restore.await;
            let _ = this.update(cx, |view, cx| {
                view.preset_folder_loading = false;
                match result {
                    Ok(Some((access, entries, path))) => {
                        let count = entries.len();
                        view.preset_folder_access = Some(access);
                        view.preset_catalog.replace_local(entries);
                        view.preset_category = None;
                        view.milkdrop_preset_notice = Some(format!(
                            "Restored {count} local MilkDrop presets from {}",
                            path.display()
                        ));
                        view.milkdrop_preset_error = None;
                        view.refresh_preset_gallery(cx);
                    }
                    Ok(None) => {
                        view.milkdrop_preset_notice = None;
                        cx.notify();
                    }
                    Err(error) => {
                        view.milkdrop_preset_notice = None;
                        view.milkdrop_preset_error =
                            Some(format!("Could not restore local MilkDrop presets: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn choose_preset_folder(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add a MilkDrop preset folder".into()),
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
            let _ = this.update(cx, |view, cx| {
                view.preset_folder_loading = true;
                view.milkdrop_preset_notice = Some("Scanning local MilkDrop presets…".to_owned());
                view.milkdrop_preset_error = None;
                cx.notify();
            });
            let scan_path = path.clone();
            let bookmark_store = this
                .read_with(cx, |view, _| view.preset_bookmark_store.clone())
                .ok()
                .flatten();
            let scan = cx.background_executor().spawn(async move {
                if let Some(store) = bookmark_store {
                    let prepared =
                        create_bookmark(&scan_path).map_err(|error| error.to_string())?;
                    let entries = PresetCatalog::discover_local(prepared.access().path())
                        .map_err(|error| error.to_string())?;
                    store
                        .save(prepared.bookmark())
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((Some(prepared.into_access()), entries))
                } else {
                    PresetCatalog::discover_local(&scan_path)
                        .map(|entries| (None, entries))
                        .map_err(|error| error.to_string())
                }
            });
            let result = scan.await;
            let _ = this.update(cx, |view, cx| {
                view.preset_folder_loading = false;
                match result {
                    Ok((access, entries)) => {
                        let count = entries.len();
                        if let Some(access) = access {
                            view.preset_folder_access = Some(access);
                        }
                        view.preset_catalog.replace_local(entries);
                        view.preset_category = None;
                        view.milkdrop_preset_notice = Some(format!(
                            "Added {count} local MilkDrop presets from {}",
                            path.display()
                        ));
                        view.milkdrop_preset_error = None;
                        view.refresh_preset_gallery(cx);
                    }
                    Err(error) => {
                        view.milkdrop_preset_notice = None;
                        view.milkdrop_preset_error = Some(error.to_string());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn preview_selected_preset(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.preset_selected_id.as_deref() else {
            return;
        };
        let Some(entry) = self
            .preset_catalog
            .entries()
            .iter()
            .find(|entry| entry.id() == selected)
            .cloned()
        else {
            return;
        };
        let name = entry.title().to_owned();
        self.milkdrop_preset_notice = Some(format!("Loading preview: {name}"));
        self.milkdrop_preset_error = None;
        let load = cx.background_executor().spawn(async move { entry.load() });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(preset) => match view.milkdrop.install_preset(&preset) {
                        Ok(()) => {
                            view.preset_preview.preview_succeeded(preset);
                            view.milkdrop_preset_notice = Some(format!("Previewing: {name}"));
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
        cx.notify();
    }

    fn use_previewed_preset(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self
            .preset_preview
            .previewed()
            .map(|preset| preset.name().to_owned())
        else {
            return;
        };
        self.preset_preview.use_preview();
        self.milkdrop_preset_notice = Some(format!("Using MilkDrop preset: {name}"));
        self.milkdrop_preset_error = None;
        cx.notify();
    }

    fn restore_applied_preset(&mut self, cx: &mut Context<Self>) {
        let Some(applied) = self.preset_preview.restore_target().cloned() else {
            return;
        };
        match self.milkdrop.install_preset(&applied) {
            Ok(()) => {
                let name = applied.name().to_owned();
                self.preset_preview.restore_succeeded();
                self.milkdrop_preset_notice = Some(format!("Restored MilkDrop preset: {name}"));
                self.milkdrop_preset_error = None;
            }
            Err(error) => {
                self.milkdrop_preset_notice = None;
                self.milkdrop_preset_error = Some(error.to_string());
            }
        }
        cx.notify();
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
        if self.library_table.is_none() {
            let owner = cx.weak_entity();
            let mut delegate = LibraryTableDelegate::new(owner, self.skin);
            let tracks = match &self.library_status {
                LibraryStatus::Ready(tracks) => tracks.clone(),
                LibraryStatus::Unloaded | LibraryStatus::Loading | LibraryStatus::Failed(_) => {
                    Vec::new()
                }
            };
            let query = self
                .library_search
                .as_ref()
                .map_or_else(String::new, |search| search.read(cx).value().to_string());
            let selected_playlist_name = self.selected_playlist.and_then(|selected| {
                self.playlists
                    .iter()
                    .find(|playlist| playlist.id() == selected)
                    .map(|playlist| playlist.name().to_owned())
            });
            delegate.set_tracks(tracks);
            delegate.set_query(query);
            delegate.set_scope(self.library_search_scope);
            delegate.set_selected_playlist_name(selected_playlist_name);
            let table = cx.new(|cx| {
                TableState::new(delegate, window, cx)
                    .row_selectable(true)
                    .col_selectable(false)
                    .cell_selectable(false)
                    .col_movable(true)
                    .col_resizable(true)
                    .sortable(true)
            });
            cx.subscribe(&table, |this, table, event, cx| {
                if let TableEvent::DoubleClickedRow(row_ix) = event
                    && let Some(track) = table.read(cx).delegate().track(*row_ix)
                {
                    this.play_library_track(QueuedTrack::from(track), cx);
                }
            })
            .detach();
            self.library_table = Some(table);
        }
        if self.playlist_name.is_none() {
            self.playlist_name =
                Some(cx.new(|cx| InputState::new(window, cx).placeholder("Playlist name")));
        }
    }

    fn toggle_library(&mut self, cx: &mut Context<Self>) {
        self.library_visible = !self.library_visible;
        if self.library_visible {
            self.settings_visible = false;
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
                        view.schedule_library_artwork(&tracks, cx);
                        view.library_catalogue = Some(catalogue);
                        view.library_status = LibraryStatus::Ready(tracks);
                        view.playlists = playlists;
                        view.sync_library_table(cx);
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
        self.sync_library_table(cx);
        cx.notify();
    }

    fn set_library_search_scope(&mut self, scope: LibrarySearchScope, cx: &mut Context<Self>) {
        self.library_search_scope = scope;
        self.sync_library_table(cx);
        cx.notify();
    }

    fn sync_library_table(&mut self, cx: &mut Context<Self>) {
        let Some(table) = self.library_table.clone() else {
            return;
        };
        let tracks = match &self.library_status {
            LibraryStatus::Ready(tracks) => tracks.as_slice(),
            LibraryStatus::Unloaded | LibraryStatus::Loading | LibraryStatus::Failed(_) => &[],
        };
        let query = self
            .library_search
            .as_ref()
            .map_or_else(String::new, |search| search.read(cx).value().to_string());
        let selected_playlist_name = self.selected_playlist.and_then(|selected| {
            self.playlists
                .iter()
                .find(|playlist| playlist.id() == selected)
                .map(|playlist| playlist.name().to_owned())
        });
        if table.read(cx).delegate().is_current(
            tracks,
            &query,
            self.library_search_scope,
            selected_playlist_name.as_deref(),
        ) {
            return;
        }
        let tracks = tracks.to_vec();
        table.update(cx, |state, cx| {
            state.delegate_mut().set_tracks(tracks);
            state.delegate_mut().set_query(query);
            state.delegate_mut().set_scope(self.library_search_scope);
            state
                .delegate_mut()
                .set_selected_playlist_name(selected_playlist_name);
            state.refresh(cx);
            cx.notify();
        });
    }

    fn refresh_library_table(&mut self, cx: &mut Context<Self>) {
        if let Some(table) = self.library_table.clone() {
            table.update(cx, |state, cx| {
                state.refresh(cx);
                cx.notify();
            });
        }
    }

    fn activate_selected_library_track(&mut self, cx: &mut Context<Self>) {
        let track = self.library_table.as_ref().and_then(|table| {
            let state = table.read(cx);
            state
                .selected_row()
                .and_then(|row_ix| state.delegate().track(row_ix))
                .cloned()
        });
        if let Some(track) = track {
            self.play_library_track(QueuedTrack::from(&track), cx);
        }
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
                let tracks = catalogue.search("")?;
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
                        view.schedule_library_artwork(&tracks, cx);
                        view.library_catalogue = Some(catalogue);
                        view.library_scan_summary = Some(summary);
                        view.library_status = LibraryStatus::Ready(tracks);
                        view.playlists = playlists;
                        view.sync_library_table(cx);
                    }
                    Err(error) => view.library_status = LibraryStatus::Failed(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn schedule_library_artwork(&mut self, tracks: &[LibraryTrack], cx: &mut Context<Self>) {
        const REQUEST_LIMIT: usize = 48;

        let requests = self
            .library_artwork
            .request(tracks.iter().map(ArtworkKey::from), REQUEST_LIMIT);
        if requests.is_empty() {
            return;
        }
        let load = cx.background_executor().spawn(async move {
            requests
                .into_iter()
                .map(|key| {
                    let artwork = probe_cover_art(&key.path).ok().flatten();
                    (key, artwork)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let artwork = load.await;
            let _ = this.update(cx, |view, cx| {
                for (key, artwork) in artwork {
                    view.library_artwork
                        .resolve(key, cover_image_from_art(artwork.as_ref()));
                }
                view.refresh_library_table(cx);
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
                        view.sync_library_table(cx);
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
                        view.sync_library_table(cx);
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
                        view.sync_library_table(cx);
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
                        view.sync_library_table(cx);
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
                        view.settings_visible = false;
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
        self.ensure_preset_search(window, cx);
        self.ensure_library_search(window, cx);
        self.ensure_apple_track_search(window, cx);
        self.sync_library_table(cx);
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
        let AppleMusicPresentation {
            button_label: apple_button_label,
            accessibility_label: apple_accessibility_label,
            status: apple_status,
            disabled: apple_connection_disabled,
            is_error: apple_status_is_error,
        } = apple_music_presentation(self.apple_music.session(), self.apple_music.error());
        let apple_account_status =
            apple_music_account_status(self.apple_music.session(), self.apple_music.error());
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
        let bpm = if self.session.is_some() {
            snapshot
                .as_ref()
                .map_or(BpmEstimate::Estimating, |snapshot| {
                    snapshot.analysis().bpm()
                })
        } else {
            BpmEstimate::Unavailable
        };
        let technical_metadata = TechnicalMetadataDisplay::new(
            self.session.as_ref().map(PlaybackSession::metadata),
            bpm,
        );
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
        if !should_present_milkdrop(
            self.library_visible || self.settings_visible,
            self.visualizer_mode,
        ) {
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
        let visualizer = if self.visualizer_mode == VisualizerMode::MilkDrop {
            let preset_search = self
                .preset_search
                .as_ref()
                .expect("preset search is initialized before gallery rendering")
                .clone();
            let categories = self.preset_catalog.categories();
            let visible_count = self.preset_visible_indices.len();
            let total_count = self.preset_catalog.entries().len();
            let selected_title = self.preset_selected_id.as_deref().and_then(|selected| {
                self.preset_catalog
                    .entries()
                    .iter()
                    .find(|entry| entry.id() == selected)
                    .map(|entry| entry.title().to_owned())
            });
            let preview_label = selected_title.as_ref().map_or_else(
                || "Preview selected preset".to_owned(),
                |title| format!("Preview {title}"),
            );
            let has_preview = self.preset_preview.previewed().is_some();
            let gallery_status = if self.preset_folder_loading {
                "Scanning local MilkDrop presets".to_owned()
            } else {
                format!("{visible_count} of {total_count} presets shown")
            };
            let preset_scroll = self.preset_scroll.clone();
            let preset_list = uniform_list(
                "preset-gallery-results",
                visible_count,
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .filter_map(|row| {
                            let entry_index = *this.preset_visible_indices.get(row)?;
                            let entry = this.preset_catalog.entries().get(entry_index)?;
                            let id = entry.id().to_owned();
                            let title = entry.title().to_owned();
                            let category = entry.category().to_owned();
                            let selected = this.preset_selected_id.as_deref() == Some(id.as_str());
                            let preset_focus = this.preset_focus.clone();
                            let button = hardware_button(
                                ("preset-gallery-row", row),
                                if selected {
                                    HardwareButtonTone::Primary
                                } else {
                                    HardwareButtonTone::Neutral
                                },
                                this.skin,
                                cx,
                            )
                            .tab_stop(selected)
                            .w_full()
                            .label(format!("{title}  ·  {category}"))
                            .accessibility_label(format!("Select {title}, {category} preset"))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.select_preset(id.clone(), cx);
                                    window.focus(&preset_focus, cx);
                                },
                            ));
                            let button = if selected {
                                button.track_focus(&this.preset_focus)
                            } else {
                                button
                            };
                            Some(div().w_full().key_context("PresetGallery").child(button))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .w_full()
            .h_full()
            .track_scroll(&preset_scroll);
            let gallery = div()
                .id("preset-gallery")
                .test_support()
                .role(Role::Group)
                .aria_label("MilkDrop preset gallery")
                .flex()
                .flex_col()
                .flex_shrink_0()
                .w(px(330.0))
                .min_h_0()
                .rounded_md()
                .border_1()
                .border_color(panel_raised)
                .bg(canvas.opacity(0.72))
                .p_3()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_sm().font_semibold().child("PRESET GALLERY"))
                        .child(
                            hardware_button(
                                "add-preset-folder",
                                HardwareButtonTone::Neutral,
                                self.skin,
                                cx,
                            )
                            .label("ADD FOLDER")
                            .accessibility_label("Add a local MilkDrop preset folder")
                            .disabled(self.preset_folder_loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_preset_folder(cx);
                            })),
                        ),
                )
                .child(
                    hardware_input(&preset_search, "preset-gallery-search", self.skin)
                        .aria_label("Search MilkDrop presets by title or type"),
                )
                .child(
                    div()
                        .id("preset-category-filter")
                        .test_support()
                        .role(Role::Group)
                        .aria_label("MilkDrop preset type filter")
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .child(
                            hardware_button(
                                ("preset-category", 0_usize),
                                if self.preset_category.is_none() {
                                    HardwareButtonTone::Primary
                                } else {
                                    HardwareButtonTone::Neutral
                                },
                                self.skin,
                                cx,
                            )
                            .label("ALL")
                            .accessibility_label(if self.preset_category.is_none() {
                                "Show all preset types, selected"
                            } else {
                                "Show all preset types"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_preset_category(None, cx);
                            })),
                        )
                        .children(categories.into_iter().enumerate().map(|(index, category)| {
                            let selected =
                                self.preset_category.as_deref() == Some(category.as_str());
                            let label = category.to_uppercase();
                            hardware_button(
                                ("preset-category", index + 1),
                                if selected {
                                    HardwareButtonTone::Primary
                                } else {
                                    HardwareButtonTone::Neutral
                                },
                                self.skin,
                                cx,
                            )
                            .label(label)
                            .accessibility_label(format!(
                                "Show {category} presets{}",
                                if selected { ", selected" } else { "" }
                            ))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.set_preset_category(Some(category.clone()), cx);
                                },
                            ))
                        })),
                )
                .child(
                    div()
                        .id("preset-gallery-status")
                        .test_support()
                        .role(Role::Status)
                        .aria_label(gallery_status.clone())
                        .text_xs()
                        .text_color(text_muted)
                        .child(gallery_status),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(preset_list),
                )
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            hardware_button(
                                "preview-preset",
                                HardwareButtonTone::Primary,
                                self.skin,
                                cx,
                            )
                            .label("PREVIEW")
                            .accessibility_label(preview_label)
                            .disabled(
                                !has_session
                                    || selected_title.is_none()
                                    || self.preset_folder_loading,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preview_selected_preset(cx);
                            })),
                        )
                        .child(
                            hardware_button(
                                "use-previewed-preset",
                                HardwareButtonTone::Neutral,
                                self.skin,
                                cx,
                            )
                            .label("USE")
                            .accessibility_label("Use the previewed MilkDrop preset")
                            .disabled(!has_preview)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.use_previewed_preset(cx);
                            })),
                        )
                        .child(
                            hardware_button(
                                "restore-applied-preset",
                                HardwareButtonTone::Neutral,
                                self.skin,
                                cx,
                            )
                            .label("RESTORE")
                            .accessibility_label("Restore the applied MilkDrop preset")
                            .disabled(!has_preview)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.restore_applied_preset(cx);
                            })),
                        ),
                );
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .gap_3()
                .child(gallery)
                .child(visualizer)
                .into_any_element()
        } else {
            visualizer
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
        let remote_source_snapshot = self.remote_source.as_ref().map(|source| {
            let job = source.job();
            let (phase, detail) = match job.state() {
                RemoteSourceState::Inspecting => (
                    "INSPECTING",
                    "Reading source capabilities without starting playback".to_owned(),
                ),
                RemoteSourceState::Downloading {
                    received_bytes,
                    total_bytes,
                } => (
                    "TRANSFERRING",
                    total_bytes.map_or_else(
                        || format!("{received_bytes} bytes received"),
                        |total| format!("{received_bytes} of {total} bytes received"),
                    ),
                ),
                RemoteSourceState::Preparing => (
                    "PREPARING",
                    "Preparing media for the local playback boundary".to_owned(),
                ),
                RemoteSourceState::Ready => (
                    "READY",
                    source.completed_path().map_or_else(
                        || "Validated local media is ready".to_owned(),
                        |path| {
                            let name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("completed media");
                            format!("Validated local file: {name}")
                        },
                    ),
                ),
                RemoteSourceState::Failed { message } => ("FAILED", message.clone()),
                RemoteSourceState::Cancelled => (
                    "CANCELLED",
                    "yt-dlp stopped without changing the library".to_owned(),
                ),
            };
            (
                job.display_source().to_owned(),
                phase,
                detail,
                job.progress_fraction(),
                job.can_cancel(),
                job.can_retry(),
            )
        });
        let now_playing_card = if let Some(image) = self.cover_image.clone() {
            div()
                .id("now-playing-summary")
                .test_support()
                .role(Role::Group)
                .aria_label(track_accessibility_label.clone())
                .relative()
                .aspect_square()
                .overflow_hidden()
                .rounded_md()
                .border_1()
                .border_color(panel_raised)
                .bg(canvas.opacity(0.68))
                .child(
                    div()
                        .id("album-art")
                        .test_support()
                        .role(Role::Image)
                        .aria_label(format!("Album artwork for {title}"))
                        .absolute()
                        .inset_0()
                        .child(img(image).size_full().object_fit(ObjectFit::Cover)),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .bg(panel.opacity(0.9))
                        .px_3()
                        .py_2()
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(text)
                                .child(title.clone()),
                        )
                        .child(div().text_xs().text_color(text_muted).child(artist.clone()))
                        .child(div().text_xs().text_color(text_muted).child(album.clone())),
                )
                .into_any_element()
        } else {
            div()
                .id("now-playing-summary")
                .test_support()
                .role(Role::Group)
                .aria_label(track_accessibility_label.clone())
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
                        .child(title.clone()),
                )
                .child(div().text_sm().child(artist.clone()))
                .child(div().text_xs().text_color(text_muted).child(album.clone()))
                .into_any_element()
        };
        let TechnicalMetadataDisplay {
            bpm,
            bpm_accessibility,
            container,
            codec,
            sample_rate,
            channels,
            bit_depth,
            duration: technical_duration,
            bitrate,
            accessibility_label,
        } = technical_metadata;
        let audio_details = div()
            .id("audio-details")
            .test_support()
            .role(Role::Group)
            .aria_label(accessibility_label)
            .flex()
            .flex_col()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(meter_high.opacity(0.48))
            .bg(panel_raised)
            .px_3()
            .py_3()
            .child(
                div()
                    .id("bpm-status")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(format!("BPM {bpm_accessibility}"))
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(text_muted)
                            .child("TEMPO"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_1()
                            .text_color(meter_high)
                            .child(div().text_2xl().font_semibold().child(bpm))
                            .child(div().pb_1().text_xs().child("BPM")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(text_muted)
                    .child("FORMAT")
                    .child(
                        div()
                            .text_color(text)
                            .child(format!("{container} / {codec}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(text_muted)
                    .child("SIGNAL")
                    .child(
                        div()
                            .text_color(text)
                            .child(format!("{sample_rate} / {channels}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(text_muted)
                    .child("DEPTH")
                    .child(div().text_color(text).child(bit_depth)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(text_muted)
                    .child("LENGTH")
                    .child(div().text_color(text).child(technical_duration)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(text_muted)
                    .child("BITRATE")
                    .child(div().text_color(text).child(bitrate)),
            );
        let library_visible = self.library_visible;
        let settings_visible = self.settings_visible;
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
        let visible_library_tracks = self
            .library_table
            .as_ref()
            .map_or(0, |table| table.read(cx).delegate().visible_count());
        let library_status = match &self.library_status {
            LibraryStatus::Unloaded => "Local library has not been loaded".to_owned(),
            LibraryStatus::Loading => "Loading local library".to_owned(),
            LibraryStatus::Ready(tracks) if tracks.is_empty() => {
                "No local tracks. Add a music folder to begin.".to_owned()
            }
            LibraryStatus::Ready(tracks) if visible_library_tracks == tracks.len() => {
                format!("{} local tracks", tracks.len())
            }
            LibraryStatus::Ready(tracks) => format!(
                "{visible_library_tracks} of {} local tracks match",
                tracks.len()
            ),
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
        let library_table = self
            .library_table
            .as_ref()
            .expect("library table is initialized before rendering")
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
                    .id("library-search-scope")
                    .test_support()
                    .role(Role::Group)
                    .aria_label("Search field filter")
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(LibrarySearchScope::ALL.into_iter().enumerate().map(
                        |(index, scope)| {
                            let selected = self.library_search_scope == scope;
                            hardware_button(
                                ("library-search-scope", index),
                                if selected {
                                    HardwareButtonTone::Primary
                                } else {
                                    HardwareButtonTone::Neutral
                                },
                                self.skin,
                                cx,
                            )
                            .label(scope.label())
                            .accessibility_label(format!(
                                "Search {} metadata{}",
                                scope.label().to_lowercase(),
                                if selected { ", selected" } else { "" }
                            ))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.set_library_search_scope(scope, cx);
                                },
                            ))
                        },
                    )),
            )
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
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_shrink_0()
                            .w(px(320.0))
                            .min_h_0()
                            .overflow_hidden()
                            .gap_3()
                            .child(playlist_panel)
                            .child(queue_panel),
                    )
                    .child(
                        div()
                            .id("library-track-table")
                            .test_support()
                            .aria_label("Local library tracks")
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                DataTable::new(&library_table)
                                    .accessibility_label("Local library tracks")
                                    .stripe(true)
                                    .bordered(true)
                                    .header_row_height(px(38.0))
                                    .scrollbar_visible(true, true),
                            ),
                    ),
            );
        let apple_playlists = match self.apple_music.session().state() {
            AppleMusicState::LibraryLoaded { playlists } => playlists.clone(),
            _ => Vec::new(),
        };
        let apple_library_status = match self.apple_music.session().state() {
            AppleMusicState::LoadingPlaylists { .. } => "Loading Apple Music playlists".to_owned(),
            AppleMusicState::LibraryEmpty => "No Apple Music playlists found".to_owned(),
            AppleMusicState::LibraryLoaded { playlists } => match playlists.len() {
                1 => "1 Apple Music playlist".to_owned(),
                count => format!("{count} Apple Music playlists"),
            },
            AppleMusicState::Ready => "Apple Music playlists are ready to load".to_owned(),
            _ => "Connect Apple Music to show your playlists".to_owned(),
        };
        let apple_track_search = self
            .apple_track_search
            .as_ref()
            .expect("Apple track search is initialized before rendering")
            .clone();
        let apple_track_query = apple_track_search.read(cx).value().to_string();
        let apple_tracks = match self.apple_music.playlist_tracks() {
            ApplePlaylistTracksState::Loaded { tracks, .. } => tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| apple_track_matches(track, &apple_track_query))
                .map(|(playlist_index, track)| (playlist_index, track.clone()))
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let apple_track_status = match self.apple_music.playlist_tracks() {
            ApplePlaylistTracksState::Idle => "Select a playlist to view its tracks".to_owned(),
            ApplePlaylistTracksState::Loading { .. } => "Loading playlist tracks".to_owned(),
            ApplePlaylistTracksState::Empty { .. } => "This playlist has no tracks".to_owned(),
            ApplePlaylistTracksState::Loaded { tracks, .. } => {
                if apple_track_query.trim().is_empty() {
                    match tracks.len() {
                        1 => "1 track".to_owned(),
                        count => format!("{count} tracks"),
                    }
                } else {
                    format!("{} of {} tracks shown", apple_tracks.len(), tracks.len())
                }
            }
            ApplePlaylistTracksState::Failed { .. } => self.apple_music.track_error().map_or_else(
                || "The selected playlist could not be loaded".to_owned(),
                |error| format!("Playlist error / {error}"),
            ),
        };
        let apple_can_play_playlist = self.apple_music.can_play_loaded_playlist();
        let apple_playback_status = match self.apple_music.playback() {
            ApplePlaybackState::Idle => None,
            ApplePlaybackState::Preparing { .. } => {
                Some("Preparing Apple Music playback".to_owned())
            }
            ApplePlaybackState::Playing { .. } => Some("Playing through Apple Music".to_owned()),
            ApplePlaybackState::Pausing { .. } => Some("Pausing Apple Music playback".to_owned()),
            ApplePlaybackState::Paused { .. } => Some("Apple Music playback paused".to_owned()),
            ApplePlaybackState::Resuming { .. } => Some("Resuming Apple Music playback".to_owned()),
            ApplePlaybackState::Stopping { .. } => Some("Stopping Apple Music playback".to_owned()),
            ApplePlaybackState::Stopped { .. } => Some("Apple Music playback stopped".to_owned()),
            ApplePlaybackState::SkippingNext { .. } => {
                Some("Skipping to next Apple Music track".to_owned())
            }
            ApplePlaybackState::SkippingPrevious { .. } => {
                Some("Skipping to previous Apple Music track".to_owned())
            }
            ApplePlaybackState::Seeking { .. } => Some("Seeking Apple Music playback".to_owned()),
            ApplePlaybackState::SettingRepeat { target_mode, .. } => Some(format!(
                "Setting Apple Music repeat to {}",
                target_mode.name()
            )),
            ApplePlaybackState::SettingShuffle { target_mode, .. } => Some(format!(
                "Setting Apple Music shuffle to {}",
                target_mode.name()
            )),
            ApplePlaybackState::Failed { .. } => {
                Some(self.apple_music.playback_error().map_or_else(
                    || "Apple Music playback could not start".to_owned(),
                    |error| format!("Playback error / {error}"),
                ))
            }
        };
        let apple_playback = self.apple_music.playback().clone();
        let apple_active_occurrence = self
            .apple_music
            .active_playlist_occurrence()
            .map(|(playlist_id, playlist_index)| (playlist_id.clone(), playlist_index));
        let apple_can_pause = matches!(&apple_playback, ApplePlaybackState::Playing { .. });
        let apple_can_resume = matches!(&apple_playback, ApplePlaybackState::Paused { .. });
        let apple_can_stop = matches!(
            &apple_playback,
            ApplePlaybackState::Playing { .. } | ApplePlaybackState::Paused { .. }
        );
        let apple_can_skip_to_previous = self.apple_music.can_skip_to_previous();
        let apple_can_skip_to_next = self.apple_music.can_skip_to_next();
        let apple_can_seek = matches!(&apple_playback, ApplePlaybackState::Playing { .. })
            && self
                .apple_music
                .playback_position()
                .and_then(|position| position.duration_ms())
                .is_some_and(|duration_ms| duration_ms > 0);
        let apple_can_repeat = matches!(
            &apple_playback,
            ApplePlaybackState::Playing { .. } | ApplePlaybackState::Paused { .. }
        );
        let apple_repeat_mode = self.apple_music.repeat_mode();
        let apple_repeat_label =
            format!("REPEAT {}", apple_repeat_mode.name().to_ascii_uppercase());
        let apple_repeat_accessibility_label = format!(
            "Cycle Apple Music repeat mode, currently {}",
            apple_repeat_mode.name()
        );
        let apple_can_shuffle = apple_can_repeat;
        let apple_shuffle_mode = self.apple_music.shuffle_mode();
        let apple_shuffle_label =
            format!("SHUFFLE {}", apple_shuffle_mode.name().to_ascii_uppercase());
        let apple_shuffle_accessibility_label = format!(
            "Toggle Apple Music shuffle, currently {}",
            apple_shuffle_mode.name()
        );
        let apple_playback_progress = self.apple_music.playback_position().map(|position| {
            let elapsed = format_time(position.elapsed_ms() as f64 / 1_000.0);
            let duration = position.duration_ms().map_or_else(
                || "--:--".to_owned(),
                |duration_ms| format_time(duration_ms as f64 / 1_000.0),
            );
            let percent = position.duration_ms().and_then(|duration_ms| {
                (duration_ms > 0).then(|| {
                    (position.elapsed_ms() as f64 / duration_ms as f64 * 100.0).clamp(0.0, 100.0)
                })
            });
            let label = format!("Apple Music playback progress: {elapsed} elapsed of {duration}");
            (elapsed, duration, percent, label)
        });
        let apple_position_error = self
            .apple_music
            .playback_position_error()
            .map(|error| format!("Apple Music playback position unavailable / {error}"));
        let apple_track_count = apple_tracks.len();
        let apple_tracks = Arc::new(apple_tracks);
        let rendered_apple_tracks = Arc::clone(&apple_tracks);
        let apple_track_scroll = self.apple_track_scroll.clone();
        let apple_track_list = uniform_list(
            "apple-playlist-track-results",
            apple_track_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        let (playlist_index, track) = rendered_apple_tracks.get(index)?;
                        let playlist_index = *playlist_index;
                        let album = track.album().unwrap_or("Unknown album");
                        let duration = apple_track_duration(track.duration_ms());
                        let active = apple_active_occurrence.as_ref().is_some_and(
                            |(playlist_id, active_index)| {
                                Some(playlist_id) == this.selected_apple_playlist.as_ref()
                                    && *active_index == playlist_index
                            },
                        );
                        let state = match &apple_playback {
                            ApplePlaybackState::Preparing { .. } if active => ", preparing",
                            ApplePlaybackState::Playing { .. } if active => ", playing",
                            ApplePlaybackState::Pausing { .. } if active => ", pausing",
                            ApplePlaybackState::Paused { .. } if active => ", paused",
                            ApplePlaybackState::Resuming { .. } if active => ", resuming",
                            ApplePlaybackState::Stopping { .. } if active => ", stopping",
                            ApplePlaybackState::Stopped { .. } if active => ", stopped",
                            ApplePlaybackState::SkippingNext { .. } if active => ", skipping next",
                            ApplePlaybackState::SkippingPrevious { .. } if active => {
                                ", skipping previous"
                            }
                            ApplePlaybackState::Seeking { .. } if active => ", seeking",
                            ApplePlaybackState::SettingRepeat { .. } if active => {
                                ", setting repeat"
                            }
                            ApplePlaybackState::SettingShuffle { .. } if active => {
                                ", setting shuffle"
                            }
                            ApplePlaybackState::Failed { .. } if active => ", playback failed",
                            _ => "",
                        };
                        let accessibility_label = format!(
                            "Play track {}: {}, by {}, album {}, duration {}{}",
                            playlist_index + 1,
                            track.title(),
                            track.artist(),
                            album,
                            duration,
                            state
                        );
                        let display_label = format!(
                            "{:02}  {}  /  {}  /  {}  ·  {}",
                            playlist_index + 1,
                            track.title(),
                            track.artist(),
                            album,
                            duration
                        );
                        let track_id = track.id().clone();
                        Some(
                            hardware_button(
                                ("apple-playlist-track", index),
                                if active {
                                    HardwareButtonTone::Primary
                                } else {
                                    HardwareButtonTone::Neutral
                                },
                                this.skin,
                                cx,
                            )
                            .w_full()
                            .label(display_label)
                            .accessibility_label(accessibility_label)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.play_apple_track(track_id.clone(), playlist_index, cx);
                                },
                            )),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .w_full()
        .h(px(220.0))
        .track_scroll(&apple_track_scroll);
        let apple_has_transport_controls = apple_can_seek
            || apple_can_repeat
            || apple_can_skip_to_previous
            || apple_can_skip_to_next
            || apple_can_pause
            || apple_can_resume
            || apple_can_stop;
        let apple_transport_controls = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .when(apple_can_seek, |controls| {
                controls
                    .child(
                        hardware_button(
                            "settings-seek-back-apple-playback",
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .label("-10S APPLE")
                        .accessibility_label("Seek Apple Music back 10 seconds")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.seek_apple_playback_by(-10_000, cx);
                        })),
                    )
                    .child(
                        hardware_button(
                            "settings-seek-forward-apple-playback",
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .label("+10S APPLE")
                        .accessibility_label("Seek Apple Music forward 10 seconds")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.seek_apple_playback_by(10_000, cx);
                        })),
                    )
            })
            .when(apple_can_skip_to_previous, |controls| {
                controls.child(
                    hardware_button(
                        "settings-previous-apple-track",
                        HardwareButtonTone::Neutral,
                        self.skin,
                        cx,
                    )
                    .label("PREVIOUS APPLE")
                    .accessibility_label("Play previous Apple Music track")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.play_previous_apple_track(cx);
                    })),
                )
            })
            .when(apple_can_skip_to_next, |controls| {
                controls.child(
                    hardware_button(
                        "settings-next-apple-track",
                        HardwareButtonTone::Neutral,
                        self.skin,
                        cx,
                    )
                    .label("NEXT APPLE")
                    .accessibility_label("Play next Apple Music track")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.play_next_apple_track(cx);
                    })),
                )
            })
            .when(apple_can_repeat, |controls| {
                controls.child(
                    hardware_button(
                        "settings-cycle-apple-repeat",
                        if apple_repeat_mode == AppleRepeatMode::Off {
                            HardwareButtonTone::Neutral
                        } else {
                            HardwareButtonTone::Primary
                        },
                        self.skin,
                        cx,
                    )
                    .label(apple_repeat_label)
                    .accessibility_label(apple_repeat_accessibility_label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cycle_apple_repeat(cx);
                    })),
                )
            })
            .when(apple_can_shuffle, |controls| {
                controls.child(
                    hardware_button(
                        "settings-toggle-apple-shuffle",
                        if apple_shuffle_mode == AppleShuffleMode::Off {
                            HardwareButtonTone::Neutral
                        } else {
                            HardwareButtonTone::Primary
                        },
                        self.skin,
                        cx,
                    )
                    .label(apple_shuffle_label)
                    .accessibility_label(apple_shuffle_accessibility_label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_apple_shuffle(cx);
                    })),
                )
            })
            .when(apple_can_pause, |controls| {
                controls.child(
                    hardware_button(
                        "settings-pause-apple-playback",
                        HardwareButtonTone::Primary,
                        self.skin,
                        cx,
                    )
                    .label("PAUSE APPLE")
                    .accessibility_label("Pause Apple Music playback")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pause_apple_playback(cx);
                    })),
                )
            })
            .when(apple_can_resume, |controls| {
                controls.child(
                    hardware_button(
                        "settings-resume-apple-playback",
                        HardwareButtonTone::Primary,
                        self.skin,
                        cx,
                    )
                    .label("RESUME APPLE")
                    .accessibility_label("Resume Apple Music playback")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.resume_apple_playback(cx);
                    })),
                )
            })
            .when(apple_can_stop, |controls| {
                controls.child(
                    hardware_button(
                        "settings-stop-apple-playback",
                        HardwareButtonTone::Neutral,
                        self.skin,
                        cx,
                    )
                    .label("STOP APPLE")
                    .accessibility_label("Stop Apple Music playback")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.stop_apple_playback(cx);
                    })),
                )
            });
        let apple_track_panel = div()
            .id("settings-apple-playlist-tracks")
            .test_support()
            .role(Role::List)
            .aria_label("Selected Apple Music playlist tracks")
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(accent)
                            .child("SELECTED PLAYLIST"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                hardware_button(
                                    "settings-play-apple-playlist",
                                    HardwareButtonTone::Primary,
                                    self.skin,
                                    cx,
                                )
                                .label("PLAY PLAYLIST")
                                .accessibility_label("Play selected Apple Music playlist")
                                .disabled(!apple_can_play_playlist)
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.play_selected_apple_playlist(cx);
                                    },
                                )),
                            )
                            .child(
                                hardware_button(
                                    "settings-change-apple-playlist",
                                    HardwareButtonTone::Neutral,
                                    self.skin,
                                    cx,
                                )
                                .label("CHANGE PLAYLIST")
                                .accessibility_label("Return to Apple Music playlists")
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.show_apple_playlists(cx);
                                    },
                                )),
                            ),
                    ),
            )
            .child(
                hardware_input(
                    &apple_track_search,
                    "settings-apple-track-search",
                    self.skin,
                )
                .aria_label("Search selected Apple Music playlist by title, artist, or album"),
            )
            .child(
                div()
                    .id("settings-apple-track-status")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(apple_track_status.clone())
                    .text_xs()
                    .text_color(text_muted)
                    .child(apple_track_status),
            )
            .when_some(apple_playback_status, |panel, status| {
                panel.child(
                    div()
                        .id("settings-apple-playback-status")
                        .test_support()
                        .role(Role::Status)
                        .aria_label(status.clone())
                        .text_sm()
                        .font_semibold()
                        .text_color(accent)
                        .child(status),
                )
            })
            .when_some(
                apple_playback_progress,
                |panel, (elapsed, duration, percent, label)| {
                    panel.child(
                        div()
                            .id("settings-apple-playback-progress")
                            .test_support()
                            .role(Role::ProgressIndicator)
                            .aria_label(label)
                            .aria_min_numeric_value(0.0)
                            .aria_max_numeric_value(100.0)
                            .when_some(percent, |progress, value| {
                                progress.aria_numeric_value(value)
                            })
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(elapsed)
                                    .child(duration),
                            )
                            .child(
                                div()
                                    .h(px(8.0))
                                    .w_full()
                                    .overflow_hidden()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(accent.opacity(0.56))
                                    .bg(canvas.opacity(0.76))
                                    .when_some(percent, |rail, value| {
                                        rail.child(
                                            div()
                                                .h_full()
                                                .w(relative((value / 100.0).clamp(0.0, 1.0) as f32))
                                                .rounded_full()
                                                .bg(accent),
                                        )
                                    }),
                            ),
                    )
                },
            )
            .when_some(apple_position_error, |panel, error| {
                panel.child(
                    div()
                        .id("settings-apple-playback-position-error")
                        .test_support()
                        .role(Role::Status)
                        .aria_label(error.clone())
                        .text_xs()
                        .text_color(text_muted)
                        .child(error),
                )
            })
            .when(apple_has_transport_controls, |panel| {
                panel.child(apple_transport_controls)
            })
            .when(apple_track_count > 0, |panel| panel.child(apple_track_list));
        let selected_apple_playlist = self.selected_apple_playlist.clone();
        let apple_playlist_selected = selected_apple_playlist.is_some();
        let apple_playlist_gallery = div()
            .id("settings-apple-library")
            .test_support()
            .role(Role::Group)
            .aria_label("Apple Music playlist library")
            .flex()
            .flex_col()
            .gap_3()
            .rounded_md()
            .border_1()
            .border_color(self.color(self.skin.control_border()))
            .bg(panel_raised)
            .p_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(accent)
                            .child("YOUR PLAYLISTS"),
                    )
                    .child(
                        div()
                            .id("settings-apple-library-status")
                            .test_support()
                            .role(Role::Status)
                            .aria_label(apple_library_status.clone())
                            .text_sm()
                            .text_color(text)
                            .child(apple_library_status),
                    ),
            )
            .when(
                !apple_playlists.is_empty() && !apple_playlist_selected,
                |gallery| {
                    gallery.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .h(px(280.0))
                            .overflow_y_scrollbar()
                            .gap_3()
                            .children(apple_playlists.into_iter().enumerate().map(
                                |(index, playlist)| {
                                    let id = playlist.id().clone();
                                    let title =
                                        playlist.name().unwrap_or("Untitled playlist").to_owned();
                                    let selected = selected_apple_playlist.as_ref() == Some(&id);
                                    let artwork_label = format!("Artwork for {title}");
                                    let artwork = playlist.artwork_url().map_or_else(
                                        || {
                                            div()
                                                .id(("apple-playlist-artwork", index))
                                                .test_support()
                                                .role(Role::Image)
                                                .aria_label(artwork_label.clone())
                                                .flex()
                                                .w_full()
                                                .h(px(120.0))
                                                .items_center()
                                                .justify_center()
                                                .rounded_md()
                                                .bg(canvas.opacity(0.72))
                                                .text_xs()
                                                .text_color(text_muted)
                                                .child("NO ART")
                                                .into_any_element()
                                        },
                                        |artwork_url| {
                                            div()
                                                .id(("apple-playlist-artwork", index))
                                                .test_support()
                                                .role(Role::Image)
                                                .aria_label(artwork_label.clone())
                                                .w_full()
                                                .h(px(120.0))
                                                .overflow_hidden()
                                                .rounded_md()
                                                .child(
                                                    img(artwork_url.to_owned())
                                                        .size_full()
                                                        .object_fit(ObjectFit::Cover),
                                                )
                                                .into_any_element()
                                        },
                                    );
                                    let accessibility_label = if selected {
                                        format!("Apple Music playlist {title}, selected")
                                    } else {
                                        format!("Select Apple Music playlist {title}")
                                    };

                                    div()
                                        .flex()
                                        .flex_col()
                                        .w(px(176.0))
                                        .gap_2()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(if selected {
                                            accent
                                        } else {
                                            self.color(self.skin.control_border())
                                        })
                                        .bg(canvas.opacity(0.5))
                                        .p_2()
                                        .child(artwork)
                                        .child(
                                            hardware_button(
                                                ("apple-playlist", index),
                                                if selected {
                                                    HardwareButtonTone::Primary
                                                } else {
                                                    HardwareButtonTone::Neutral
                                                },
                                                self.skin,
                                                cx,
                                            )
                                            .label(title)
                                            .accessibility_label(accessibility_label)
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.select_apple_playlist(id.clone(), cx);
                                                }),
                                            ),
                                        )
                                },
                            )),
                    )
                },
            )
            .when(apple_playlist_selected, |gallery| {
                gallery.child(apple_track_panel)
            });
        let settings_panel = div()
            .id("settings-panel")
            .test_support()
            .role(Role::Group)
            .aria_label("Settings and accounts")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .rounded_lg()
            .border_1()
            .border_color(panel_raised)
            .bg(panel)
            .p_5()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child("SETTINGS"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child("ACCOUNTS / SERVICES / PRIVACY"),
                            ),
                    )
                    .child(
                        hardware_button(
                            "settings-back-player",
                            HardwareButtonTone::Neutral,
                            self.skin,
                            cx,
                        )
                        .label("BACK TO PLAYER")
                        .accessibility_label("Show player")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_settings(cx);
                        })),
                    ),
            )
            .child(
                div()
                    .id("settings-apple-account")
                    .test_support()
                    .role(Role::Group)
                    .aria_label("Apple Music account")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_md()
                    .border_1()
                    .border_color(if apple_status_is_error {
                        meter_high
                    } else {
                        self.color(self.skin.control_border())
                    })
                    .bg(panel_raised)
                    .p_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_semibold()
                                            .text_color(accent)
                                            .child("APPLE MUSIC"),
                                    )
                                    .child(
                                        div()
                                            .id("settings-apple-status")
                                            .test_support()
                                            .role(if apple_status_is_error {
                                                Role::Alert
                                            } else {
                                                Role::Status
                                            })
                                            .aria_label(apple_account_status.clone())
                                            .text_sm()
                                            .text_color(text)
                                            .child(apple_account_status),
                                    ),
                            )
                            .child(
                                hardware_button(
                                    "settings-connect-apple-music",
                                    HardwareButtonTone::Primary,
                                    self.skin,
                                    cx,
                                )
                                .key_context("Amp808Control")
                                .label(apple_button_label)
                                .accessibility_label(apple_accessibility_label)
                                .disabled(apple_connection_disabled)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.connect_apple_music(cx);
                                })),
                            ),
                    )
                    .when(!apple_playlist_selected, |account| {
                        account
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(text)
                                    .child(
                                        "Authorization is handled by macOS. AMP808 never asks for or stores Apple Music tokens.",
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(
                                        "READ-ONLY LIBRARY + CATALOG METADATA / APPLE MUSIC PLAYBACK / NO PCM, BPM, EQ OR AUDIO-REACTIVE MILKDROP",
                                    ),
                            )
                    }),
            )
            .child(apple_playlist_gallery);
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
            .on_action(cx.listener(|this, _: &ActivateLibraryTrack, _, cx| {
                this.activate_selected_library_track(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousPreset, _, cx| {
                this.move_preset_selection(PresetSelectionDirection::Previous, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectNextPreset, _, cx| {
                this.move_preset_selection(PresetSelectionDirection::Next, cx);
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
                                    "open-settings",
                                    HardwareButtonTone::Primary,
                                    self.skin,
                                    cx,
                                )
                                .key_context("Amp808Control")
                                .label(if settings_visible { "PLAYER" } else { "SETTINGS" })
                                .accessibility_label(if settings_visible {
                                    "Show player"
                                } else {
                                    "Open settings"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_settings(cx);
                                })),
                            )
                            .child(
                                hardware_button(
                                    "preview-remote-source",
                                    HardwareButtonTone::Neutral,
                                    self.skin,
                                    cx,
                                )
                                .key_context("Amp808Control")
                                .label("USE YT-DLP")
                                .accessibility_label("Use yt-dlp for an audio source")
                                .disabled(self.remote_transfer.is_some())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_remote_source_form(window, cx);
                                })),
                            )
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
            .child(
                div()
                    .id("apple-music-status")
                    .test_support()
                    .role(if apple_status_is_error {
                        Role::Alert
                    } else {
                        Role::Status
                    })
                    .aria_label(apple_status.clone())
                    .rounded_md()
                    .border_1()
                    .border_color(if apple_status_is_error {
                        meter_high
                    } else {
                        self.color(self.skin.control_border())
                    })
                    .bg(panel)
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(if apple_status_is_error { text } else { text_muted })
                    .child(apple_status),
            )
            .when(self.remote_source_form_visible, |root| {
                let input = self
                    .remote_source_input
                    .as_ref()
                    .expect("yt-dlp source input is initialized before rendering")
                    .clone();
                root.child(
                    div()
                        .id("remote-source-form")
                        .test_support()
                        .role(Role::Group)
                        .aria_label("Use yt-dlp with an HTTP or HTTPS audio source")
                        .flex()
                        .items_end()
                        .gap_3()
                        .rounded_md()
                        .border_1()
                        .border_color(meter_high.opacity(0.72))
                        .bg(panel_raised)
                        .px_4()
                        .py_3()
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(meter_high)
                                        .child("YT-DLP SOURCE"),
                                )
                                .child(
                                    hardware_input(&input, "remote-source-url", self.skin)
                                        .aria_label("yt-dlp source URL"),
                                ),
                        )
                        .child(
                            hardware_button(
                                "choose-remote-destination",
                                HardwareButtonTone::Primary,
                                self.skin,
                                cx,
                            )
                            .label("CHOOSE LOCATION")
                            .accessibility_label(
                                "Choose where yt-dlp stores the completed file",
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_remote_destination(cx);
                            })),
                        )
                        .child(
                            hardware_button(
                                "cancel-remote-source-form",
                                HardwareButtonTone::Neutral,
                                self.skin,
                                cx,
                            )
                            .label("CANCEL")
                            .accessibility_label("Cancel yt-dlp source entry")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_remote_source_form(cx);
                            })),
                        ),
                )
            })
            .when_some(self.remote_source_error.clone(), |root, error| {
                root.child(
                    div()
                        .id("remote-source-error")
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(format!("yt-dlp integration error: {error}"))
                        .rounded_md()
                        .border_1()
                        .border_color(accent)
                        .bg(panel_raised)
                        .px_4()
                        .py_3()
                        .text_sm()
                        .text_color(text)
                        .child(error),
                )
            })
            .when_some(remote_source_snapshot, |root, snapshot| {
                let (source, phase, detail, progress, can_cancel, can_retry) = snapshot;
                let progress_percent = progress.map(|value| (value * 100.0).round() as u32);
                let accessible_phase = match phase {
                    "TRANSFERRING" => "transfer",
                    "PREPARING" => "preparation",
                    "INSPECTING" => "inspection",
                    _ => phase,
                };
                let progress_label = progress_percent.map_or_else(
                    || format!("yt-dlp {accessible_phase}: progress is indeterminate"),
                    |percent| format!("yt-dlp transfer: {percent} percent complete"),
                );
                let status_label = progress_percent.map_or_else(
                    || {
                        format!(
                            "yt-dlp integration. {phase}: {source}. {detail}"
                        )
                    },
                    |percent| {
                        format!(
                            "yt-dlp integration. {phase}: {source}. {percent} percent complete. {detail}"
                        )
                    },
                );

                root.child(
                    div()
                        .id("remote-source-status")
                        .test_support()
                        .role(Role::Status)
                        .aria_label(status_label)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .rounded_md()
                        .border_1()
                        .border_color(meter_high.opacity(0.72))
                        .bg(panel_raised)
                        .px_4()
                        .py_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_semibold()
                                                        .text_color(meter_high)
                                                        .child("YT-DLP INTEGRATION"),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(accent)
                                                        .child(phase),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .text_color(text)
                                                .child(source),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(text_muted)
                                                .child(detail),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .when(can_cancel, |actions| {
                                            actions.child(
                                                hardware_button(
                                                    "cancel-remote-source",
                                                    HardwareButtonTone::Neutral,
                                                    self.skin,
                                                    cx,
                                                )
                                                .label("CANCEL")
                                                .accessibility_label("Cancel yt-dlp transfer")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.cancel_remote_transfer(cx);
                                                })),
                                            )
                                        })
                                        .when(can_retry, |actions| {
                                            actions.child(
                                                hardware_button(
                                                    "retry-remote-source",
                                                    HardwareButtonTone::Primary,
                                                    self.skin,
                                                    cx,
                                                )
                                                .label("RETRY")
                                                .accessibility_label("Retry yt-dlp integration")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.retry_remote_transfer(window, cx);
                                                })),
                                            )
                                        })
                                        .when(!can_cancel && !can_retry, |actions| {
                                            actions.child(
                                                hardware_button(
                                                    "dismiss-remote-source",
                                                    HardwareButtonTone::Neutral,
                                                    self.skin,
                                                    cx,
                                                )
                                                .label("DISMISS")
                                                .accessibility_label("Dismiss yt-dlp integration")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.dismiss_remote_transfer(cx);
                                                })),
                                            )
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .id("remote-source-progress")
                                .test_support()
                                .role(Role::ProgressIndicator)
                                .aria_label(progress_label)
                                .aria_min_numeric_value(0.0)
                                .aria_max_numeric_value(100.0)
                                .when_some(progress_percent, |rail, percent| {
                                    rail.aria_numeric_value(f64::from(percent))
                                })
                                .h(px(10.0))
                                .w_full()
                                .overflow_hidden()
                                .rounded_full()
                                .border_1()
                                .border_color(meter_high.opacity(0.56))
                                .bg(canvas.opacity(0.76))
                                .when_some(progress, |rail, value| {
                                    rail.child(
                                        div()
                                            .h_full()
                                            .w(relative(value.clamp(0.0, 1.0)))
                                            .rounded_full()
                                            .bg(meter_high),
                                    )
                                }),
                        ),
                )
            })
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
            .when(settings_visible, |root| root.child(settings_panel))
            .when(library_visible, |root| root.child(library_panel))
            .when(!library_visible && !settings_visible, |root| root.child(
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
                            .child(now_playing_card)
                            .child(audio_details)
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

    use amp808_core::bpm::BpmEstimate;

    use crate::app::transport::TransportState;
    use crate::apple_music::{
        AppleArtworkUrl, AppleMusicId, AppleMusicState, ApplePlaybackState, ApplePlaylist,
        ApplePlaylistTrack, ApplePlaylistTracksState, AppleRepeatMode, AppleShuffleMode,
        CatalogPlayback, CloudLibrary, ConnectionResult, ConnectionSubscription, PlaylistPage,
        SubscriptionCapabilities, SubscriptionOffer, TestConnection, TestPlayback,
        TestPlaylistPage, TestPosition, TestTrackPage, TestTransport, TrackPage,
    };
    use crate::media::{CoverArt, probe_metadata};
    use crate::milkdrop::MilkdropStatus;
    use crate::playback::DecodeWorkerState;
    use crate::remote_transfer::RemoteTransferCoordinator;
    use crate::yt_dlp::{YtDlpError, YtDlpEvent};

    use super::{
        ArtworkCache, ArtworkKey, LibraryCatalogue, LibrarySearchScope, LibrarySortDirection,
        LibrarySortField, LibraryStatus, LibraryTrack, PerformanceProbe, PlaybackToggle,
        PlayerView, PresetPreviewState, QueuedTrack, TechnicalMetadataDisplay, TrackDisplay,
        VisualizerMode, apple_music_poll_requires_render, apple_track_duration,
        apple_track_matches, cover_image_from_art, init_skin_theme, library_fuzzy_score,
        library_metadata_score, milkdrop_status_label, playback_toggle, should_present_milkdrop,
        signal_status, skin_hsla, skin_hsla_with_opacity, sort_library_tracks,
    };

    #[test]
    fn apple_playlist_track_search_is_fuzzy_across_title_artist_and_album() {
        let track = ApplePlaylistTrack::new(
            AppleMusicId::try_from("i.first").expect("fixture id should be valid"),
            "First Light".to_owned(),
            "Signal Artist".to_owned(),
            Some("Cosmic Orbit".to_owned()),
            Some(183_500),
        );

        assert!(apple_track_matches(&track, "frst sig"));
        assert!(apple_track_matches(&track, "orbit"));
        assert!(!apple_track_matches(&track, "terrestrial"));
        assert_eq!(apple_track_duration(track.duration_ms()), "3:03");
        assert_eq!(apple_track_duration(None), "--:--");
    }

    #[test]
    fn apple_music_poll_repaints_each_async_state_boundary() {
        let authorising = AppleMusicState::Authorising {
            request_id: crate::apple_music::RequestId::new(1),
        };
        let loading = AppleMusicState::LoadingPlaylists {
            request_id: crate::apple_music::RequestId::new(2),
        };

        assert!(apple_music_poll_requires_render(
            &authorising,
            None,
            &loading,
            None,
        ));
        assert!(apple_music_poll_requires_render(
            &loading,
            None,
            &AppleMusicState::LibraryEmpty,
            None,
        ));
        assert!(apple_music_poll_requires_render(
            &AppleMusicState::Ready,
            None,
            &AppleMusicState::Ready,
            Some("playlist request failed"),
        ));
        assert!(!apple_music_poll_requires_render(
            &loading, None, &loading, None,
        ));
    }

    #[gpui_kit::test]
    fn apple_music_connection_is_visible_accessible_and_user_initiated(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, _| {
                view.apple_music
                    .use_test_connection(TestConnection::Pending);
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(view.read(cx).apple_music.test_connection_launches(), 0);
            assert_eq!(window.find("open-settings").label(), Some("Open settings"));

            window.click("open-settings", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-connect-apple-music").label(),
                Some("Connect Apple Music")
            );

            window.click("settings-connect-apple-music", cx);
            window.render_frame(cx);
        })
        .expect("test window remains available");

        cx.update(|cx| {
            assert_eq!(view.read(cx).apple_music.test_connection_launches(), 1);
            assert!(matches!(
                view.read(cx).apple_music.session().state(),
                AppleMusicState::Authorising { .. }
            ));
        });
    }

    #[gpui_kit::test]
    fn settings_accounts_view_owns_apple_music_sign_in(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, _| {
                view.apple_music
                    .use_test_connection(TestConnection::Pending);
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("open-settings").label(), Some("Open settings"));
            assert_eq!(view.read(cx).apple_music.test_connection_launches(), 0);

            window.click("open-settings", cx);
            window.render_frame(cx);

            assert_eq!(
                window.find("settings-panel").label(),
                Some("Settings and accounts")
            );
            assert_eq!(
                window.find("settings-apple-status").label(),
                Some("Apple Music account is not connected")
            );
            assert_eq!(
                window.find("settings-connect-apple-music").label(),
                Some("Connect Apple Music")
            );

            window.click("settings-connect-apple-music", cx);
            window.render_frame(cx);
        })
        .expect("test window remains available");

        cx.update(|cx| {
            assert_eq!(view.read(cx).apple_music.test_connection_launches(), 1);
            assert!(matches!(
                view.read(cx).apple_music.session().state(),
                AppleMusicState::Authorising { .. }
            ));
        });
    }

    #[gpui_kit::test]
    fn connected_settings_view_presents_a_selectable_apple_playlist_gallery(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        let capabilities = SubscriptionCapabilities::new(
            CatalogPlayback::Available,
            SubscriptionOffer::Unavailable,
            CloudLibrary::Enabled,
        );
        let artwork = AppleArtworkUrl::try_from("https://example.apple/cosmic-orbit/300x300.jpg")
            .expect("fixture artwork should be valid");
        cx.update(|cx| {
            view.update(cx, |view, _| {
                view.apple_music
                    .use_test_connection(TestConnection::Completed(ConnectionResult::Authorised(
                        ConnectionSubscription::Available(capabilities),
                    )));
                view.apple_music
                    .use_test_playlist_pages(vec![TestPlaylistPage::Completed(PlaylistPage::new(
                        vec![
                            ApplePlaylist::with_artwork(
                                AppleMusicId::try_from("p.cosmic")
                                    .expect("fixture id should be valid"),
                                Some("Cosmic Orbit".to_owned()),
                                Some(artwork),
                            ),
                            ApplePlaylist::new(
                                AppleMusicId::try_from("p.signal")
                                    .expect("fixture id should be valid"),
                                Some("Signal Drift".to_owned()),
                            ),
                        ],
                        None,
                    ))]);
                view.apple_music
                    .use_test_track_pages(vec![TestTrackPage::Completed(TrackPage::new(
                        AppleMusicId::try_from("p.signal")
                            .expect("fixture playlist id should be valid"),
                        vec![
                            ApplePlaylistTrack::new(
                                AppleMusicId::try_from("i.first-light")
                                    .expect("fixture track id should be valid"),
                                "First Light".to_owned(),
                                "Signal Artist".to_owned(),
                                None,
                                Some(183_000),
                            ),
                            ApplePlaylistTrack::new(
                                AppleMusicId::try_from("i.second-sun")
                                    .expect("fixture track id should be valid"),
                                "Second Sun".to_owned(),
                                "Signal Artist".to_owned(),
                                None,
                                Some(245_000),
                            ),
                            ApplePlaylistTrack::new(
                                AppleMusicId::try_from("i.third-orbit")
                                    .expect("fixture track id should be valid"),
                                "Third Orbit".to_owned(),
                                "Signal Artist".to_owned(),
                                None,
                                Some(203_000),
                            ),
                        ],
                        None,
                    ))]);
                view.apple_music.use_test_playback(TestPlayback::Completed);
                view.apple_music.use_test_transport(TestTransport::Paused);
                view.apple_music
                    .use_test_positions(vec![TestPosition::Completed { elapsed_ms: 12_345 }]);
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("open-settings", cx);
            window.render_frame(cx);
            window.click("settings-connect-apple-music", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(view.poll_apple_music_connection(cx));
                assert!(!view.poll_apple_music_connection(cx));
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-apple-library-status").label(),
                Some("2 Apple Music playlists")
            );
            assert_eq!(
                window.find(("apple-playlist-artwork", 0_usize)).label(),
                Some("Artwork for Cosmic Orbit")
            );
            assert_eq!(
                window.find(("apple-playlist", 1_usize)).label(),
                Some("Select Apple Music playlist Signal Drift")
            );

            window.click(("apple-playlist", 1_usize), cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-change-apple-playlist").label(),
                Some("Return to Apple Music playlists")
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playlist_tracks(),
                    ApplePlaylistTracksState::Loaded { tracks, .. } if tracks.len() == 3
                ));
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-apple-track-status").label(),
                Some("3 tracks")
            );
            assert_eq!(
                window.find("settings-play-apple-playlist").label(),
                Some("Play selected Apple Music playlist")
            );
            assert_eq!(
                window.find(("apple-playlist-track", 1_usize)).label(),
                Some(
                    "Play track 2: Second Sun, by Signal Artist, album Unknown album, duration 4:05"
                )
            );
            assert_eq!(
                window.find(("apple-playlist-track", 1_usize)).role(),
                Some(Role::Button)
            );
            window.click(("apple-playlist-track", 1_usize), cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Preparing { .. }
                ));
                assert!(view.poll_apple_music_connection(cx));
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { .. }
                ));
                view.apple_music
                    .use_test_transport(TestTransport::RepeatPlaying {
                        mode: AppleRepeatMode::All,
                    });
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-apple-playback-status").label(),
                Some("Playing through Apple Music")
            );
            let progress = window.find("settings-apple-playback-progress");
            assert_eq!(progress.role(), Some(Role::ProgressIndicator));
            assert_eq!(
                progress.label(),
                Some("Apple Music playback progress: 00:12 elapsed of 04:05")
            );
            assert_eq!(
                window.find(("apple-playlist-track", 1_usize)).label(),
                Some(
                    "Play track 2: Second Sun, by Signal Artist, album Unknown album, duration 4:05, playing"
                )
            );
            assert_eq!(
                window.find("settings-previous-apple-track").label(),
                Some("Play previous Apple Music track")
            );
            assert_eq!(
                window.find("settings-next-apple-track").label(),
                Some("Play next Apple Music track")
            );
            assert_eq!(
                window.find("settings-seek-back-apple-playback").label(),
                Some("Seek Apple Music back 10 seconds")
            );
            assert_eq!(
                window.find("settings-seek-forward-apple-playback").label(),
                Some("Seek Apple Music forward 10 seconds")
            );
            assert_eq!(
                window.find("settings-cycle-apple-repeat").label(),
                Some("Cycle Apple Music repeat mode, currently off")
            );
            assert_eq!(
                window.find("settings-toggle-apple-shuffle").label(),
                Some("Toggle Apple Music shuffle, currently off")
            );
            window.click("settings-cycle-apple-repeat", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::SettingRepeat {
                        target_mode: AppleRepeatMode::All,
                        ..
                    }
                ));
                assert!(!view.poll_apple_music_connection(cx));
                assert_eq!(view.apple_music.repeat_mode(), AppleRepeatMode::All);
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { .. }
                ));
                view.apple_music
                    .use_test_transport(TestTransport::ShufflePlaying {
                        mode: AppleShuffleMode::Songs,
                    });
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-cycle-apple-repeat").label(),
                Some("Cycle Apple Music repeat mode, currently all")
            );
            window.click("settings-toggle-apple-shuffle", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::SettingShuffle {
                        target_mode: AppleShuffleMode::Songs,
                        ..
                    }
                ));
                assert!(!view.poll_apple_music_connection(cx));
                assert_eq!(view.apple_music.shuffle_mode(), AppleShuffleMode::Songs);
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { .. }
                ));
                view.apple_music.use_test_transport(TestTransport::Playing);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-toggle-apple-shuffle").label(),
                Some("Toggle Apple Music shuffle, currently songs")
            );
            window.click("settings-seek-forward-apple-playback", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Seeking {
                        target_ms: 22_345,
                        ..
                    }
                ));
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { .. }
                ));
                view.apple_music
                    .use_test_positions(vec![TestPosition::CompletedForOccurrence {
                        playlist_id: AppleMusicId::try_from("p.signal")
                            .expect("fixture playlist id should be valid"),
                        playlist_index: 2,
                        elapsed_ms: 1_000,
                    }]);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("settings-next-apple-track", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::SkippingNext { .. }
                ));
                assert!(view.poll_apple_music_connection(cx));
                assert_eq!(
                    view.apple_music.playback(),
                    &ApplePlaybackState::Playing {
                        track_id: AppleMusicId::try_from("i.second-sun")
                            .expect("fixture track id should be valid"),
                    }
                );
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { track_id }
                        if track_id.as_str() == "i.third-orbit"
                ));
                view.apple_music
                    .use_test_positions(vec![TestPosition::CompletedForOccurrence {
                        playlist_id: AppleMusicId::try_from("p.signal")
                            .expect("fixture playlist id should be valid"),
                        playlist_index: 1,
                        elapsed_ms: 500,
                    }]);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(("apple-playlist-track", 2_usize)).label(),
                Some(
                    "Play track 3: Third Orbit, by Signal Artist, album Unknown album, duration 3:23, playing"
                )
            );
            window.click("settings-previous-apple-track", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::SkippingPrevious { .. }
                ));
                assert!(view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { track_id }
                        if track_id.as_str() == "i.third-orbit"
                ));
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { track_id }
                        if track_id.as_str() == "i.second-sun"
                ));
                view.apple_music.use_test_transport(TestTransport::Paused);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-pause-apple-playback").label(),
                Some("Pause Apple Music playback")
            );
            window.click("settings-pause-apple-playback", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Pausing { .. }
                ));
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Paused { .. }
                ));
                view.apple_music.use_test_transport(TestTransport::Playing);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-apple-playback-status").label(),
                Some("Apple Music playback paused")
            );
            assert_eq!(
                window.find("settings-resume-apple-playback").label(),
                Some("Resume Apple Music playback")
            );
            assert_eq!(
                window.find("settings-stop-apple-playback").label(),
                Some("Stop Apple Music playback")
            );
            window.click("settings-resume-apple-playback", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Resuming { .. }
                ));
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Playing { .. }
                ));
                view.apple_music.use_test_transport(TestTransport::Stopped);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-apple-playback-status").label(),
                Some("Playing through Apple Music")
            );
            assert_eq!(
                window.find("settings-stop-apple-playback").label(),
                Some("Stop Apple Music playback")
            );
            window.click("settings-stop-apple-playback", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Stopping { .. }
                ));
                assert!(!view.poll_apple_music_connection(cx));
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Stopped { .. }
                ));
                view.apple_music.use_test_playback(TestPlayback::Pending);
            });
        });
        cx.update_window(window, |_, window, cx| {
            let search = view
                .read(cx)
                .apple_track_search
                .as_ref()
                .expect("Apple track search is initialized")
                .clone();
            search.update(cx, |input, cx| input.set_value("third", window, cx));
        })
        .expect("test window remains available");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-apple-playback-status").label(),
                Some("Apple Music playback stopped")
            );
            assert_eq!(
                window.find("settings-apple-track-status").label(),
                Some("1 of 3 tracks shown")
            );
            window.click("settings-play-apple-playlist", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, _| {
                assert!(matches!(
                    view.apple_music.playback(),
                    ApplePlaybackState::Preparing { track_id, .. }
                        if track_id.as_str() == "i.first-light"
                ));
                assert!(matches!(
                    view.apple_music.test_playback_request(),
                    Some((playlist_id, track_id, 0))
                        if playlist_id.as_str() == "p.signal"
                            && track_id.as_str() == "i.first-light"
                ));
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("settings-change-apple-playlist", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find(("apple-playlist", 1_usize)).label(),
                Some("Select Apple Music playlist Signal Drift")
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn denied_apple_music_connection_is_visible_and_retryable(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, _| {
                view.apple_music
                    .use_test_connection(TestConnection::Completed(ConnectionResult::Denied));
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("open-settings", cx);
            window.render_frame(cx);
            window.click("settings-connect-apple-music", cx);
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                assert!(!view.poll_apple_music_connection(cx));
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("apple-music-status").label(),
                Some("APPLE MUSIC / ACCESS DENIED")
            );
            assert_eq!(
                window.find("settings-connect-apple-music").label(),
                Some("Retry Apple Music connection after access was denied")
            );
        })
        .expect("test window remains available");
    }

    #[test]
    fn technical_metadata_display_uses_decoder_facts_and_honest_fallbacks() {
        let directory = tempdir().expect("temporary media directory is created");
        let path = directory.path().join("sidebar.wav");
        write_stereo_wav(&path, 8_000, 8_000);
        let metadata = probe_metadata(&path).expect("WAV metadata is readable");

        let display = TechnicalMetadataDisplay::new(Some(&metadata), BpmEstimate::Locked(128));

        assert_eq!(display.bpm(), "128");
        assert_eq!(display.codec(), "PCM S16LE");
        assert_eq!(display.container(), "WAVE");
        assert_eq!(display.bit_depth(), "16 BIT");
        assert_eq!(display.bitrate(), "--");
        assert_eq!(
            display.accessibility_label(),
            "Audio details. BPM 128. WAVE container. PCM S16LE codec. 8000 hertz. 2 channels. 16 bit. Duration 00:01. Bitrate unavailable."
        );
    }

    #[test]
    fn library_fuzzy_search_matches_ordered_non_contiguous_characters() {
        assert!(library_fuzzy_score("csmpltn", "Cosmopolitan").is_some());
        assert_eq!(library_fuzzy_score("cmz", "Cosmopolitan"), None);
    }

    #[test]
    fn library_fuzzy_search_respects_field_scope_and_multiple_terms() {
        assert!(
            library_metadata_score(
                "blue intl",
                LibrarySearchScope::All,
                "Before My Head",
                Some("Bluelight Isms"),
                Some("International EP"),
            )
            .is_some()
        );
        assert_eq!(
            library_metadata_score(
                "international",
                LibrarySearchScope::Artist,
                "Before My Head",
                Some("Bluelight Isms"),
                Some("International EP"),
            ),
            None
        );
    }

    #[test]
    fn library_table_title_sort_is_case_insensitive_and_reversible() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Zulu.wav"), 8_000, 8_000);
        write_stereo_wav(&music.join("alpha.wav"), 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music fixture scans");
        let mut tracks = catalogue.search("").expect("library tracks load");

        sort_library_tracks(
            &mut tracks,
            LibrarySortField::Title,
            LibrarySortDirection::Ascending,
        );
        assert_eq!(
            tracks.iter().map(LibraryTrack::title).collect::<Vec<_>>(),
            ["alpha", "Zulu"]
        );

        sort_library_tracks(
            &mut tracks,
            LibrarySortField::Title,
            LibrarySortDirection::Descending,
        );
        assert_eq!(
            tracks.iter().map(LibraryTrack::title).collect::<Vec<_>>(),
            ["Zulu", "alpha"]
        );
    }

    #[gpui_kit::test]
    fn library_table_fuzzy_query_and_field_filter_update_visible_rows(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Cosmopolitan.wav"), 8_000, 8_000);
        write_stereo_wav(&music.join("First Light.wav"), 8_000, 8_000);
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
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .expect("test window remains available");

        cx.update_window(window, |_, window, cx| {
            let search = view
                .read(cx)
                .library_search
                .as_ref()
                .expect("library search is initialized")
                .clone();
            search.update(cx, |input, cx| input.set_value("csmpltn", window, cx));
        })
        .expect("test window remains available");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("library-status").label(),
                Some("1 of 2 local tracks match")
            );
            window.click(("library-search-scope", 2_usize), cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("library-status").label(),
                Some("0 of 2 local tracks match")
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn library_table_header_cycles_sort_direction(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Zulu.wav"), 8_000, 8_000);
        write_stereo_wav(&music.join("alpha.wav"), 8_000, 8_000);
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
            let table = view
                .read(cx)
                .library_table
                .as_ref()
                .expect("library table is initialized")
                .clone();
            assert_eq!(table.read(cx).delegate().track(0).unwrap().title(), "alpha");
            window.click(("icon-sort", 1_usize), cx);
            window.render_frame(cx);
            assert_eq!(table.read(cx).delegate().track(0).unwrap().title(), "Zulu");
        })
        .expect("test window remains available");
    }

    #[test]
    fn library_artwork_cache_requests_each_file_fingerprint_once() {
        let key = ArtworkKey::for_test("/music/first-light.mp3", 42, 7);
        let mut cache = ArtworkCache::<u8>::new(4);

        assert_eq!(cache.request([key.clone()], 4), vec![key.clone()]);
        assert!(cache.request([key.clone()], 4).is_empty());

        cache.resolve(key.clone(), None);

        assert!(cache.request([key.clone()], 4).is_empty());
        assert_eq!(cache.value(&key), None);
    }

    #[test]
    fn library_artwork_cache_reprobes_a_changed_file_fingerprint() {
        let original = ArtworkKey::for_test("/music/first-light.mp3", 42, 7);
        let changed = ArtworkKey::for_test("/music/first-light.mp3", 43, 8);
        let mut cache = ArtworkCache::<u8>::new(4);

        assert_eq!(cache.request([original.clone()], 4), vec![original.clone()]);
        cache.resolve(original, Some(9));

        assert_eq!(cache.request([changed.clone()], 4), vec![changed]);
    }

    #[test]
    fn library_artwork_cache_never_exceeds_its_entry_limit() {
        let mut cache = ArtworkCache::<u8>::new(2);
        let first = ArtworkKey::for_test("/music/one.mp3", 1, 1);
        let second = ArtworkKey::for_test("/music/two.mp3", 2, 2);
        let third = ArtworkKey::for_test("/music/three.mp3", 3, 3);

        assert_eq!(cache.request([first.clone(), second.clone()], 2).len(), 2);
        cache.resolve(first.clone(), Some(1));
        cache.resolve(second.clone(), None);
        assert_eq!(cache.request([third.clone()], 1), vec![third.clone()]);

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.value(&first), None);
        assert_eq!(cache.value(&third), None);
    }

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
            assert_eq!(
                theme.button,
                skin_hsla_with_opacity(skin.control(), skin.control_opacity())
            );
            assert_eq!(theme.tokens.button.background, theme.button.into());
            assert_eq!(
                theme.button_hover,
                skin_hsla_with_opacity(skin.control_hover(), skin.control_hover_opacity())
            );
            assert_eq!(
                theme.button_active,
                skin_hsla_with_opacity(skin.control_active(), skin.control_active_opacity())
            );
            assert_eq!(theme.button_foreground, skin_hsla(skin.text()));
            assert_eq!(
                theme.button_primary,
                skin_hsla_with_opacity(skin.control_primary(), skin.control_primary_opacity())
            );
            assert_eq!(
                theme.button_primary_foreground,
                skin_hsla(skin.control_primary_foreground())
            );
            assert_eq!(
                theme.tokens.button_primary.background,
                theme.button_primary.into()
            );
            assert_eq!(theme.table, skin_hsla(skin.panel()));
            assert_eq!(theme.table_even, skin_hsla(skin.panel_raised()));
            assert_eq!(theme.table_head, skin_hsla(skin.field()));
            assert_eq!(theme.table_head_foreground, skin_hsla(skin.text()));
            assert_eq!(theme.table_row_border, skin_hsla(skin.control_border()));
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
    fn preset_preview_is_reversible_until_the_user_promotes_it() {
        let directory = tempdir().expect("temporary preset directory is created");
        let candidate_path = directory.path().join("Cosmic Bloom.milk");
        fs::write(&candidate_path, "[preset00]\nfRating=5.0\n")
            .expect("candidate preset is written");
        let candidate =
            crate::milkdrop::MilkdropPreset::read(&candidate_path).expect("candidate preset loads");
        let applied = crate::milkdrop::MilkdropPreset::first_light();
        let mut state = PresetPreviewState::new(applied);

        state.preview_succeeded(candidate.clone());
        assert_eq!(
            state
                .restore_target()
                .expect("active preview has a restore target")
                .name(),
            "First Light"
        );

        state.use_preview();
        assert_eq!(state.applied().name(), "Cosmic Bloom.milk");
        assert_eq!(state.restore_target(), None);

        state.preview_succeeded(crate::milkdrop::MilkdropPreset::first_light());
        state.restore_succeeded();
        assert_eq!(state.applied().name(), "Cosmic Bloom.milk");
        assert_eq!(state.restore_target(), None);
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

    #[test]
    fn embedded_cover_art_maps_supported_mime_types_to_gpui_images() {
        let jpeg = CoverArt::for_test(vec![1, 2, 3], "image/jpg");
        let unsupported = CoverArt::for_test(vec![4, 5, 6], "application/octet-stream");

        let image = cover_image_from_art(Some(&jpeg)).expect("jpeg artwork is supported");

        assert_eq!(image.format(), gpui_kit::ImageFormat::Jpeg);
        assert!(cover_image_from_art(Some(&unsupported)).is_none());
        assert!(cover_image_from_art(None).is_none());
    }

    #[gpui_kit::test]
    fn embedded_cover_art_is_exposed_as_an_accessible_now_playing_image(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                let art = CoverArt::for_test(vec![1, 2, 3], "image/png");
                view.cover_image = cover_image_from_art(Some(&art));
                cx.notify();
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let artwork = window.find("album-art");
            assert_eq!(artwork.role(), Some(Role::Image));
            assert_eq!(artwork.label(), Some("Album artwork for NO TRACK LOADED"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn cached_artwork_is_exposed_as_an_accessible_library_thumbnail(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Signal Bloom.wav"), 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music fixture scans");
        let tracks = catalogue.search("").expect("library tracks load");
        let key = ArtworkKey::from(&tracks[0]);

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
                assert_eq!(
                    view.library_artwork.request([key.clone()], 1),
                    vec![key.clone()]
                );
                let art = CoverArt::for_test(vec![1, 2, 3], "image/png");
                view.library_artwork
                    .resolve(key, cover_image_from_art(Some(&art)));
                cx.notify();
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let table = view
                .read(cx)
                .library_table
                .as_ref()
                .expect("library table is initialized");
            assert_eq!(table.read(cx).delegate().visible_count(), 1);
            assert_eq!(table.read(cx).visible_range().rows(), &(0..1));
            let artwork = window.find(("library-artwork", 0_usize));
            assert_eq!(artwork.role(), Some(Role::Image));
            assert_eq!(artwork.label(), Some("Album artwork for Signal Bloom"));
            assert_eq!(
                window.find(("library-track", 0_usize)).label(),
                Some("Play Signal Bloom  —  Unknown artist  /  Unknown album. 8000 hertz, 2 channels, duration 00:01")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn library_artwork_is_probed_in_background_and_missing_art_is_cached(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_tagged_stereo_wav(&music.join("Artwork Signal.wav"), 8_000, 8_000);
        write_stereo_wav(&music.join("Plain Signal.wav"), 8_000, 8_000);
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
                view.library_status = LibraryStatus::Ready(tracks.clone());
                view.schedule_library_artwork(&tracks, cx);
                cx.notify();
            });
        });
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(("library-artwork", 0_usize)).label(),
                Some("Album artwork for Artwork Signal")
            );
            assert!(
                window
                    .find(("library-artwork-placeholder", 1_usize))
                    .visible()
            );
        })
        .unwrap();
        cx.update(|cx| {
            view.update(cx, |view, _| {
                assert!(
                    view.library_artwork
                        .request(tracks.iter().map(ArtworkKey::from), 2)
                        .is_empty(),
                    "ready and missing artwork entries stay cached"
                );
            });
        });
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
    fn sidebar_audio_details_expose_honest_unavailable_states(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .expect("test window opens")
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let details = window.find("audio-details");
            assert_eq!(details.role(), Some(Role::Group));
            assert_eq!(
                details.label(),
                Some("Audio details. BPM unavailable. Container unavailable. Codec unavailable. Sample rate unavailable. Channels unavailable. Bit depth unavailable. Duration unavailable. Bitrate unavailable.")
            );
            let bpm = window.find("bpm-status");
            assert_eq!(bpm.role(), Some(Role::Status));
            assert_eq!(bpm.label(), Some("BPM unavailable"));
        })
        .expect("window remains available");
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
    fn yt_dlp_action_opens_an_accessible_source_form(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("preview-remote-source", cx);
            window.render_frame(cx);

            assert_eq!(window.find("remote-source-form").role(), Some(Role::Group));
            assert_eq!(
                window.find("remote-source-form").label(),
                Some("Use yt-dlp with an HTTP or HTTPS audio source")
            );
            assert_eq!(
                window.find("remote-source-url").label(),
                Some("yt-dlp source URL")
            );
            assert_eq!(
                window.find("choose-remote-destination").label(),
                Some("Choose where yt-dlp stores the completed file")
            );
            assert_eq!(
                window.find("cancel-remote-source-form").label(),
                Some("Cancel yt-dlp source entry")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn yt_dlp_form_requests_one_destination_directory(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("preview-remote-source", cx);
            window.render_frame(cx);
            window.click("choose-remote-destination", cx);
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
    fn yt_dlp_form_rejects_option_shaped_input_before_process_start(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let destination = tempfile::tempdir().expect("destination directory is created");
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("preview-remote-source", cx);
            let input = view
                .read(cx)
                .remote_source_input
                .as_ref()
                .expect("source input is initialized")
                .clone();
            input.update(cx, |input, cx| {
                input.set_value("--exec=touch /tmp/amp808-nope", window, cx);
            });
            window.render_frame(cx);
            window.click("choose-remote-destination", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(|_| Some(vec![destination.path().to_path_buf()]));
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window
                    .find("remote-source-error")
                    .label()
                    .is_some_and(|label| label.contains("HTTP(S) URL"))
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn remote_transfer_exposes_determinate_progress_and_cancel(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });
        cx.update(|cx| {
            view.update(cx, |view, _| {
                let mut source = RemoteTransferCoordinator::new("yt-dlp source");
                source
                    .apply(YtDlpEvent::Progress {
                        transferred_bytes: 420,
                        total_bytes: Some(1_000),
                    })
                    .unwrap();
                view.remote_source = Some(source);
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("preview-remote-source").label(),
                Some("Use yt-dlp for an audio source")
            );

            let progress = window.find("remote-source-progress");
            assert_eq!(progress.role(), Some(Role::ProgressIndicator));
            assert_eq!(
                progress.label(),
                Some("yt-dlp transfer: 42 percent complete")
            );
            assert!(
                window
                    .find("remote-source-status")
                    .label()
                    .is_some_and(|label| {
                        !label.to_ascii_lowercase().contains("download")
                            && !label.to_ascii_lowercase().contains("preview")
                    })
            );
            assert_eq!(
                window.find("cancel-remote-source").label(),
                Some("Cancel yt-dlp transfer")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn confirmed_remote_cancellation_exposes_status_and_dismiss(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });
        cx.update(|cx| {
            view.update(cx, |view, _| {
                let mut source = RemoteTransferCoordinator::new("yt-dlp source");
                source.apply(YtDlpEvent::Cancelled).unwrap();
                view.remote_source = Some(source);
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);

            assert!(
                window
                    .find("remote-source-status")
                    .label()
                    .is_some_and(|label| label.contains("CANCELLED"))
            );
            assert_eq!(
                window.find("dismiss-remote-source").label(),
                Some("Dismiss yt-dlp integration")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn failed_remote_transfer_exposes_retry_form(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });
        cx.update(|cx| {
            view.update(cx, |view, _| {
                let mut source = RemoteTransferCoordinator::new("yt-dlp source");
                source
                    .apply(YtDlpEvent::Failed(YtDlpError::MissingOutput))
                    .unwrap();
                view.remote_source = Some(source);
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("retry-remote-source").label(),
                Some("Retry yt-dlp integration")
            );
            window.click("retry-remote-source", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("remote-source-form").label(),
                Some("Use yt-dlp with an HTTP or HTTPS audio source")
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
                Some("No local tracks. Add a music folder to begin.")
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
        let mut table_reached = false;
        for _ in 0..32 {
            cx.update_window(window, |_, window, cx| window.press("tab", cx))
                .expect("test window remains available");
            cx.run_until_parked();
            table_reached = cx
                .update_window(window, |_, window, _| {
                    window.find("table").focused() == Some(true)
                })
                .expect("test window remains available");
            if table_reached {
                break;
            }
        }
        assert!(
            table_reached,
            "library table is reachable with Tab navigation"
        );

        cx.update_window(window, |_, window, cx| {
            let table = window.find("table");
            assert_eq!(table.focused(), Some(true));
            assert_eq!(table.role(), Some(Role::Grid));
            assert_eq!(table.label(), Some("Local library tracks"));
            window.press("down", cx);
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
    fn milkdrop_mode_exposes_an_accessible_virtual_preset_gallery(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
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
                |_, cx| cx.new(PlayerView::new),
            )
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
            assert_eq!(window.find("preset-gallery").role(), Some(Role::Group));
            assert_eq!(
                window.find("preset-gallery").label(),
                Some("MilkDrop preset gallery")
            );
            let first_row = window.find(("preset-gallery-row", 0_usize));
            assert_eq!(first_row.label(), Some("Select First Light, AMP808 preset"));
            assert!(first_row.visible(), "the first preset row is visible");
            assert!(
                first_row.bounds().size.width > px(100.0),
                "the first preset row has usable visual width: {:?}",
                first_row.bounds()
            );
            assert_eq!(
                window.find("add-preset-folder").label(),
                Some("Add a local MilkDrop preset folder")
            );
            assert_eq!(
                window.find("preview-preset").label(),
                Some("Preview First Light")
            );
            assert_eq!(window.find("preview-preset").disabled(), Some(true));
            assert_eq!(window.find("use-previewed-preset").disabled(), Some(true));
            assert_eq!(window.find("restore-applied-preset").disabled(), Some(true));
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn preset_gallery_uses_one_tab_stop_and_arrow_selection(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary preset directory is created");
        fs::create_dir(directory.path().join("Fractal")).expect("fractal category is created");
        fs::write(
            directory.path().join("Fractal/Cosmic Bloom.milk"),
            "[preset00]\n",
        )
        .expect("local preset is written");
        fs::create_dir(directory.path().join("Particles")).expect("particle category is created");
        fs::write(
            directory.path().join("Particles/Nebula Tide.milk"),
            "[preset00]\n",
        )
        .expect("second local preset is written");
        let local_presets = crate::preset_catalog::PresetCatalog::discover_local(directory.path())
            .expect("local presets are discovered");

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
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.visualizer_mode = VisualizerMode::MilkDrop;
                view.preset_catalog.replace_local(local_presets);
                view.refresh_preset_gallery(cx);
            });
        });

        let mut first_row_reached = false;
        for _ in 0..24 {
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                if window.find(("preset-gallery-row", 0_usize)).focused() == Some(true) {
                    first_row_reached = true;
                } else {
                    window.press("tab", cx);
                }
            })
            .expect("test window remains available");
            cx.run_until_parked();
            if first_row_reached {
                break;
            }
        }
        assert!(first_row_reached, "the virtual gallery is reachable by Tab");

        let (first_id, second_id) = cx.update(|cx| {
            let view = view.read(cx);
            let first_entry_index = *view
                .preset_visible_indices
                .first()
                .expect("the gallery contains a first preset");
            let entry_index = *view
                .preset_visible_indices
                .get(1)
                .expect("the gallery contains a second preset");
            (
                view.preset_catalog.entries()[first_entry_index]
                    .id()
                    .to_owned(),
                view.preset_catalog.entries()[entry_index].id().to_owned(),
            )
        });
        cx.update_window(window, |_, window, cx| window.press("down", cx))
            .expect("test window remains available");
        cx.run_until_parked();

        cx.update(|cx| {
            assert_eq!(
                view.read(cx).preset_selected_id.as_deref(),
                Some(second_id.as_str()),
                "Down selects the next visible preset without previewing it"
            );
        });
        cx.update_window(window, |_, window, cx| window.press("up", cx))
            .expect("test window remains available");
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                view.read(cx).preset_selected_id.as_deref(),
                Some(first_id.as_str()),
                "Up selects the previous visible preset without previewing it"
            );
        });
        cx.update_window(window, |_, window, cx| window.press("down", cx))
            .expect("test window remains available");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(("preset-gallery-row", 1_usize)).focused(),
                Some(true),
                "focus follows the selected virtual row"
            );
            window.press("tab", cx);
        })
        .expect("test window remains available");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let focus_states = [
                ("row 0", window.find(("preset-gallery-row", 0_usize)).focused()),
                ("row 1", window.find(("preset-gallery-row", 1_usize)).focused()),
                ("row 2", window.find(("preset-gallery-row", 2_usize)).focused()),
            ];
            assert!(
                focus_states.iter().all(|(_, focused)| *focused == Some(false)),
                "the next Tab leaves the virtual list instead of visiting another row: {focus_states:?}"
            );
        })
        .expect("test window remains available");
    }

    #[gpui_kit::test]
    fn failed_gallery_preview_preserves_applied_preset_and_exposes_alert(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary preset directory is created");
        let invalid_path = directory.path().join("Broken Signal.milk");
        fs::write(&invalid_path, [0xff, 0xfe]).expect("invalid UTF-8 preset is written");
        let local_presets = crate::preset_catalog::PresetCatalog::discover_local(directory.path())
            .expect("invalid preset metadata is still discoverable");

        cx.update(gpui_kit::init);
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
                |_, cx| cx.new(PlayerView::new),
            )
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.visualizer_mode = VisualizerMode::MilkDrop;
                view.preset_catalog.replace_local(local_presets);
                view.refresh_preset_gallery(cx);
                view.preset_selected_id = view
                    .preset_catalog
                    .entries()
                    .iter()
                    .find(|entry| entry.title() == "Broken Signal")
                    .map(|entry| entry.id().to_owned());
                view.preview_selected_preset(cx);
            });
        });
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("milkdrop-preset-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            assert_eq!(
                alert.label(),
                Some("MilkDrop preset error: preset is not valid UTF-8 text")
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(view.preset_preview.applied().name(), "First Light");
            assert_eq!(view.preset_preview.previewed(), None);
        });
    }

    #[gpui_kit::test]
    fn corrupt_restored_preset_folder_keeps_first_light_and_exposes_alert(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary bookmark directory is created");
        let store = crate::preset_folder::PresetFolderBookmarkStore::new(
            directory.path().join("preset-folder.bookmark"),
        );
        store
            .save(&[0xff, 0x00, 0xfe])
            .expect("corrupt bookmark fixture is persisted");

        cx.update(gpui_kit::init);
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
                |_, cx| cx.new(PlayerView::new),
            )
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.visualizer_mode = VisualizerMode::MilkDrop;
                view.restore_preset_folder(store, cx);
            });
        });
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("milkdrop-preset-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            assert!(
                alert
                    .label()
                    .is_some_and(|label| label.contains("resolve preset folder bookmark")),
                "restoration failure is exposed through the existing preset alert"
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(view.preset_catalog.entries().len(), 1);
            assert_eq!(view.preset_catalog.entries()[0].title(), "First Light");
        });
    }

    #[gpui_kit::test]
    fn persisted_preset_folder_restores_and_rescans_after_reopen(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary preset directory is created");
        fs::write(directory.path().join("Return Signal.milk"), "[preset00]\n")
            .expect("local preset is written");
        let store = crate::preset_folder::PresetFolderBookmarkStore::new(
            directory.path().join("preset-folder.bookmark"),
        );
        let prepared = crate::preset_folder::create_bookmark(directory.path())
            .expect("selected folder produces a bookmark");
        store
            .save(prepared.bookmark())
            .expect("bookmark is persisted");
        drop(prepared);

        cx.update(gpui_kit::init);
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
                |_, cx| cx.new(PlayerView::new),
            )
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.visualizer_mode = VisualizerMode::MilkDrop;
                view.restore_preset_folder(store, cx);
            });
        });
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("preset-gallery-status").label(),
                Some("2 of 2 presets shown")
            );
            assert_eq!(
                window.find(("preset-gallery-row", 1_usize)).label(),
                Some("Select Return Signal, Uncategorised preset")
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            assert!(view.read(cx).preset_folder_access.is_some());
        });
    }

    #[gpui_kit::test]
    fn stale_restored_preset_folder_renews_the_persisted_bookmark(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary preset directory is created");
        let original = directory.path().join("Original Presets");
        let moved = directory.path().join("Moved Presets");
        fs::create_dir(&original).expect("original preset folder is created");
        fs::write(original.join("Return Signal.milk"), "[preset00]\n")
            .expect("local preset is written");
        let store = crate::preset_folder::PresetFolderBookmarkStore::new(
            directory.path().join("preset-folder.bookmark"),
        );
        let prepared = crate::preset_folder::create_bookmark(&original)
            .expect("selected folder produces a bookmark");
        let stale_bookmark = prepared.bookmark().to_vec();
        store
            .save(&stale_bookmark)
            .expect("original bookmark is persisted");
        drop(prepared);
        fs::rename(&original, &moved).expect("preset folder is moved on the same volume");

        cx.update(gpui_kit::init);
        let view = cx.update(|cx| cx.new(PlayerView::new));
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.restore_preset_folder(store.clone(), cx);
            });
        });
        cx.run_until_parked();

        let renewed_bookmark = store
            .load()
            .expect("renewed bookmark reopens")
            .expect("renewed bookmark is present");
        assert_ne!(
            renewed_bookmark, stale_bookmark,
            "stale bookmark bytes are atomically replaced"
        );
        let renewed = crate::preset_folder::restore_bookmark(&renewed_bookmark)
            .expect("renewed bookmark resolves");
        assert_eq!(
            renewed.access().path(),
            moved
                .canonicalize()
                .expect("moved preset folder has a canonical path")
        );
        assert_eq!(
            renewed.refreshed_bookmark(),
            None,
            "the persisted replacement is no longer stale"
        );
        cx.update(|cx| {
            let view = view.read(cx);
            assert!(view.preset_folder_access.is_some());
            assert_eq!(view.preset_catalog.entries().len(), 2);
        });
    }

    #[gpui_kit::test]
    fn failed_preset_folder_creation_preserves_the_last_working_folder(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary preset directory is created");
        let working = directory.path().join("Working Presets");
        fs::create_dir(&working).expect("working preset folder is created");
        fs::write(working.join("Existing Signal.milk"), "[preset00]\n")
            .expect("working preset is written");
        let missing = directory.path().join("Missing Presets");
        let store = crate::preset_folder::PresetFolderBookmarkStore::new(
            directory.path().join("preset-folder.bookmark"),
        );
        let prepared = crate::preset_folder::create_bookmark(&working)
            .expect("working folder produces a bookmark");
        let working_bookmark = prepared.bookmark().to_vec();
        store
            .save(&working_bookmark)
            .expect("working bookmark is persisted");
        let working_entries = crate::preset_catalog::PresetCatalog::discover_local(&working)
            .expect("working presets are discovered");

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
                view.preset_bookmark_store = Some(store.clone());
                view.preset_folder_access = Some(prepared.into_access());
                view.preset_catalog.replace_local(working_entries);
                view.refresh_preset_gallery(cx);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("add-preset-folder", cx);
        })
        .expect("test window remains available");
        cx.simulate_path_prompt_response(move |_| Some(vec![missing]));
        cx.run_until_parked();

        assert_eq!(
            store.load().expect("working bookmark reopens"),
            Some(working_bookmark),
            "failed creation does not replace the persisted grant"
        );
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("milkdrop-preset-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            let label = alert
                .label()
                .expect("creation failure has an accessible label");
            assert!(
                label.contains("create preset folder bookmark"),
                "the creation failure remains visible: {label}"
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(
                view.preset_folder_access
                    .as_ref()
                    .expect("working access remains active")
                    .path(),
                working
            );
            assert!(
                view.preset_catalog
                    .entries()
                    .iter()
                    .any(|entry| entry.title() == "Existing Signal"),
                "the working preset remains in the gallery"
            );
        });
    }

    #[gpui_kit::test]
    fn failed_preset_folder_scan_preserves_the_last_working_folder(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempdir().expect("temporary preset directory is created");
        let working = directory.path().join("Working Presets");
        fs::create_dir(&working).expect("working preset folder is created");
        fs::write(working.join("Existing Signal.milk"), "[preset00]\n")
            .expect("working preset is written");
        let replacement = directory.path().join("Replacement Presets");
        let blocked = replacement.join("Blocked Category");
        fs::create_dir(&replacement).expect("replacement preset folder is created");
        fs::create_dir(&blocked).expect("blocked category is created");
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))
            .expect("blocked category permissions are set");
        let store = crate::preset_folder::PresetFolderBookmarkStore::new(
            directory.path().join("preset-folder.bookmark"),
        );
        let prepared = crate::preset_folder::create_bookmark(&working)
            .expect("working folder produces a bookmark");
        let working_bookmark = prepared.bookmark().to_vec();
        store
            .save(&working_bookmark)
            .expect("working bookmark is persisted");
        let working_entries = crate::preset_catalog::PresetCatalog::discover_local(&working)
            .expect("working presets are discovered");

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
                view.preset_bookmark_store = Some(store.clone());
                view.preset_folder_access = Some(prepared.into_access());
                view.preset_catalog.replace_local(working_entries);
                view.refresh_preset_gallery(cx);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("add-preset-folder", cx);
        })
        .expect("test window remains available");
        cx.simulate_path_prompt_response(move |_| Some(vec![replacement]));
        cx.run_until_parked();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700))
            .expect("blocked category permissions are restored");

        assert_eq!(
            store.load().expect("working bookmark reopens"),
            Some(working_bookmark),
            "failed scanning does not replace the persisted grant"
        );
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("milkdrop-preset-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            let label = alert.label().expect("scan failure has an accessible label");
            assert!(
                label.contains("could not scan preset folder"),
                "the scan failure remains visible: {label}"
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(
                view.preset_folder_access
                    .as_ref()
                    .expect("working access remains active")
                    .path(),
                working
            );
            assert!(
                view.preset_catalog
                    .entries()
                    .iter()
                    .any(|entry| entry.title() == "Existing Signal"),
                "the working preset remains in the gallery"
            );
        });
    }

    #[gpui_kit::test]
    fn failed_preset_folder_save_preserves_the_last_working_folder(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempdir().expect("temporary preset directory is created");
        let working = directory.path().join("Working Presets");
        fs::create_dir(&working).expect("working preset folder is created");
        fs::write(working.join("Existing Signal.milk"), "[preset00]\n")
            .expect("working preset is written");
        let replacement = directory.path().join("Replacement Presets");
        fs::create_dir(&replacement).expect("replacement preset folder is created");
        fs::write(replacement.join("New Signal.milk"), "[preset00]\n")
            .expect("replacement preset is written");
        let store = crate::preset_folder::PresetFolderBookmarkStore::new(
            directory.path().join("preset-folder.bookmark"),
        );
        let prepared = crate::preset_folder::create_bookmark(&working)
            .expect("working folder produces a bookmark");
        let working_bookmark = prepared.bookmark().to_vec();
        store
            .save(&working_bookmark)
            .expect("working bookmark is persisted");
        let working_entries = crate::preset_catalog::PresetCatalog::discover_local(&working)
            .expect("working presets are discovered");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500))
            .expect("bookmark directory is made read-only");

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
                view.preset_bookmark_store = Some(store.clone());
                view.preset_folder_access = Some(prepared.into_access());
                view.preset_catalog.replace_local(working_entries);
                view.refresh_preset_gallery(cx);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("add-preset-folder", cx);
        })
        .expect("test window remains available");
        cx.simulate_path_prompt_response(move |_| Some(vec![replacement]));
        cx.run_until_parked();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .expect("bookmark directory permissions are restored");

        assert_eq!(
            store.load().expect("working bookmark reopens"),
            Some(working_bookmark),
            "failed persistence does not replace the previous bookmark"
        );
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("milkdrop-preset-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            let label = alert.label().expect("save failure has an accessible label");
            assert!(
                label.contains("create temporary bookmark"),
                "the save failure remains visible: {label}"
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(
                view.preset_folder_access
                    .as_ref()
                    .expect("working access remains active")
                    .path(),
                working
            );
            assert!(
                view.preset_catalog
                    .entries()
                    .iter()
                    .any(|entry| entry.title() == "Existing Signal"),
                "the working preset remains in the gallery"
            );
            assert!(
                view.preset_catalog
                    .entries()
                    .iter()
                    .all(|entry| entry.title() != "New Signal"),
                "the undurable replacement is not published"
            );
        });
    }

    #[gpui_kit::test]
    fn preset_gallery_adds_a_local_folder_then_filters_and_searches_it(cx: &mut TestAppContext) {
        let directory = tempdir().expect("temporary preset directory is created");
        fs::create_dir(directory.path().join("Fractal")).expect("fractal category is created");
        fs::create_dir(directory.path().join("Particles")).expect("particle category is created");
        fs::write(
            directory.path().join("Fractal/Cosmic Spiral.milk"),
            "[preset00]\n",
        )
        .expect("fractal preset is written");
        fs::write(
            directory.path().join("Particles/Cosmic Bloom.milk"),
            "[preset00]\n",
        )
        .expect("particle preset is written");
        let preset_root = directory.path().to_path_buf();
        let bookmark_store = crate::preset_folder::PresetFolderBookmarkStore::new(
            directory.path().join("preset-folder.bookmark"),
        );
        let persisted_bookmark = bookmark_store.clone();

        cx.update(gpui_kit::init);
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
                |_, cx| cx.new(PlayerView::new),
            )
            .expect("test window opens")
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.visualizer_mode = VisualizerMode::MilkDrop;
                view.preset_bookmark_store = Some(bookmark_store);
                cx.notify();
            });
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("add-preset-folder", cx);
        })
        .expect("test window remains available");
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(move |options| {
            assert!(!options.files);
            assert!(options.directories);
            assert!(!options.multiple);
            assert_eq!(
                options.prompt.as_deref(),
                Some("Add a MilkDrop preset folder")
            );
            Some(vec![preset_root])
        });
        cx.run_until_parked();

        assert!(
            persisted_bookmark
                .load()
                .expect("persisted bookmark reopens")
                .is_some(),
            "successful Add Folder persists opaque bookmark data"
        );
        cx.update(|cx| {
            assert!(
                view.read(cx).preset_folder_access.is_some(),
                "the security-scope guard remains alive for later previews"
            );
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("preset-gallery-status").label(),
                Some("3 of 3 presets shown")
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.set_preset_category(Some("Fractal".to_owned()), cx);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("preset-gallery-status").label(),
                Some("1 of 3 presets shown")
            );
            assert_eq!(
                window.find(("preset-gallery-row", 0_usize)).label(),
                Some("Select Cosmic Spiral, Fractal preset")
            );
        })
        .expect("test window remains available");
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.set_preset_category(None, cx);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let search = view
                .read(cx)
                .preset_search
                .as_ref()
                .expect("preset search is initialized")
                .clone();
            search.update(cx, |input, cx| input.set_value("csmc blm", window, cx));
        })
        .expect("test window remains available");
        cx.run_until_parked();
        cx.update(|cx| {
            view.update(cx, |view, cx| view.refresh_preset_gallery(cx));
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("preset-gallery-status").label(),
                Some("1 of 3 presets shown")
            );
            assert_eq!(
                window.find(("preset-gallery-row", 0_usize)).label(),
                Some("Select Cosmic Bloom, Particles preset")
            );
        })
        .expect("test window remains available");
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
            assert_eq!(window.find("open-settings").focused(), Some(true));
            window.press("tab", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("preview-remote-source").focused(), Some(true));
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

    fn write_tagged_stereo_wav(path: &std::path::Path, sample_rate: u32, frame_count: u32) {
        write_stereo_wav(path, sample_rate, frame_count);
        let wav = fs::read(path).expect("WAV fixture is read for tagging");
        let mut apic_body = Vec::new();
        apic_body.push(0);
        apic_body.extend_from_slice(b"image/png\0");
        apic_body.push(3);
        apic_body.push(0);
        apic_body.extend_from_slice(b"literal-png-fixture");
        let mut apic_frame = Vec::new();
        apic_frame.extend_from_slice(b"APIC");
        apic_frame.extend_from_slice(&(apic_body.len() as u32).to_be_bytes());
        apic_frame.extend_from_slice(&[0, 0]);
        apic_frame.extend_from_slice(&apic_body);
        let mut tagged = Vec::new();
        tagged.extend_from_slice(b"ID3\x03\x00\x00");
        tagged.extend_from_slice(&syncsafe(apic_frame.len()));
        tagged.extend_from_slice(&apic_frame);
        tagged.extend_from_slice(&wav);
        fs::write(path, tagged).expect("tagged WAV fixture is written");
    }

    fn syncsafe(value: usize) -> [u8; 4] {
        [
            ((value >> 21) & 0x7f) as u8,
            ((value >> 14) & 0x7f) as u8,
            ((value >> 7) & 0x7f) as u8,
            (value & 0x7f) as u8,
        ]
    }
}
