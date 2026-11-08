//! Safe macOS boundary for the native MusicKit bridge.

use std::{
    error::Error,
    ffi::c_void,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    slice, str,
    sync::mpsc,
};

use serde::Deserialize;

#[cfg(not(test))]
use super::AppleTransportCommand;
use super::{
    AppleArtworkUrl, AppleMusicId, ApplePlaylist, ApplePlaylistTrack, AppleTransportStatus,
    AuthorisationStatus, CatalogPlayback, CloudLibrary, ConnectionResult, ConnectionSubscription,
    PageCursor, PlaylistPage, RequestId, SubscriptionCapabilities, SubscriptionOffer, TrackPage,
};

const CAPABILITY_ENVELOPE_VERSION: u16 = 1;
const MAX_BRIDGE_ENVELOPE_BYTES: usize = 64 * 1024;

type BridgeRelease = unsafe extern "C" fn(*mut u8);

struct ConnectionCallbackContext {
    sender: mpsc::Sender<Result<ConnectionResponse, BridgeCallError>>,
}

#[cfg(not(test))]
struct PlaylistCallbackContext {
    sender: mpsc::Sender<Result<PlaylistPageResponse, BridgeCallError>>,
}

#[cfg(not(test))]
struct TrackCallbackContext {
    sender: mpsc::Sender<Result<TrackPageResponse, BridgeCallError>>,
}

#[cfg(not(test))]
struct PlaybackCallbackContext {
    sender: mpsc::Sender<Result<PlaybackResponse, BridgeCallError>>,
}

#[cfg(not(test))]
struct TransportCallbackContext {
    sender: mpsc::Sender<Result<TransportResponse, BridgeCallError>>,
}

/// Owns one allocation created by the Swift bridge.
struct OwnedBridgeBuffer {
    pointer: NonNull<u8>,
    length: usize,
    release: BridgeRelease,
}

impl OwnedBridgeBuffer {
    /// Takes ownership of a bridge allocation and its matching release operation.
    ///
    /// # Safety
    ///
    /// `pointer` must address `length` initialized bytes and must have been allocated for exactly
    /// one release through `release`. No other owner may read, write, or free the allocation after
    /// this call succeeds.
    unsafe fn from_raw_parts(
        pointer: *mut u8,
        length: usize,
        release: BridgeRelease,
    ) -> Result<Self, NullBridgeBuffer> {
        let Some(pointer) = NonNull::new(pointer) else {
            return Err(NullBridgeBuffer);
        };
        Ok(Self {
            pointer,
            length,
            release,
        })
    }

    fn as_bounded_bytes(&self) -> Result<&[u8], BridgeProtocolError> {
        if self.length > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: self.length,
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }
        // SAFETY: `from_raw_parts` requires a live allocation containing `length` initialized
        // bytes, and the buffer cannot be released until this borrowed wrapper is dropped.
        Ok(unsafe { slice::from_raw_parts(self.pointer.as_ptr(), self.length) })
    }
}

impl Drop for OwnedBridgeBuffer {
    fn drop(&mut self) {
        // SAFETY: the constructor transfers unique ownership and stores the matching Swift-side
        // release operation. `Drop` runs at most once for this non-Clone owner.
        unsafe { (self.release)(self.pointer.as_ptr()) };
    }
}

/// Reports that the native bridge returned no owned response allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NullBridgeBuffer;

impl fmt::Display for NullBridgeBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the MusicKit bridge returned a null response")
    }
}

impl Error for NullBridgeBuffer {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCapability<'wire> {
    version: u16,
    kind: &'wire str,
    request_id: u64,
    authorisation: &'wire str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSubscription<'wire> {
    version: u16,
    kind: &'wire str,
    request_id: u64,
    can_play_catalog_content: bool,
    can_become_subscriber: bool,
    has_cloud_library_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireConnection<'wire> {
    version: u16,
    kind: &'wire str,
    request_id: u64,
    authorisation: &'wire str,
    subscription: &'wire str,
    can_play_catalog_content: Option<bool>,
    can_become_subscriber: Option<bool>,
    has_cloud_library_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePlaylist {
    id: String,
    name: Option<String>,
    artwork_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePlaylistPage {
    version: u16,
    kind: String,
    request_id: u64,
    requested_cursor: Option<String>,
    next_cursor: Option<String>,
    playlists: Vec<WirePlaylist>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTrack {
    id: String,
    title: String,
    artist: String,
    album: Option<String>,
    duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTrackPage {
    version: u16,
    kind: String,
    request_id: u64,
    playlist_id: String,
    requested_cursor: Option<String>,
    next_cursor: Option<String>,
    tracks: Vec<WireTrack>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePlayback {
    version: u16,
    kind: String,
    request_id: u64,
    track_id: String,
    status: String,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTransport {
    version: u16,
    kind: String,
    request_id: u64,
    track_id: String,
    status: String,
    error: Option<String>,
}

/// Contains a validated capability result from the native bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CapabilityResponse {
    request_id: RequestId,
    authorisation: AuthorisationStatus,
}

impl CapabilityResponse {
    fn parse(bytes: &[u8]) -> Result<Self, BridgeProtocolError> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }

        let text = str::from_utf8(bytes).map_err(BridgeProtocolError::InvalidUtf8)?;
        let wire: WireCapability<'_> =
            serde_json::from_str(text).map_err(BridgeProtocolError::MalformedEnvelope)?;
        if wire.version != CAPABILITY_ENVELOPE_VERSION {
            return Err(BridgeProtocolError::UnsupportedVersion);
        }
        if wire.kind != "capability" {
            return Err(BridgeProtocolError::UnexpectedKind);
        }

        let authorisation = match wire.authorisation {
            "not_determined" => AuthorisationStatus::NotDetermined,
            "denied" => AuthorisationStatus::Denied,
            "restricted" => AuthorisationStatus::Restricted,
            "authorised" => AuthorisationStatus::Authorised,
            "unknown" => AuthorisationStatus::Unknown,
            _ => return Err(BridgeProtocolError::UnknownAuthorisation),
        };

        Ok(Self {
            request_id: RequestId::new(wire.request_id),
            authorisation,
        })
    }

    pub(super) const fn request_id(self) -> RequestId {
        self.request_id
    }

    pub(super) const fn authorisation(self) -> AuthorisationStatus {
        self.authorisation
    }
}

/// Contains validated Apple Music subscription capabilities from the native bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SubscriptionResponse {
    request_id: RequestId,
    capabilities: SubscriptionCapabilities,
}

impl SubscriptionResponse {
    fn parse(bytes: &[u8]) -> Result<Self, BridgeProtocolError> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }

        let text = str::from_utf8(bytes).map_err(BridgeProtocolError::InvalidUtf8)?;
        let wire: WireSubscription<'_> =
            serde_json::from_str(text).map_err(BridgeProtocolError::MalformedEnvelope)?;
        if wire.version != CAPABILITY_ENVELOPE_VERSION {
            return Err(BridgeProtocolError::UnsupportedVersion);
        }
        if wire.kind != "subscription" {
            return Err(BridgeProtocolError::UnexpectedKind);
        }

        let catalog_playback = if wire.can_play_catalog_content {
            CatalogPlayback::Available
        } else {
            CatalogPlayback::Unavailable
        };
        let subscription_offer = if wire.can_become_subscriber {
            SubscriptionOffer::Available
        } else {
            SubscriptionOffer::Unavailable
        };
        let cloud_library = if wire.has_cloud_library_enabled {
            CloudLibrary::Enabled
        } else {
            CloudLibrary::Disabled
        };

        Ok(Self {
            request_id: RequestId::new(wire.request_id),
            capabilities: SubscriptionCapabilities::new(
                catalog_playback,
                subscription_offer,
                cloud_library,
            ),
        })
    }

    pub(super) const fn request_id(self) -> RequestId {
        self.request_id
    }

    pub(super) const fn capabilities(self) -> SubscriptionCapabilities {
        self.capabilities
    }
}

/// Contains one validated terminal result from the native Apple Music connection flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConnectionResponse {
    request_id: RequestId,
    result: ConnectionResult,
}

impl ConnectionResponse {
    fn parse(bytes: &[u8]) -> Result<Self, BridgeProtocolError> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }

        let text = str::from_utf8(bytes).map_err(BridgeProtocolError::InvalidUtf8)?;
        let wire: WireConnection<'_> =
            serde_json::from_str(text).map_err(BridgeProtocolError::MalformedEnvelope)?;
        if wire.version != CAPABILITY_ENVELOPE_VERSION {
            return Err(BridgeProtocolError::UnsupportedVersion);
        }
        if wire.kind != "connection" {
            return Err(BridgeProtocolError::UnexpectedKind);
        }

        let has_capabilities = wire.can_play_catalog_content.is_some()
            || wire.can_become_subscriber.is_some()
            || wire.has_cloud_library_enabled.is_some();
        let result = match wire.authorisation {
            "not_determined" => {
                require_no_subscription(&wire, has_capabilities)?;
                ConnectionResult::NotDetermined
            }
            "denied" => {
                require_no_subscription(&wire, has_capabilities)?;
                ConnectionResult::Denied
            }
            "restricted" => {
                require_no_subscription(&wire, has_capabilities)?;
                ConnectionResult::Restricted
            }
            "authorised" => match wire.subscription {
                "available" => {
                    let Some(can_play_catalog_content) = wire.can_play_catalog_content else {
                        return Err(BridgeProtocolError::InconsistentConnection);
                    };
                    let Some(can_become_subscriber) = wire.can_become_subscriber else {
                        return Err(BridgeProtocolError::InconsistentConnection);
                    };
                    let Some(has_cloud_library_enabled) = wire.has_cloud_library_enabled else {
                        return Err(BridgeProtocolError::InconsistentConnection);
                    };
                    ConnectionResult::Authorised(ConnectionSubscription::Available(
                        subscription_capabilities(
                            can_play_catalog_content,
                            can_become_subscriber,
                            has_cloud_library_enabled,
                        ),
                    ))
                }
                "unavailable" if !has_capabilities => {
                    ConnectionResult::Authorised(ConnectionSubscription::Unavailable)
                }
                _ => return Err(BridgeProtocolError::InconsistentConnection),
            },
            "unknown" => {
                require_no_subscription(&wire, has_capabilities)?;
                ConnectionResult::Unknown
            }
            _ => return Err(BridgeProtocolError::UnknownAuthorisation),
        };

        Ok(Self {
            request_id: RequestId::new(wire.request_id),
            result,
        })
    }

    pub(crate) const fn request_id(self) -> RequestId {
        self.request_id
    }

    pub(crate) const fn result(self) -> ConnectionResult {
        self.result
    }
}

/// Contains one validated page of read-only Apple Music playlists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaylistPageResponse {
    request_id: RequestId,
    requested_cursor: Option<PageCursor>,
    page: PlaylistPage,
}

impl PlaylistPageResponse {
    fn parse(bytes: &[u8]) -> Result<Self, BridgeProtocolError> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }

        let text = str::from_utf8(bytes).map_err(BridgeProtocolError::InvalidUtf8)?;
        let wire: WirePlaylistPage =
            serde_json::from_str(text).map_err(BridgeProtocolError::MalformedEnvelope)?;
        if wire.version != CAPABILITY_ENVELOPE_VERSION {
            return Err(BridgeProtocolError::UnsupportedVersion);
        }
        if wire.kind != "playlist_page" {
            return Err(BridgeProtocolError::UnexpectedKind);
        }
        if wire.error.is_some() {
            return Err(BridgeProtocolError::PlaylistRequestFailed);
        }

        let requested_cursor = wire
            .requested_cursor
            .as_deref()
            .map(PageCursor::try_from)
            .transpose()
            .map_err(|_error| BridgeProtocolError::InvalidPlaylistCursor)?;
        let next_cursor = wire
            .next_cursor
            .as_deref()
            .map(PageCursor::try_from)
            .transpose()
            .map_err(|_error| BridgeProtocolError::InvalidPlaylistCursor)?;
        let playlists = wire
            .playlists
            .into_iter()
            .map(|playlist| {
                let id = AppleMusicId::try_from(playlist.id.as_str())
                    .map_err(|_error| BridgeProtocolError::InvalidPlaylistIdentifier)?;
                let artwork_url = playlist
                    .artwork_url
                    .as_deref()
                    .and_then(|url| AppleArtworkUrl::try_from(url).ok());
                Ok(ApplePlaylist::with_artwork(id, playlist.name, artwork_url))
            })
            .collect::<Result<Vec<_>, BridgeProtocolError>>()?;

        Ok(Self {
            request_id: RequestId::new(wire.request_id),
            requested_cursor,
            page: PlaylistPage::new(playlists, next_cursor),
        })
    }

    pub(crate) const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub(crate) const fn requested_cursor(&self) -> Option<&PageCursor> {
        self.requested_cursor.as_ref()
    }

    pub(crate) fn into_page(self) -> PlaylistPage {
        self.page
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(
        request_id: RequestId,
        requested_cursor: Option<PageCursor>,
        page: PlaylistPage,
    ) -> Self {
        Self {
            request_id,
            requested_cursor,
            page,
        }
    }
}

/// Contains one validated page of entries for a selected Apple Music playlist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackPageResponse {
    request_id: RequestId,
    requested_cursor: Option<PageCursor>,
    page: TrackPage,
}

impl TrackPageResponse {
    fn parse(bytes: &[u8]) -> Result<Self, BridgeProtocolError> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }
        let text = str::from_utf8(bytes).map_err(BridgeProtocolError::InvalidUtf8)?;
        let wire: WireTrackPage =
            serde_json::from_str(text).map_err(BridgeProtocolError::MalformedEnvelope)?;
        if wire.version != CAPABILITY_ENVELOPE_VERSION {
            return Err(BridgeProtocolError::UnsupportedVersion);
        }
        if wire.kind != "playlist_track_page" {
            return Err(BridgeProtocolError::UnexpectedKind);
        }
        if wire.error.is_some() {
            return Err(BridgeProtocolError::TrackRequestFailed);
        }
        let playlist_id = AppleMusicId::try_from(wire.playlist_id.as_str())
            .map_err(|_error| BridgeProtocolError::InvalidPlaylistIdentifier)?;
        let requested_cursor = wire
            .requested_cursor
            .as_deref()
            .map(PageCursor::try_from)
            .transpose()
            .map_err(|_error| BridgeProtocolError::InvalidPlaylistCursor)?;
        let next_cursor = wire
            .next_cursor
            .as_deref()
            .map(PageCursor::try_from)
            .transpose()
            .map_err(|_error| BridgeProtocolError::InvalidPlaylistCursor)?;
        let tracks = wire
            .tracks
            .into_iter()
            .map(|track| {
                let id = AppleMusicId::try_from(track.id.as_str())
                    .map_err(|_error| BridgeProtocolError::InvalidTrackIdentifier)?;
                Ok(ApplePlaylistTrack::new(
                    id,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_ms,
                ))
            })
            .collect::<Result<Vec<_>, BridgeProtocolError>>()?;
        Ok(Self {
            request_id: RequestId::new(wire.request_id),
            requested_cursor,
            page: TrackPage::new(playlist_id, tracks, next_cursor),
        })
    }

    pub(crate) const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub(crate) const fn requested_cursor(&self) -> Option<&PageCursor> {
        self.requested_cursor.as_ref()
    }

    pub(crate) fn into_page(self) -> TrackPage {
        self.page
    }

    #[cfg(test)]
    fn completed_for_test(
        request_id: RequestId,
        requested_cursor: Option<PageCursor>,
        page: TrackPage,
    ) -> Self {
        Self {
            request_id,
            requested_cursor,
            page,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackResponse {
    request_id: RequestId,
    track_id: AppleMusicId,
}

impl PlaybackResponse {
    fn parse(bytes: &[u8]) -> Result<Self, BridgeProtocolError> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }
        let text = str::from_utf8(bytes).map_err(BridgeProtocolError::InvalidUtf8)?;
        let wire: WirePlayback =
            serde_json::from_str(text).map_err(BridgeProtocolError::MalformedEnvelope)?;
        if wire.version != CAPABILITY_ENVELOPE_VERSION {
            return Err(BridgeProtocolError::UnsupportedVersion);
        }
        if wire.kind != "playback" {
            return Err(BridgeProtocolError::UnexpectedKind);
        }
        if wire.error.is_some() || wire.status != "playing" {
            return Err(BridgeProtocolError::PlaybackFailed);
        }
        let track_id = AppleMusicId::try_from(wire.track_id.as_str())
            .map_err(|_error| BridgeProtocolError::InvalidTrackIdentifier)?;
        Ok(Self {
            request_id: RequestId::new(wire.request_id),
            track_id,
        })
    }

    pub(crate) const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub(crate) fn into_track_id(self) -> AppleMusicId {
        self.track_id
    }

    #[cfg(test)]
    fn completed_for_test(request_id: RequestId, track_id: AppleMusicId) -> Self {
        Self {
            request_id,
            track_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransportResponse {
    request_id: RequestId,
    track_id: AppleMusicId,
    status: AppleTransportStatus,
}

impl TransportResponse {
    fn parse(bytes: &[u8]) -> Result<Self, BridgeProtocolError> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err(BridgeProtocolError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: MAX_BRIDGE_ENVELOPE_BYTES,
            });
        }
        let text = str::from_utf8(bytes).map_err(BridgeProtocolError::InvalidUtf8)?;
        let wire: WireTransport =
            serde_json::from_str(text).map_err(BridgeProtocolError::MalformedEnvelope)?;
        if wire.version != CAPABILITY_ENVELOPE_VERSION {
            return Err(BridgeProtocolError::UnsupportedVersion);
        }
        if wire.kind != "transport" {
            return Err(BridgeProtocolError::UnexpectedKind);
        }
        if wire.error.is_some() {
            return Err(BridgeProtocolError::TransportFailed);
        }
        let status = match wire.status.as_str() {
            "paused" => AppleTransportStatus::Paused,
            "playing" => AppleTransportStatus::Playing,
            "stopped" => AppleTransportStatus::Stopped,
            _ => return Err(BridgeProtocolError::TransportFailed),
        };
        let track_id = AppleMusicId::try_from(wire.track_id.as_str())
            .map_err(|_error| BridgeProtocolError::InvalidTrackIdentifier)?;
        Ok(Self {
            request_id: RequestId::new(wire.request_id),
            track_id,
            status,
        })
    }

    pub(crate) const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub(crate) fn into_track_id(self) -> AppleMusicId {
        self.track_id
    }

    pub(crate) const fn status(&self) -> AppleTransportStatus {
        self.status
    }

    #[cfg(test)]
    fn completed_for_test(
        request_id: RequestId,
        track_id: AppleMusicId,
        status: AppleTransportStatus,
    ) -> Self {
        Self {
            request_id,
            track_id,
            status,
        }
    }
}

fn require_no_subscription(
    wire: &WireConnection<'_>,
    has_capabilities: bool,
) -> Result<(), BridgeProtocolError> {
    if wire.subscription != "not_applicable" || has_capabilities {
        return Err(BridgeProtocolError::InconsistentConnection);
    }
    Ok(())
}

fn subscription_capabilities(
    can_play_catalog_content: bool,
    can_become_subscriber: bool,
    has_cloud_library_enabled: bool,
) -> SubscriptionCapabilities {
    let catalog_playback = if can_play_catalog_content {
        CatalogPlayback::Available
    } else {
        CatalogPlayback::Unavailable
    };
    let subscription_offer = if can_become_subscriber {
        SubscriptionOffer::Available
    } else {
        SubscriptionOffer::Unavailable
    };
    let cloud_library = if has_cloud_library_enabled {
        CloudLibrary::Enabled
    } else {
        CloudLibrary::Disabled
    };
    SubscriptionCapabilities::new(catalog_playback, subscription_offer, cloud_library)
}

/// Reports a response that cannot safely cross the MusicKit bridge boundary.
#[derive(Debug)]
pub(crate) enum BridgeProtocolError {
    EnvelopeTooLarge { actual: usize, maximum: usize },
    InvalidUtf8(str::Utf8Error),
    MalformedEnvelope(serde_json::Error),
    UnsupportedVersion,
    UnexpectedKind,
    UnknownAuthorisation,
    InconsistentConnection,
    InvalidPlaylistCursor,
    InvalidPlaylistIdentifier,
    InvalidTrackIdentifier,
    PlaylistRequestFailed,
    TrackRequestFailed,
    PlaybackFailed,
    TransportFailed,
}

impl fmt::Display for BridgeProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnvelopeTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "the MusicKit bridge response is too large: {actual} bytes exceeds {maximum}"
                )
            }
            Self::InvalidUtf8(_) => {
                formatter.write_str("the MusicKit bridge response is not valid UTF-8")
            }
            Self::MalformedEnvelope(_) => {
                formatter.write_str("the MusicKit bridge response is not a valid envelope")
            }
            Self::UnsupportedVersion => {
                formatter.write_str("the MusicKit bridge response version is unsupported")
            }
            Self::UnexpectedKind => {
                formatter.write_str("the MusicKit bridge response kind is unexpected")
            }
            Self::UnknownAuthorisation => {
                formatter.write_str("the MusicKit bridge authorization status is unknown")
            }
            Self::InconsistentConnection => {
                formatter.write_str("the MusicKit bridge connection result is inconsistent")
            }
            Self::InvalidPlaylistCursor => {
                formatter.write_str("Apple Music returned an invalid playlist page cursor")
            }
            Self::InvalidPlaylistIdentifier => {
                formatter.write_str("Apple Music returned an invalid playlist identifier")
            }
            Self::InvalidTrackIdentifier => {
                formatter.write_str("Apple Music returned an invalid playlist entry identifier")
            }
            Self::PlaylistRequestFailed => {
                formatter.write_str("Apple Music could not load the playlist library")
            }
            Self::TrackRequestFailed => {
                formatter.write_str("Apple Music could not load the selected playlist")
            }
            Self::PlaybackFailed => {
                formatter.write_str("Apple Music could not start MusicKit playback")
            }
            Self::TransportFailed => formatter.write_str("Apple Music transport command failed"),
        }
    }
}

impl Error for BridgeProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidUtf8(error) => Some(error),
            Self::MalformedEnvelope(error) => Some(error),
            Self::EnvelopeTooLarge { .. }
            | Self::UnsupportedVersion
            | Self::UnexpectedKind
            | Self::UnknownAuthorisation
            | Self::InconsistentConnection
            | Self::InvalidPlaylistCursor
            | Self::InvalidPlaylistIdentifier
            | Self::InvalidTrackIdentifier
            | Self::PlaylistRequestFailed
            | Self::TrackRequestFailed
            | Self::PlaybackFailed
            | Self::TransportFailed => None,
        }
    }
}

/// Reports a failure to obtain a trustworthy response from the native bridge.
#[derive(Debug)]
pub(crate) enum BridgeCallError {
    #[cfg(not(amp808_music_kit_bridge))]
    Unavailable,
    MissingResponse,
    Protocol(BridgeProtocolError),
    CallbackPanicked,
    StartRejected,
    MissingUsageDescription,
    UnexpectedBundleIdentifier,
    #[cfg(not(test))]
    InvalidPlaylistCursor,
}

impl fmt::Display for BridgeCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(not(amp808_music_kit_bridge))]
            Self::Unavailable => formatter.write_str("the MusicKit bridge is unavailable"),
            Self::MissingResponse => {
                formatter.write_str("the MusicKit bridge returned a null response")
            }
            Self::Protocol(error) => error.fmt(formatter),
            Self::CallbackPanicked => {
                formatter.write_str("the MusicKit bridge callback failed safely")
            }
            Self::StartRejected => {
                formatter.write_str("the MusicKit bridge rejected the connection request")
            }
            Self::MissingUsageDescription => formatter
                .write_str("Apple Music requires the app bundle media-library usage description"),
            Self::UnexpectedBundleIdentifier => {
                formatter.write_str("Apple Music requires the com.opsydyn.amp808r3tr0 app bundle")
            }
            #[cfg(not(test))]
            Self::InvalidPlaylistCursor => {
                formatter.write_str("Apple Music returned an invalid playlist page cursor")
            }
        }
    }
}

impl Error for BridgeCallError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            #[cfg(not(amp808_music_kit_bridge))]
            Self::Unavailable => None,
            Self::MissingResponse => None,
            Self::Protocol(error) => Some(error),
            Self::CallbackPanicked => None,
            Self::StartRejected => None,
            Self::MissingUsageDescription => None,
            Self::UnexpectedBundleIdentifier => None,
            #[cfg(not(test))]
            Self::InvalidPlaylistCursor => None,
        }
    }
}

/// Receives one terminal result from a user-triggered MusicKit connection request.
pub(crate) struct ConnectionTask {
    receiver: mpsc::Receiver<Result<ConnectionResponse, BridgeCallError>>,
    #[cfg(test)]
    _test_sender: Option<mpsc::Sender<Result<ConnectionResponse, BridgeCallError>>>,
}

impl ConnectionTask {
    #[cfg(amp808_music_kit_bridge)]
    pub(crate) fn start(request_id: RequestId) -> Result<Self, BridgeCallError> {
        let (sender, receiver) = mpsc::channel();
        let context = Box::new(ConnectionCallbackContext { sender });
        let context = Box::into_raw(context).cast::<c_void>();
        // SAFETY: `context` is a unique one-shot callback allocation. The Swift bridge promises
        // exactly one terminal callback when it accepts the request; rejection returns ownership.
        let accepted = unsafe {
            amp808_music_kit_request_connection(request_id.value(), context, connection_callback)
        };
        if accepted != 1 {
            // SAFETY: Swift rejected the request and therefore did not retain or callback with the
            // unique context pointer, so Rust still owns it.
            drop(unsafe { Box::from_raw(context.cast::<ConnectionCallbackContext>()) });
            return Err(match accepted {
                2 => BridgeCallError::MissingUsageDescription,
                3 => BridgeCallError::UnexpectedBundleIdentifier,
                _ => BridgeCallError::StartRejected,
            });
        }
        Ok(Self {
            receiver,
            #[cfg(test)]
            _test_sender: None,
        })
    }

    #[cfg(not(amp808_music_kit_bridge))]
    pub(crate) const fn start(_request_id: RequestId) -> Result<Self, BridgeCallError> {
        Err(BridgeCallError::Unavailable)
    }

    pub(crate) fn try_recv(
        &self,
    ) -> Result<Result<ConnectionResponse, BridgeCallError>, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn pending_for_test() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            receiver,
            _test_sender: Some(sender),
        }
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(request_id: RequestId, result: ConnectionResult) -> Self {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(ConnectionResponse { request_id, result }))
            .expect("the test connection receiver remains live during construction");
        Self {
            receiver,
            _test_sender: None,
        }
    }
}

/// Receives one terminal page from a read-only MusicKit library request.
pub(crate) struct PlaylistTask {
    receiver: mpsc::Receiver<Result<PlaylistPageResponse, BridgeCallError>>,
    #[cfg(test)]
    _test_sender: Option<mpsc::Sender<Result<PlaylistPageResponse, BridgeCallError>>>,
}

impl PlaylistTask {
    #[cfg(all(amp808_music_kit_bridge, not(test)))]
    pub(crate) fn start(
        request_id: RequestId,
        requested_cursor: Option<&PageCursor>,
    ) -> Result<Self, BridgeCallError> {
        let offset = requested_cursor.map_or(Ok(0_u64), |cursor| {
            cursor
                .as_str()
                .parse::<u64>()
                .map_err(|_error| BridgeCallError::InvalidPlaylistCursor)
        })?;
        let (sender, receiver) = mpsc::channel();
        let context = Box::new(PlaylistCallbackContext { sender });
        let context = Box::into_raw(context).cast::<c_void>();
        // SAFETY: `context` is a unique one-shot callback allocation. Swift either accepts and
        // invokes the callback exactly once or rejects without retaining the context.
        let accepted = unsafe {
            amp808_music_kit_request_playlist_page(
                request_id.value(),
                offset,
                context,
                playlist_page_callback,
            )
        };
        if accepted != 1 {
            // SAFETY: a rejected request did not retain the unique context allocation.
            drop(unsafe { Box::from_raw(context.cast::<PlaylistCallbackContext>()) });
            return Err(match accepted {
                2 => BridgeCallError::MissingUsageDescription,
                3 => BridgeCallError::UnexpectedBundleIdentifier,
                _ => BridgeCallError::StartRejected,
            });
        }
        Ok(Self {
            receiver,
            #[cfg(test)]
            _test_sender: None,
        })
    }

    #[cfg(all(not(amp808_music_kit_bridge), not(test)))]
    pub(crate) const fn start(
        _request_id: RequestId,
        _requested_cursor: Option<&PageCursor>,
    ) -> Result<Self, BridgeCallError> {
        Err(BridgeCallError::Unavailable)
    }

    pub(crate) fn try_recv(
        &self,
    ) -> Result<Result<PlaylistPageResponse, BridgeCallError>, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn pending_for_test() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            receiver,
            _test_sender: Some(sender),
        }
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(
        request_id: RequestId,
        requested_cursor: Option<PageCursor>,
        page: PlaylistPage,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(PlaylistPageResponse::completed_for_test(
                request_id,
                requested_cursor,
                page,
            )))
            .expect("the test playlist receiver remains live during construction");
        Self {
            receiver,
            _test_sender: None,
        }
    }
}

/// Receives one terminal page from a selected Apple Music playlist.
pub(crate) struct TrackTask {
    receiver: mpsc::Receiver<Result<TrackPageResponse, BridgeCallError>>,
    #[cfg(test)]
    _test_sender: Option<mpsc::Sender<Result<TrackPageResponse, BridgeCallError>>>,
}

impl TrackTask {
    #[cfg(all(amp808_music_kit_bridge, not(test)))]
    pub(crate) fn start(
        request_id: RequestId,
        playlist_id: &AppleMusicId,
        requested_cursor: Option<&PageCursor>,
    ) -> Result<Self, BridgeCallError> {
        let offset = requested_cursor.map_or(Ok(0_u64), |cursor| {
            cursor
                .as_str()
                .parse::<u64>()
                .map_err(|_error| BridgeCallError::InvalidPlaylistCursor)
        })?;
        let playlist_id = playlist_id.as_str().as_bytes();
        let (sender, receiver) = mpsc::channel();
        let context = Box::new(TrackCallbackContext { sender });
        let context = Box::into_raw(context).cast::<c_void>();
        // SAFETY: the string bytes remain live for the duration of the synchronous bridge call;
        // Swift copies them before returning. The unique callback context is consumed once.
        let accepted = unsafe {
            amp808_music_kit_request_track_page(
                request_id.value(),
                playlist_id.as_ptr(),
                playlist_id.len(),
                offset,
                context,
                track_page_callback,
            )
        };
        if accepted != 1 {
            // SAFETY: a rejected request did not retain the unique context allocation.
            drop(unsafe { Box::from_raw(context.cast::<TrackCallbackContext>()) });
            return Err(match accepted {
                2 => BridgeCallError::MissingUsageDescription,
                3 => BridgeCallError::UnexpectedBundleIdentifier,
                _ => BridgeCallError::StartRejected,
            });
        }
        Ok(Self {
            receiver,
            #[cfg(test)]
            _test_sender: None,
        })
    }

    #[cfg(all(not(amp808_music_kit_bridge), not(test)))]
    pub(crate) const fn start(
        _request_id: RequestId,
        _playlist_id: &AppleMusicId,
        _requested_cursor: Option<&PageCursor>,
    ) -> Result<Self, BridgeCallError> {
        Err(BridgeCallError::Unavailable)
    }

    pub(crate) fn try_recv(
        &self,
    ) -> Result<Result<TrackPageResponse, BridgeCallError>, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn pending_for_test() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            receiver,
            _test_sender: Some(sender),
        }
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(
        request_id: RequestId,
        requested_cursor: Option<PageCursor>,
        page: TrackPage,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(TrackPageResponse::completed_for_test(
                request_id,
                requested_cursor,
                page,
            )))
            .expect("the test track receiver remains live during construction");
        Self {
            receiver,
            _test_sender: None,
        }
    }
}

/// Receives one terminal result from a user-triggered MusicKit playback handoff.
pub(crate) struct PlaybackTask {
    receiver: mpsc::Receiver<Result<PlaybackResponse, BridgeCallError>>,
    #[cfg(test)]
    _test_sender: Option<mpsc::Sender<Result<PlaybackResponse, BridgeCallError>>>,
}

impl PlaybackTask {
    #[cfg(all(amp808_music_kit_bridge, not(test)))]
    pub(crate) fn start(
        request_id: RequestId,
        playlist_id: &AppleMusicId,
        track_id: &AppleMusicId,
    ) -> Result<Self, BridgeCallError> {
        let playlist_id = playlist_id.as_str().as_bytes();
        let track_id = track_id.as_str().as_bytes();
        let (sender, receiver) = mpsc::channel();
        let context = Box::new(PlaybackCallbackContext { sender });
        let context = Box::into_raw(context).cast::<c_void>();
        // SAFETY: the identifier bytes remain live for the synchronous bridge call. Swift copies
        // them before returning, and the unique callback context is consumed exactly once.
        let accepted = unsafe {
            amp808_music_kit_play_track(
                request_id.value(),
                playlist_id.as_ptr(),
                playlist_id.len(),
                track_id.as_ptr(),
                track_id.len(),
                context,
                playback_callback,
            )
        };
        if accepted != 1 {
            // SAFETY: a rejected request did not retain the unique context allocation.
            drop(unsafe { Box::from_raw(context.cast::<PlaybackCallbackContext>()) });
            return Err(match accepted {
                2 => BridgeCallError::MissingUsageDescription,
                3 => BridgeCallError::UnexpectedBundleIdentifier,
                _ => BridgeCallError::StartRejected,
            });
        }
        Ok(Self {
            receiver,
            #[cfg(test)]
            _test_sender: None,
        })
    }

    #[cfg(all(not(amp808_music_kit_bridge), not(test)))]
    pub(crate) const fn start(
        _request_id: RequestId,
        _playlist_id: &AppleMusicId,
        _track_id: &AppleMusicId,
    ) -> Result<Self, BridgeCallError> {
        Err(BridgeCallError::Unavailable)
    }

    pub(crate) fn try_recv(
        &self,
    ) -> Result<Result<PlaybackResponse, BridgeCallError>, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn pending_for_test() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            receiver,
            _test_sender: Some(sender),
        }
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(request_id: RequestId, track_id: AppleMusicId) -> Self {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(PlaybackResponse::completed_for_test(
                request_id, track_id,
            )))
            .expect("the test playback receiver remains live during construction");
        Self {
            receiver,
            _test_sender: None,
        }
    }
}

/// Receives one terminal result from a user-triggered MusicKit transport command.
pub(crate) struct TransportTask {
    receiver: mpsc::Receiver<Result<TransportResponse, BridgeCallError>>,
    #[cfg(test)]
    _test_sender: Option<mpsc::Sender<Result<TransportResponse, BridgeCallError>>>,
}

impl TransportTask {
    #[cfg(all(amp808_music_kit_bridge, not(test)))]
    pub(crate) fn start(
        request_id: RequestId,
        track_id: &AppleMusicId,
        command: AppleTransportCommand,
    ) -> Result<Self, BridgeCallError> {
        let track_id = track_id.as_str().as_bytes();
        let (sender, receiver) = mpsc::channel();
        let context = Box::new(TransportCallbackContext { sender });
        let context = Box::into_raw(context).cast::<c_void>();
        // SAFETY: the identifier bytes remain live for the synchronous bridge call. Swift copies
        // them before returning, and the unique callback context is consumed exactly once.
        let accepted = unsafe {
            amp808_music_kit_transport(
                request_id.value(),
                match command {
                    AppleTransportCommand::Pause => 0,
                    AppleTransportCommand::Resume => 1,
                    AppleTransportCommand::Stop => 2,
                },
                track_id.as_ptr(),
                track_id.len(),
                context,
                transport_callback,
            )
        };
        if accepted != 1 {
            // SAFETY: a rejected request did not retain the unique context allocation.
            drop(unsafe { Box::from_raw(context.cast::<TransportCallbackContext>()) });
            return Err(match accepted {
                2 => BridgeCallError::MissingUsageDescription,
                3 => BridgeCallError::UnexpectedBundleIdentifier,
                _ => BridgeCallError::StartRejected,
            });
        }
        Ok(Self {
            receiver,
            #[cfg(test)]
            _test_sender: None,
        })
    }

    #[cfg(all(not(amp808_music_kit_bridge), not(test)))]
    pub(crate) const fn start(
        _request_id: RequestId,
        _track_id: &AppleMusicId,
        _command: AppleTransportCommand,
    ) -> Result<Self, BridgeCallError> {
        Err(BridgeCallError::Unavailable)
    }

    pub(crate) fn try_recv(
        &self,
    ) -> Result<Result<TransportResponse, BridgeCallError>, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(
        request_id: RequestId,
        track_id: AppleMusicId,
        status: AppleTransportStatus,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(TransportResponse::completed_for_test(
                request_id, track_id, status,
            )))
            .expect("the test transport receiver remains live during construction");
        Self {
            receiver,
            _test_sender: None,
        }
    }
}

/// Consumes one native callback context and sends one validated terminal result.
///
/// # Safety
///
/// `context` must be either null or the unique pointer produced from one
/// `Box<ConnectionCallbackContext>`. A non-null `pointer` must address `length` initialized bytes
/// allocated for exactly one release through `release`. The caller must invoke this function at
/// most once for each context and buffer.
unsafe fn deliver_connection(
    context: *mut c_void,
    pointer: *mut u8,
    length: usize,
    release: BridgeRelease,
) {
    let Some(context) = NonNull::new(context.cast::<ConnectionCallbackContext>()) else {
        if !pointer.is_null() {
            // SAFETY: the native callback transfers its unique response allocation even when the
            // opaque Rust context is unexpectedly absent.
            unsafe { release(pointer) };
        }
        return;
    };
    // SAFETY: the function contract requires the unique pointer created by `Box::into_raw`, and
    // this one-shot callback consumes it exactly once.
    let context = unsafe { Box::from_raw(context.as_ptr()) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the callback contract transfers the matching bridge allocation and release
        // operation to this one-shot Rust owner.
        let buffer = unsafe { OwnedBridgeBuffer::from_raw_parts(pointer, length, release) }
            .map_err(|_error| BridgeCallError::MissingResponse)?;
        let bytes = buffer
            .as_bounded_bytes()
            .map_err(BridgeCallError::Protocol)?;
        ConnectionResponse::parse(bytes).map_err(BridgeCallError::Protocol)
    }))
    .unwrap_or(Err(BridgeCallError::CallbackPanicked));
    let _ = context.sender.send(result);
}

/// Consumes one native playlist callback context and sends one validated page.
///
/// # Safety
///
/// `context` must be null or the unique pointer produced from one
/// `Box<PlaylistCallbackContext>`. A non-null `pointer` must address `length` initialized bytes
/// allocated for exactly one release through `release`. This function may be called at most once
/// for each context and buffer.
#[cfg(not(test))]
unsafe fn deliver_playlist_page(
    context: *mut c_void,
    pointer: *mut u8,
    length: usize,
    release: BridgeRelease,
) {
    let Some(context) = NonNull::new(context.cast::<PlaylistCallbackContext>()) else {
        if !pointer.is_null() {
            // SAFETY: the callback transfers its unique response allocation even without a Rust
            // context, so the matching release operation must consume it.
            unsafe { release(pointer) };
        }
        return;
    };
    // SAFETY: the FFI contract transfers the unique context back exactly once.
    let context = unsafe { Box::from_raw(context.as_ptr()) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the callback transfers one matching bridge allocation to this owner.
        let buffer = unsafe { OwnedBridgeBuffer::from_raw_parts(pointer, length, release) }
            .map_err(|_error| BridgeCallError::MissingResponse)?;
        let bytes = buffer
            .as_bounded_bytes()
            .map_err(BridgeCallError::Protocol)?;
        PlaylistPageResponse::parse(bytes).map_err(BridgeCallError::Protocol)
    }))
    .unwrap_or(Err(BridgeCallError::CallbackPanicked));
    let _ = context.sender.send(result);
}

#[cfg(not(test))]
unsafe fn deliver_track_page(
    context: *mut c_void,
    pointer: *mut u8,
    length: usize,
    release: BridgeRelease,
) {
    let Some(context) = NonNull::new(context.cast::<TrackCallbackContext>()) else {
        if !pointer.is_null() {
            // SAFETY: the callback transfers its unique response allocation.
            unsafe { release(pointer) };
        }
        return;
    };
    // SAFETY: the FFI contract transfers the unique context back exactly once.
    let context = unsafe { Box::from_raw(context.as_ptr()) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the callback transfers one matching bridge allocation to this owner.
        let buffer = unsafe { OwnedBridgeBuffer::from_raw_parts(pointer, length, release) }
            .map_err(|_error| BridgeCallError::MissingResponse)?;
        let bytes = buffer
            .as_bounded_bytes()
            .map_err(BridgeCallError::Protocol)?;
        TrackPageResponse::parse(bytes).map_err(BridgeCallError::Protocol)
    }))
    .unwrap_or(Err(BridgeCallError::CallbackPanicked));
    let _ = context.sender.send(result);
}

#[cfg(not(test))]
unsafe fn deliver_playback(
    context: *mut c_void,
    pointer: *mut u8,
    length: usize,
    release: BridgeRelease,
) {
    let Some(context) = NonNull::new(context.cast::<PlaybackCallbackContext>()) else {
        if !pointer.is_null() {
            // SAFETY: the callback transfers its unique response allocation.
            unsafe { release(pointer) };
        }
        return;
    };
    // SAFETY: the FFI contract transfers the unique context back exactly once.
    let context = unsafe { Box::from_raw(context.as_ptr()) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the callback transfers one matching bridge allocation to this owner.
        let buffer = unsafe { OwnedBridgeBuffer::from_raw_parts(pointer, length, release) }
            .map_err(|_error| BridgeCallError::MissingResponse)?;
        let bytes = buffer
            .as_bounded_bytes()
            .map_err(BridgeCallError::Protocol)?;
        PlaybackResponse::parse(bytes).map_err(BridgeCallError::Protocol)
    }))
    .unwrap_or(Err(BridgeCallError::CallbackPanicked));
    let _ = context.sender.send(result);
}

#[cfg(not(test))]
unsafe fn deliver_transport(
    context: *mut c_void,
    pointer: *mut u8,
    length: usize,
    release: BridgeRelease,
) {
    let Some(context) = NonNull::new(context.cast::<TransportCallbackContext>()) else {
        if !pointer.is_null() {
            // SAFETY: the callback transfers its unique response allocation.
            unsafe { release(pointer) };
        }
        return;
    };
    // SAFETY: the FFI contract transfers the unique context back exactly once.
    let context = unsafe { Box::from_raw(context.as_ptr()) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the callback transfers one matching bridge allocation to this owner.
        let buffer = unsafe { OwnedBridgeBuffer::from_raw_parts(pointer, length, release) }
            .map_err(|_error| BridgeCallError::MissingResponse)?;
        let bytes = buffer
            .as_bounded_bytes()
            .map_err(BridgeCallError::Protocol)?;
        TransportResponse::parse(bytes).map_err(BridgeCallError::Protocol)
    }))
    .unwrap_or(Err(BridgeCallError::CallbackPanicked));
    let _ = context.sender.send(result);
}

#[cfg(amp808_music_kit_bridge)]
unsafe extern "C" {
    fn amp808_music_kit_capability_probe(request_id: u64, output_length: *mut usize) -> *mut u8;
    fn amp808_music_kit_request_connection(
        request_id: u64,
        context: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, *mut u8, usize),
    ) -> u8;
    #[cfg(not(test))]
    fn amp808_music_kit_request_playlist_page(
        request_id: u64,
        offset: u64,
        context: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, *mut u8, usize),
    ) -> u8;
    #[cfg(not(test))]
    fn amp808_music_kit_request_track_page(
        request_id: u64,
        playlist_id: *const u8,
        playlist_id_length: usize,
        offset: u64,
        context: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, *mut u8, usize),
    ) -> u8;
    #[cfg(not(test))]
    fn amp808_music_kit_play_track(
        request_id: u64,
        playlist_id: *const u8,
        playlist_id_length: usize,
        track_id: *const u8,
        track_id_length: usize,
        context: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, *mut u8, usize),
    ) -> u8;
    #[cfg(not(test))]
    fn amp808_music_kit_transport(
        request_id: u64,
        command: u8,
        track_id: *const u8,
        track_id_length: usize,
        context: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, *mut u8, usize),
    ) -> u8;
    fn amp808_music_kit_release_buffer(pointer: *mut u8);
}

#[cfg(amp808_music_kit_bridge)]
unsafe extern "C" fn connection_callback(context: *mut c_void, pointer: *mut u8, length: usize) {
    // SAFETY: the Swift export invokes this exactly once with the unique context supplied by
    // `ConnectionTask::start` and transfers its matching response allocation to Rust.
    unsafe { deliver_connection(context, pointer, length, amp808_music_kit_release_buffer) };
}

#[cfg(all(amp808_music_kit_bridge, not(test)))]
unsafe extern "C" fn playlist_page_callback(context: *mut c_void, pointer: *mut u8, length: usize) {
    // SAFETY: Swift invokes this exactly once with the unique context supplied by
    // `PlaylistTask::start` and transfers its matching response allocation to Rust.
    unsafe { deliver_playlist_page(context, pointer, length, amp808_music_kit_release_buffer) };
}

#[cfg(all(amp808_music_kit_bridge, not(test)))]
unsafe extern "C" fn track_page_callback(context: *mut c_void, pointer: *mut u8, length: usize) {
    // SAFETY: Swift invokes this exactly once with the unique context supplied by
    // `TrackTask::start` and transfers its matching response allocation to Rust.
    unsafe { deliver_track_page(context, pointer, length, amp808_music_kit_release_buffer) };
}

#[cfg(all(amp808_music_kit_bridge, not(test)))]
unsafe extern "C" fn playback_callback(context: *mut c_void, pointer: *mut u8, length: usize) {
    // SAFETY: Swift invokes this exactly once with the unique context supplied by
    // `PlaybackTask::start` and transfers its matching response allocation to Rust.
    unsafe { deliver_playback(context, pointer, length, amp808_music_kit_release_buffer) };
}

#[cfg(all(amp808_music_kit_bridge, not(test)))]
unsafe extern "C" fn transport_callback(context: *mut c_void, pointer: *mut u8, length: usize) {
    // SAFETY: Swift invokes this exactly once with the unique context supplied by
    // `TransportTask::start` and transfers its matching response allocation to Rust.
    unsafe { deliver_transport(context, pointer, length, amp808_music_kit_release_buffer) };
}

#[cfg(amp808_music_kit_bridge)]
pub(super) fn probe_capability(
    request_id: RequestId,
) -> Result<CapabilityResponse, BridgeCallError> {
    let mut length = 0;
    // SAFETY: the output pointer addresses a live `usize`, and the exported Swift function only
    // writes the response length before returning its separately owned allocation.
    let pointer = unsafe { amp808_music_kit_capability_probe(request_id.value(), &mut length) };
    // SAFETY: on success the Swift export returns `length` initialized bytes that must be released
    // by `amp808_music_kit_release_buffer`; ownership is transferred to this RAII wrapper.
    let buffer = unsafe {
        OwnedBridgeBuffer::from_raw_parts(pointer, length, amp808_music_kit_release_buffer)
    }
    .map_err(|_error| BridgeCallError::MissingResponse)?;
    let bytes = buffer
        .as_bounded_bytes()
        .map_err(BridgeCallError::Protocol)?;
    CapabilityResponse::parse(bytes).map_err(BridgeCallError::Protocol)
}

#[cfg(not(amp808_music_kit_bridge))]
pub(super) const fn probe_capability(
    _request_id: RequestId,
) -> Result<CapabilityResponse, BridgeCallError> {
    Err(BridgeCallError::Unavailable)
}

#[cfg(test)]
mod tests {
    use std::{
        ptr,
        sync::atomic::{AtomicUsize, Ordering},
        sync::mpsc,
    };

    use super::{
        BridgeCallError, BridgeProtocolError, CapabilityResponse, ConnectionCallbackContext,
        ConnectionResponse, ConnectionTask, MAX_BRIDGE_ENVELOPE_BYTES, OwnedBridgeBuffer,
        PlaybackResponse, PlaylistPageResponse, SubscriptionResponse, TrackPageResponse,
        TransportResponse, deliver_connection, probe_capability,
    };
    use crate::apple_music::{
        AppleTransportStatus, AuthorisationStatus, CatalogPlayback, CloudLibrary, ConnectionResult,
        ConnectionSubscription, PageCursor, RequestId, SubscriptionCapabilities, SubscriptionOffer,
    };

    static RELEASE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static OVERSIZE_RELEASE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static NULL_RELEASE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static CONNECTION_RELEASE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static CONNECTION_BUFFER_LENGTH: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn release_test_buffer(pointer: *mut u8) {
        RELEASE_COUNT.fetch_add(1, Ordering::SeqCst);
        let slice = ptr::slice_from_raw_parts_mut(pointer, 12);
        // SAFETY: this callback receives the exact pointer and length produced by
        // `Box::into_raw` below, and the bridge owner invokes it exactly once.
        drop(unsafe { Box::from_raw(slice) });
    }

    unsafe extern "C" fn release_oversize_test_buffer(pointer: *mut u8) {
        OVERSIZE_RELEASE_COUNT.fetch_add(1, Ordering::SeqCst);
        let slice = ptr::slice_from_raw_parts_mut(pointer, MAX_BRIDGE_ENVELOPE_BYTES + 1);
        // SAFETY: this callback receives the matching boxed-slice allocation created by the
        // oversized-buffer test and is invoked once by the unique bridge owner.
        drop(unsafe { Box::from_raw(slice) });
    }

    unsafe extern "C" fn release_null_test_buffer(_pointer: *mut u8) {
        NULL_RELEASE_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn release_connection_test_buffer(pointer: *mut u8) {
        CONNECTION_RELEASE_COUNT.fetch_add(1, Ordering::SeqCst);
        let length = CONNECTION_BUFFER_LENGTH.load(Ordering::SeqCst);
        let slice = ptr::slice_from_raw_parts_mut(pointer, length);
        // SAFETY: the callback receives the exact pointer and length produced by the connection
        // callback test's boxed slice and is invoked by its unique buffer owner.
        drop(unsafe { Box::from_raw(slice) });
    }

    #[test]
    fn versioned_capability_envelope_maps_to_domain_values() {
        let response = CapabilityResponse::parse(
            br#"{"version":1,"kind":"capability","request_id":42,"authorisation":"not_determined"}"#,
        )
        .expect("a supported capability envelope should parse");

        assert_eq!(response.request_id(), RequestId::new(42));
        assert_eq!(response.authorisation(), AuthorisationStatus::NotDetermined);
    }

    #[test]
    fn versioned_subscription_envelope_preserves_independent_capabilities() {
        let response = SubscriptionResponse::parse(
            br#"{"version":1,"kind":"subscription","request_id":43,"can_play_catalog_content":false,"can_become_subscriber":true,"has_cloud_library_enabled":false}"#,
        )
        .expect("a supported subscription envelope should parse");

        assert_eq!(response.request_id(), RequestId::new(43));
        assert_eq!(
            response.capabilities().catalog_playback(),
            CatalogPlayback::Unavailable
        );
        assert_eq!(
            response.capabilities().subscription_offer(),
            SubscriptionOffer::Available
        );
        assert_eq!(
            response.capabilities().cloud_library(),
            CloudLibrary::Disabled
        );
    }

    #[test]
    fn versioned_connection_envelope_maps_one_terminal_result() {
        let response = ConnectionResponse::parse(
            br#"{"version":1,"kind":"connection","request_id":44,"authorisation":"authorised","subscription":"available","can_play_catalog_content":true,"can_become_subscriber":false,"has_cloud_library_enabled":true}"#,
        )
        .expect("a supported connection envelope should parse");
        let expected = SubscriptionCapabilities::new(
            CatalogPlayback::Available,
            SubscriptionOffer::Unavailable,
            CloudLibrary::Enabled,
        );

        assert_eq!(response.request_id(), RequestId::new(44));
        assert_eq!(
            response.result(),
            ConnectionResult::Authorised(ConnectionSubscription::Available(expected))
        );
    }

    #[test]
    fn playlist_page_envelope_maps_identity_artwork_and_pagination() {
        let response = PlaylistPageResponse::parse(
            br#"{"version":1,"kind":"playlist_page","request_id":48,"requested_cursor":null,"next_cursor":"2","playlists":[{"id":"p.cosmic","name":"Cosmic Orbit","artwork_url":"https://example.apple/artwork/300x300.jpg"},{"id":"p.no-art","name":"Signal Drift","artwork_url":null}]}"#,
        )
        .expect("a supported playlist page should parse");

        assert_eq!(response.request_id(), RequestId::new(48));
        assert_eq!(response.requested_cursor(), None);
        let page = response.into_page();
        assert_eq!(page.playlists().len(), 2);
        assert_eq!(page.playlists()[0].id().as_str(), "p.cosmic");
        assert_eq!(page.playlists()[0].name(), Some("Cosmic Orbit"));
        assert_eq!(
            page.playlists()[0].artwork_url(),
            Some("https://example.apple/artwork/300x300.jpg")
        );
        assert_eq!(page.playlists()[1].artwork_url(), None);
        assert_eq!(page.next_cursor().map(PageCursor::as_str), Some("2"));
    }

    #[test]
    fn playlist_page_omits_invalid_optional_artwork_without_echoing_private_data() {
        let response = PlaylistPageResponse::parse(
            br#"{"version":1,"kind":"playlist_page","request_id":49,"requested_cursor":null,"next_cursor":null,"playlists":[{"id":"p.private","name":"Private Mix","artwork_url":"https://"}]}"#,
        )
        .expect("invalid optional artwork must not discard a valid playlist page");

        let page = response.into_page();
        assert_eq!(page.playlists().len(), 1);
        assert_eq!(page.playlists()[0].artwork_url(), None);
    }

    #[test]
    fn track_page_envelope_preserves_playlist_entry_metadata_and_order() {
        let response = TrackPageResponse::parse(
            br#"{"version":1,"kind":"playlist_track_page","request_id":53,"playlist_id":"p.cosmic","requested_cursor":null,"next_cursor":"2","tracks":[{"id":"i.first","title":"First Light","artist":"Signal Artist","album":"Orbit One","duration_ms":183500},{"id":"i.second","title":"Second Sun","artist":"Signal Artist","album":null,"duration_ms":null}],"error":null}"#,
        )
        .expect("a supported track page should parse");

        assert_eq!(response.request_id(), RequestId::new(53));
        let page = response.into_page();
        assert_eq!(page.playlist_id.as_str(), "p.cosmic");
        assert_eq!(page.tracks.len(), 2);
        assert_eq!(page.tracks[0].title(), "First Light");
        assert_eq!(page.tracks[0].duration_ms(), Some(183_500));
        assert_eq!(page.tracks[1].album(), None);
        assert_eq!(
            page.next_cursor.map(|cursor| cursor.0),
            Some("2".to_owned())
        );
    }

    #[test]
    fn playback_envelope_preserves_request_and_track_identity() {
        let response = PlaybackResponse::parse(
            br#"{"version":1,"kind":"playback","request_id":57,"track_id":"i.first","status":"playing","error":null}"#,
        )
        .expect("a playing response should cross the bridge");

        assert_eq!(response.request_id(), RequestId::new(57));
        assert_eq!(response.into_track_id().as_str(), "i.first");
    }

    #[test]
    fn playback_failure_is_redacted_at_the_bridge_boundary() {
        let error = PlaybackResponse::parse(
            br#"{"version":1,"kind":"playback","request_id":58,"track_id":"i.private","status":"failed","error":"private provider detail"}"#,
        )
        .expect_err("a failed native handoff must not report playback success");

        assert!(matches!(error, BridgeProtocolError::PlaybackFailed));
        assert_eq!(
            error.to_string(),
            "Apple Music could not start MusicKit playback"
        );
        assert!(!error.to_string().contains("private provider detail"));
    }

    #[test]
    fn transport_envelopes_map_pause_resume_and_stop_without_losing_identity() {
        let paused = TransportResponse::parse(
            br#"{"version":1,"kind":"transport","request_id":59,"track_id":"i.first","status":"paused","error":null}"#,
        )
        .expect("a paused response should cross the bridge");
        assert_eq!(paused.request_id(), RequestId::new(59));
        assert_eq!(paused.status(), AppleTransportStatus::Paused);
        assert_eq!(paused.into_track_id().as_str(), "i.first");

        let playing = TransportResponse::parse(
            br#"{"version":1,"kind":"transport","request_id":60,"track_id":"i.first","status":"playing","error":null}"#,
        )
        .expect("a resumed response should cross the bridge");
        assert_eq!(playing.request_id(), RequestId::new(60));
        assert_eq!(playing.status(), AppleTransportStatus::Playing);
        assert_eq!(playing.into_track_id().as_str(), "i.first");

        let stopped = TransportResponse::parse(
            br#"{"version":1,"kind":"transport","request_id":62,"track_id":"i.first","status":"stopped","error":null}"#,
        )
        .expect("a stopped response should cross the bridge");
        assert_eq!(stopped.request_id(), RequestId::new(62));
        assert_eq!(stopped.status(), AppleTransportStatus::Stopped);
        assert_eq!(stopped.into_track_id().as_str(), "i.first");
    }

    #[test]
    fn transport_failure_is_redacted_at_the_bridge_boundary() {
        let error = TransportResponse::parse(
            br#"{"version":1,"kind":"transport","request_id":61,"track_id":"i.private","status":"failed","error":"private provider detail"}"#,
        )
        .expect_err("a failed transport command must not report success");

        assert!(matches!(error, BridgeProtocolError::TransportFailed));
        assert_eq!(error.to_string(), "Apple Music transport command failed");
        assert!(!error.to_string().contains("private provider detail"));
    }

    #[test]
    fn playlist_page_distinguishes_invalid_cursor_and_identifier_without_echoing_them() {
        let cursor_error = PlaylistPageResponse::parse(
            br#"{"version":1,"kind":"playlist_page","request_id":51,"requested_cursor":" ","next_cursor":null,"playlists":[]}"#,
        )
        .expect_err("a blank cursor must not enter the domain");
        assert!(matches!(
            cursor_error,
            BridgeProtocolError::InvalidPlaylistCursor
        ));

        let identifier_error = PlaylistPageResponse::parse(
            br#"{"version":1,"kind":"playlist_page","request_id":52,"requested_cursor":null,"next_cursor":null,"playlists":[{"id":" ","name":"Private Mix","artwork_url":null}]}"#,
        )
        .expect_err("a blank playlist identifier must not enter the domain");
        assert!(matches!(
            identifier_error,
            BridgeProtocolError::InvalidPlaylistIdentifier
        ));
        assert!(!identifier_error.to_string().contains("Private Mix"));
    }

    #[test]
    fn playlist_request_error_is_redacted_at_the_bridge_boundary() {
        let error = PlaylistPageResponse::parse(
            br#"{"version":1,"kind":"playlist_page","request_id":50,"requested_cursor":null,"next_cursor":null,"playlists":[],"error":"private provider detail"}"#,
        )
        .expect_err("a native request failure must not produce an empty library");

        assert!(matches!(error, BridgeProtocolError::PlaylistRequestFailed));
        assert_eq!(
            error.to_string(),
            "Apple Music could not load the playlist library"
        );
        assert!(!error.to_string().contains("private provider detail"));
    }

    #[test]
    fn connection_envelope_rejects_inconsistent_subscription_fields() {
        let missing_flags = ConnectionResponse::parse(
            br#"{"version":1,"kind":"connection","request_id":46,"authorisation":"authorised","subscription":"available","can_play_catalog_content":null,"can_become_subscriber":null,"has_cloud_library_enabled":null}"#,
        );
        assert!(matches!(
            missing_flags,
            Err(BridgeProtocolError::InconsistentConnection)
        ));

        let denied_with_subscription = ConnectionResponse::parse(
            br#"{"version":1,"kind":"connection","request_id":47,"authorisation":"denied","subscription":"available","can_play_catalog_content":true,"can_become_subscriber":false,"has_cloud_library_enabled":true}"#,
        );
        assert!(matches!(
            denied_with_subscription,
            Err(BridgeProtocolError::InconsistentConnection)
        ));
    }

    #[test]
    fn connection_callback_consumes_context_and_releases_buffer_once() {
        CONNECTION_RELEASE_COUNT.store(0, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        let context = Box::into_raw(Box::new(ConnectionCallbackContext { sender })).cast();
        let bytes = br#"{"version":1,"kind":"connection","request_id":45,"authorisation":"denied","subscription":"not_applicable","can_play_catalog_content":null,"can_become_subscriber":null,"has_cloud_library_enabled":null}"#
            .to_vec()
            .into_boxed_slice();
        let length = bytes.len();
        CONNECTION_BUFFER_LENGTH.store(length, Ordering::SeqCst);
        let pointer = Box::into_raw(bytes).cast::<u8>();

        // SAFETY: the context and buffer are unique allocations created above. The matching test
        // release callback reconstructs the buffer, and `deliver_connection` consumes the context.
        unsafe { deliver_connection(context, pointer, length, release_connection_test_buffer) };

        let response = receiver
            .recv()
            .expect("the one-shot callback should send one terminal result")
            .expect("the callback fixture should be a valid connection envelope");
        assert_eq!(response.request_id(), RequestId::new(45));
        assert_eq!(response.result(), ConnectionResult::Denied);
        assert_eq!(CONNECTION_RELEASE_COUNT.load(Ordering::SeqCst), 1);
        assert!(receiver.try_recv().is_err(), "only one result may be sent");
    }

    #[test]
    fn bridge_owned_buffer_releases_its_allocation_exactly_once() {
        RELEASE_COUNT.store(0, Ordering::SeqCst);
        let bytes = b"bridge-owned".to_vec().into_boxed_slice();
        let length = bytes.len();
        let pointer = Box::into_raw(bytes).cast::<u8>();

        {
            // SAFETY: `pointer` addresses `length` initialized bytes allocated by the matching
            // `release_test_buffer` callback, and ownership is transferred to this wrapper.
            let buffer =
                unsafe { OwnedBridgeBuffer::from_raw_parts(pointer, length, release_test_buffer) }
                    .expect("a non-null bridge allocation should be owned");
            assert_eq!(
                buffer
                    .as_bounded_bytes()
                    .expect("the fixture buffer is below the protocol limit"),
                b"bridge-owned"
            );
        }

        assert_eq!(RELEASE_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn native_capability_probe_round_trips_its_request_identity() {
        let request_id = RequestId::new(73);

        let response = probe_capability(request_id)
            .expect("the linked MusicKit bridge should return a capability envelope");

        assert_eq!(response.request_id(), request_id);
    }

    #[test]
    fn native_connection_rejects_unbundled_test_process_without_prompting() {
        let result = ConnectionTask::start(RequestId::new(74));

        assert!(matches!(
            result,
            Err(BridgeCallError::MissingUsageDescription)
        ));
    }

    #[test]
    fn capability_parser_rejects_invalid_utf8_version_kind_and_status() {
        assert!(matches!(
            CapabilityResponse::parse(&[0xff]),
            Err(BridgeProtocolError::InvalidUtf8(_))
        ));
        assert!(matches!(
            CapabilityResponse::parse(
                br#"{"version":2,"kind":"capability","request_id":1,"authorisation":"denied"}"#,
            ),
            Err(BridgeProtocolError::UnsupportedVersion)
        ));
        assert!(matches!(
            CapabilityResponse::parse(
                br#"{"version":1,"kind":"playlist","request_id":1,"authorisation":"denied"}"#,
            ),
            Err(BridgeProtocolError::UnexpectedKind)
        ));
        assert!(matches!(
            CapabilityResponse::parse(
                br#"{"version":1,"kind":"capability","request_id":1,"authorisation":"surprise"}"#,
            ),
            Err(BridgeProtocolError::UnknownAuthorisation)
        ));
    }

    #[test]
    fn oversized_bridge_allocation_is_released_without_creating_a_slice() {
        OVERSIZE_RELEASE_COUNT.store(0, Ordering::SeqCst);
        let bytes = vec![0; MAX_BRIDGE_ENVELOPE_BYTES + 1].into_boxed_slice();
        let length = bytes.len();
        let pointer = Box::into_raw(bytes).cast::<u8>();

        {
            // SAFETY: the pointer and length describe the boxed slice above, and the matching
            // release callback reconstructs that allocation exactly once.
            let buffer = unsafe {
                OwnedBridgeBuffer::from_raw_parts(pointer, length, release_oversize_test_buffer)
            }
            .expect("a non-null oversized allocation should still be owned for cleanup");
            assert!(matches!(
                buffer.as_bounded_bytes(),
                Err(BridgeProtocolError::EnvelopeTooLarge { .. })
            ));
        }

        assert_eq!(OVERSIZE_RELEASE_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn malformed_envelope_error_does_not_echo_private_payload() {
        let private_payload = br#"{"private_library_id":"secret-identifier""#;

        let error = CapabilityResponse::parse(private_payload)
            .expect_err("malformed JSON must not enter the domain");
        let message = error.to_string();

        assert!(!message.contains("secret-identifier"));
        assert_eq!(
            message,
            "the MusicKit bridge response is not a valid envelope"
        );
    }

    #[test]
    fn null_bridge_response_is_rejected_without_calling_release() {
        NULL_RELEASE_COUNT.store(0, Ordering::SeqCst);

        // SAFETY: the null pointer is intentionally supplied to exercise validation; no ownership
        // exists to transfer and the release callback must therefore remain unused.
        let result = unsafe {
            OwnedBridgeBuffer::from_raw_parts(ptr::null_mut(), 0, release_null_test_buffer)
        };

        assert!(result.is_err());
        assert_eq!(NULL_RELEASE_COUNT.load(Ordering::SeqCst), 0);
    }
}
