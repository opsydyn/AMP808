//! Desktop-private Apple Music domain contract.
//!
//! This module implements the pure Rust boundary defined by ADR-0035. Platform APIs, GPUI,
//! persistence, and local PCM playback belong outside this module.

use std::{collections::BTreeSet, error::Error, fmt, mem, sync::mpsc::TryRecvError};

#[cfg(test)]
use std::collections::VecDeque;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
use macos::{BridgeCallError, ConnectionTask, PlaylistTask};

#[cfg(not(target_os = "macos"))]
mod unavailable {
    use std::{error::Error, fmt, sync::mpsc};

    use super::{ConnectionResult, PageCursor, PlaylistPage, RequestId};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct BridgeCallError;

    impl fmt::Display for BridgeCallError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("the MusicKit bridge is unavailable on this platform")
        }
    }

    impl Error for BridgeCallError {}

    pub(crate) struct ConnectionResponse {
        request_id: RequestId,
        result: ConnectionResult,
    }

    impl ConnectionResponse {
        pub(crate) const fn request_id(&self) -> RequestId {
            self.request_id
        }

        pub(crate) const fn result(&self) -> ConnectionResult {
            self.result
        }
    }

    pub(crate) struct ConnectionTask {
        receiver: Option<mpsc::Receiver<Result<ConnectionResponse, BridgeCallError>>>,
        #[cfg(test)]
        _test_sender: Option<mpsc::Sender<Result<ConnectionResponse, BridgeCallError>>>,
    }

    impl ConnectionTask {
        pub(crate) const fn start(_request_id: RequestId) -> Result<Self, BridgeCallError> {
            Err(BridgeCallError)
        }

        pub(crate) fn try_recv(
            &self,
        ) -> Result<Result<ConnectionResponse, BridgeCallError>, mpsc::TryRecvError> {
            self.receiver
                .as_ref()
                .map_or(Err(mpsc::TryRecvError::Disconnected), |receiver| {
                    receiver.try_recv()
                })
        }

        #[cfg(test)]
        pub(crate) fn pending_for_test() -> Self {
            let (sender, receiver) = mpsc::channel();
            Self {
                receiver: Some(receiver),
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
                receiver: Some(receiver),
                _test_sender: None,
            }
        }
    }

    pub(crate) struct PlaylistPageResponse {
        request_id: RequestId,
        requested_cursor: Option<PageCursor>,
        page: PlaylistPage,
    }

    impl PlaylistPageResponse {
        pub(crate) const fn request_id(&self) -> RequestId {
            self.request_id
        }

        pub(crate) const fn requested_cursor(&self) -> Option<&PageCursor> {
            self.requested_cursor.as_ref()
        }

        pub(crate) fn into_page(self) -> PlaylistPage {
            self.page
        }
    }

    pub(crate) struct PlaylistTask {
        receiver: Option<mpsc::Receiver<Result<PlaylistPageResponse, BridgeCallError>>>,
        #[cfg(test)]
        _test_sender: Option<mpsc::Sender<Result<PlaylistPageResponse, BridgeCallError>>>,
    }

    impl PlaylistTask {
        pub(crate) const fn start(
            _request_id: RequestId,
            _requested_cursor: Option<&PageCursor>,
        ) -> Result<Self, BridgeCallError> {
            Err(BridgeCallError)
        }

        pub(crate) fn try_recv(
            &self,
        ) -> Result<Result<PlaylistPageResponse, BridgeCallError>, mpsc::TryRecvError> {
            self.receiver
                .as_ref()
                .map_or(Err(mpsc::TryRecvError::Disconnected), |receiver| {
                    receiver.try_recv()
                })
        }

        #[cfg(test)]
        pub(crate) fn pending_for_test() -> Self {
            let (sender, receiver) = mpsc::channel();
            Self {
                receiver: Some(receiver),
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
                .send(Ok(PlaylistPageResponse {
                    request_id,
                    requested_cursor,
                    page,
                }))
                .expect("the test playlist receiver remains live during construction");
            Self {
                receiver: Some(receiver),
                _test_sender: None,
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
use unavailable::{BridgeCallError, ConnectionTask, PlaylistTask};

/// Stores an opaque, non-empty identifier returned by Apple Music.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct AppleMusicId(String);

impl AppleMusicId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for AppleMusicId {
    type Error = InvalidAppleMusicId;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(InvalidAppleMusicId);
        }

        Ok(Self(value.to_owned()))
    }
}

/// Reports an empty or whitespace-only Apple Music identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidAppleMusicId;

impl fmt::Display for InvalidAppleMusicId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Apple Music identifier cannot be blank")
    }
}

impl Error for InvalidAppleMusicId {}

/// Stores an opaque, non-empty cursor used for Apple Music pagination.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct PageCursor(String);

impl PageCursor {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for PageCursor {
    type Error = InvalidPageCursor;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(InvalidPageCursor);
        }

        Ok(Self(value.to_owned()))
    }
}

/// Reports an empty or whitespace-only Apple Music page cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidPageCursor;

impl fmt::Display for InvalidPageCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Apple Music page cursor cannot be blank")
    }
}

impl Error for InvalidPageCursor {}

/// Stores one validated HTTPS artwork URL returned by MusicKit.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct AppleArtworkUrl(String);

impl AppleArtworkUrl {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for AppleArtworkUrl {
    type Error = InvalidAppleArtworkUrl;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let Some(resource) = value.strip_prefix("https://") else {
            return Err(InvalidAppleArtworkUrl);
        };
        if resource.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(InvalidAppleArtworkUrl);
        }

        Ok(Self(value.to_owned()))
    }
}

/// Reports an artwork URL that is not a non-empty HTTPS resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidAppleArtworkUrl;

impl fmt::Display for InvalidAppleArtworkUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Apple Music artwork URL must use HTTPS")
    }
}

impl Error for InvalidAppleArtworkUrl {}

/// Identifies one asynchronous request within the Apple Music coordinator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RequestId(u64);

impl RequestId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

/// Describes the user-visible Apple Music session state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AppleMusicState {
    Unavailable,
    Unauthorised,
    Authorising { request_id: RequestId },
    AccessDenied,
    AccessRestricted,
    AccessRevoked,
    Ready,
    LoadingPlaylists { request_id: RequestId },
    LibraryEmpty,
    LibraryLoaded { playlists: Vec<ApplePlaylist> },
}

/// Carries the identity and display name needed to present one Apple playlist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ApplePlaylist {
    id: AppleMusicId,
    name: Option<String>,
    artwork_url: Option<AppleArtworkUrl>,
}

impl ApplePlaylist {
    pub(crate) fn new(id: AppleMusicId, name: Option<String>) -> Self {
        Self {
            id,
            name,
            artwork_url: None,
        }
    }

    pub(crate) fn with_artwork(
        id: AppleMusicId,
        name: Option<String>,
        artwork_url: Option<AppleArtworkUrl>,
    ) -> Self {
        Self {
            id,
            name,
            artwork_url,
        }
    }

    pub(crate) const fn id(&self) -> &AppleMusicId {
        &self.id
    }

    pub(crate) fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub(crate) fn artwork_url(&self) -> Option<&str> {
        self.artwork_url.as_ref().map(AppleArtworkUrl::as_str)
    }
}

/// Contains one validated page of Apple Music playlists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaylistPage {
    playlists: Vec<ApplePlaylist>,
    next_cursor: Option<PageCursor>,
}

impl PlaylistPage {
    pub(crate) fn new(playlists: Vec<ApplePlaylist>, next_cursor: Option<PageCursor>) -> Self {
        Self {
            playlists,
            next_cursor,
        }
    }

    pub(crate) fn playlists(&self) -> &[ApplePlaylist] {
        &self.playlists
    }

    pub(crate) const fn next_cursor(&self) -> Option<&PageCursor> {
        self.next_cursor.as_ref()
    }
}

/// Describes whether pagination needs another page or has produced a complete result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PageProgress {
    More(PageCursor),
    Complete(Vec<ApplePlaylist>),
}

/// Reports a pagination sequence that cannot produce a trustworthy complete collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PaginationError {
    UnexpectedCursor {
        expected: Option<PageCursor>,
        received: Option<PageCursor>,
    },
    RepeatedCursor(PageCursor),
    AlreadyComplete,
}

impl fmt::Display for PaginationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnexpectedCursor { .. } => "playlist page cursor did not match the request",
            Self::RepeatedCursor(_) => "playlist pagination repeated a cursor",
            Self::AlreadyComplete => "playlist pagination is already complete",
        };
        formatter.write_str(message)
    }
}

impl Error for PaginationError {}

/// Accumulates Apple Music playlist pages while enforcing cursor progression.
#[derive(Debug, Default)]
pub(crate) struct PlaylistPager {
    expected_cursor: Option<PageCursor>,
    seen_cursors: BTreeSet<PageCursor>,
    playlists: Vec<ApplePlaylist>,
    completed: bool,
}

impl PlaylistPager {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn accept_page(
        &mut self,
        requested_cursor: Option<&PageCursor>,
        page: PlaylistPage,
    ) -> Result<PageProgress, PaginationError> {
        if self.completed {
            return Err(PaginationError::AlreadyComplete);
        }
        if requested_cursor != self.expected_cursor.as_ref() {
            return Err(PaginationError::UnexpectedCursor {
                expected: self.expected_cursor.clone(),
                received: requested_cursor.cloned(),
            });
        }
        if let Some(next_cursor) = page.next_cursor.as_ref()
            && (requested_cursor == Some(next_cursor) || self.seen_cursors.contains(next_cursor))
        {
            return Err(PaginationError::RepeatedCursor(next_cursor.clone()));
        }

        if let Some(cursor) = requested_cursor {
            self.seen_cursors.insert(cursor.clone());
        }
        self.playlists.extend(page.playlists);

        let Some(next_cursor) = page.next_cursor else {
            self.completed = true;
            self.expected_cursor = None;
            return Ok(PageProgress::Complete(mem::take(&mut self.playlists)));
        };

        self.expected_cursor = Some(next_cursor.clone());
        Ok(PageProgress::More(next_cursor))
    }
}

/// Describes the result returned by the native Apple Music authorisation flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorisationOutcome {
    Authorised,
    Denied,
    Restricted,
}

/// Describes the current MusicKit authorization observed without requesting consent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorisationStatus {
    NotDetermined,
    Denied,
    Restricted,
    Authorised,
    Unknown,
}

/// Describes whether the current Apple Music account can play catalog content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogPlayback {
    Unavailable,
    Available,
}

/// Describes whether Apple can present a subscription offer for the current account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubscriptionOffer {
    Unavailable,
    Available,
}

/// Describes whether the current account has its cloud music library enabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CloudLibrary {
    Disabled,
    Enabled,
}

/// Contains the independent capabilities returned by MusicKit's subscription query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SubscriptionCapabilities {
    catalog_playback: CatalogPlayback,
    subscription_offer: SubscriptionOffer,
    cloud_library: CloudLibrary,
}

impl SubscriptionCapabilities {
    pub(crate) const fn new(
        catalog_playback: CatalogPlayback,
        subscription_offer: SubscriptionOffer,
        cloud_library: CloudLibrary,
    ) -> Self {
        Self {
            catalog_playback,
            subscription_offer,
            cloud_library,
        }
    }

    pub(crate) const fn catalog_playback(self) -> CatalogPlayback {
        self.catalog_playback
    }

    pub(crate) const fn subscription_offer(self) -> SubscriptionOffer {
        self.subscription_offer
    }

    pub(crate) const fn cloud_library(self) -> CloudLibrary {
        self.cloud_library
    }
}

/// Tracks subscription inspection separately from media-library authorization and loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubscriptionState {
    NotChecked,
    Checking { request_id: RequestId },
    Available(SubscriptionCapabilities),
    Unavailable,
}

/// Describes subscription information returned with an authorised connection result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionSubscription {
    Available(SubscriptionCapabilities),
    Unavailable,
}

/// Describes one terminal result from the user-triggered native connection flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionResult {
    NotDetermined,
    Denied,
    Restricted,
    Authorised(ConnectionSubscription),
    Unknown,
}

/// Distinguishes an applied event from a safely ignored obsolete callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransitionOutcome {
    Applied,
    IgnoredStale,
}

/// Reports a command that is not legal in the current session state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AppleMusicTransitionError;

impl fmt::Display for AppleMusicTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("command is not valid for the current Apple Music state")
    }
}

impl Error for AppleMusicTransitionError {}

/// Owns pure Apple Music lifecycle state without platform or presentation dependencies.
#[derive(Debug)]
pub(crate) struct AppleMusicSession {
    state: AppleMusicState,
    subscription: SubscriptionState,
}

impl AppleMusicSession {
    pub(crate) const fn new() -> Self {
        Self {
            state: AppleMusicState::Unauthorised,
            subscription: SubscriptionState::NotChecked,
        }
    }

    pub(crate) const fn state(&self) -> &AppleMusicState {
        &self.state
    }

    pub(crate) const fn subscription(&self) -> &SubscriptionState {
        &self.subscription
    }

    pub(crate) fn observe_authorisation(&mut self, status: AuthorisationStatus) {
        self.state = match status {
            AuthorisationStatus::NotDetermined => AppleMusicState::Unauthorised,
            AuthorisationStatus::Denied => AppleMusicState::AccessDenied,
            AuthorisationStatus::Restricted => AppleMusicState::AccessRestricted,
            AuthorisationStatus::Authorised => AppleMusicState::Ready,
            AuthorisationStatus::Unknown => AppleMusicState::Unavailable,
        };
        self.subscription = SubscriptionState::NotChecked;
    }

    pub(crate) fn begin_authorisation(
        &mut self,
        request_id: RequestId,
    ) -> Result<(), AppleMusicTransitionError> {
        if !matches!(
            self.state,
            AppleMusicState::Unavailable
                | AppleMusicState::Unauthorised
                | AppleMusicState::AccessDenied
                | AppleMusicState::AccessRestricted
                | AppleMusicState::AccessRevoked
        ) {
            return Err(AppleMusicTransitionError);
        }

        self.state = AppleMusicState::Authorising { request_id };
        Ok(())
    }

    pub(crate) fn resolve_authorisation(
        &mut self,
        request_id: RequestId,
        outcome: AuthorisationOutcome,
    ) -> Result<TransitionOutcome, AppleMusicTransitionError> {
        let AppleMusicState::Authorising {
            request_id: active_request,
        } = self.state
        else {
            return Err(AppleMusicTransitionError);
        };

        if request_id != active_request {
            return Ok(TransitionOutcome::IgnoredStale);
        }

        match outcome {
            AuthorisationOutcome::Authorised => self.state = AppleMusicState::Ready,
            AuthorisationOutcome::Denied => {
                self.state = AppleMusicState::AccessDenied;
                self.subscription = SubscriptionState::NotChecked;
            }
            AuthorisationOutcome::Restricted => {
                self.state = AppleMusicState::AccessRestricted;
                self.subscription = SubscriptionState::NotChecked;
            }
        }
        Ok(TransitionOutcome::Applied)
    }

    pub(crate) fn resolve_connection(
        &mut self,
        request_id: RequestId,
        result: ConnectionResult,
    ) -> Result<TransitionOutcome, AppleMusicTransitionError> {
        let AppleMusicState::Authorising {
            request_id: active_request,
        } = self.state
        else {
            return Err(AppleMusicTransitionError);
        };

        if request_id != active_request {
            return Ok(TransitionOutcome::IgnoredStale);
        }

        match result {
            ConnectionResult::NotDetermined => {
                self.state = AppleMusicState::Unauthorised;
                self.subscription = SubscriptionState::NotChecked;
            }
            ConnectionResult::Denied => {
                self.state = AppleMusicState::AccessDenied;
                self.subscription = SubscriptionState::NotChecked;
            }
            ConnectionResult::Restricted => {
                self.state = AppleMusicState::AccessRestricted;
                self.subscription = SubscriptionState::NotChecked;
            }
            ConnectionResult::Authorised(subscription) => {
                self.state = AppleMusicState::Ready;
                self.subscription = match subscription {
                    ConnectionSubscription::Available(capabilities) => {
                        SubscriptionState::Available(capabilities)
                    }
                    ConnectionSubscription::Unavailable => SubscriptionState::Unavailable,
                };
            }
            ConnectionResult::Unknown => {
                self.state = AppleMusicState::Unavailable;
                self.subscription = SubscriptionState::NotChecked;
            }
        }

        Ok(TransitionOutcome::Applied)
    }

    pub(crate) fn begin_subscription_check(
        &mut self,
        request_id: RequestId,
    ) -> Result<(), AppleMusicTransitionError> {
        if self.state != AppleMusicState::Ready {
            return Err(AppleMusicTransitionError);
        }

        self.subscription = SubscriptionState::Checking { request_id };
        Ok(())
    }

    pub(crate) fn resolve_subscription_check(
        &mut self,
        request_id: RequestId,
        capabilities: SubscriptionCapabilities,
    ) -> Result<TransitionOutcome, AppleMusicTransitionError> {
        let SubscriptionState::Checking {
            request_id: active_request,
        } = self.subscription
        else {
            return Err(AppleMusicTransitionError);
        };

        if request_id != active_request {
            return Ok(TransitionOutcome::IgnoredStale);
        }

        self.subscription = SubscriptionState::Available(capabilities);
        Ok(TransitionOutcome::Applied)
    }

    pub(crate) fn fail_subscription_check(
        &mut self,
        request_id: RequestId,
    ) -> Result<TransitionOutcome, AppleMusicTransitionError> {
        let SubscriptionState::Checking {
            request_id: active_request,
        } = self.subscription
        else {
            return Err(AppleMusicTransitionError);
        };

        if request_id != active_request {
            return Ok(TransitionOutcome::IgnoredStale);
        }

        self.subscription = SubscriptionState::Unavailable;
        Ok(TransitionOutcome::Applied)
    }

    pub(crate) fn begin_playlist_load(
        &mut self,
        request_id: RequestId,
    ) -> Result<(), AppleMusicTransitionError> {
        if self.state != AppleMusicState::Ready {
            return Err(AppleMusicTransitionError);
        }

        self.state = AppleMusicState::LoadingPlaylists { request_id };
        Ok(())
    }

    pub(crate) fn complete_playlist_load(
        &mut self,
        request_id: RequestId,
        playlists: Vec<ApplePlaylist>,
    ) -> Result<TransitionOutcome, AppleMusicTransitionError> {
        let AppleMusicState::LoadingPlaylists {
            request_id: active_request,
        } = self.state
        else {
            return Err(AppleMusicTransitionError);
        };

        if request_id != active_request {
            return Ok(TransitionOutcome::IgnoredStale);
        }

        self.state = if playlists.is_empty() {
            AppleMusicState::LibraryEmpty
        } else {
            AppleMusicState::LibraryLoaded { playlists }
        };
        Ok(TransitionOutcome::Applied)
    }

    pub(crate) fn fail_playlist_load(
        &mut self,
        request_id: RequestId,
    ) -> Result<TransitionOutcome, AppleMusicTransitionError> {
        let AppleMusicState::LoadingPlaylists {
            request_id: active_request,
        } = self.state
        else {
            return Err(AppleMusicTransitionError);
        };

        if request_id != active_request {
            return Ok(TransitionOutcome::IgnoredStale);
        }

        self.state = AppleMusicState::Ready;
        Ok(TransitionOutcome::Applied)
    }

    pub(crate) fn revoke_access(&mut self) {
        self.state = AppleMusicState::AccessRevoked;
        self.subscription = SubscriptionState::NotChecked;
    }
}

/// Coordinates user-triggered native requests without leaking bridge types into GPUI.
pub(crate) struct AppleMusicCoordinator {
    session: AppleMusicSession,
    connection: Option<ConnectionTask>,
    playlist_task: Option<PlaylistTask>,
    playlist_page_request_id: Option<RequestId>,
    playlist_load_id: Option<RequestId>,
    playlist_pager: Option<PlaylistPager>,
    request_sequence: u64,
    error: Option<String>,
    #[cfg(test)]
    test_connection: Option<TestConnection>,
    #[cfg(test)]
    test_connection_launches: usize,
    #[cfg(test)]
    test_playlist_pages: VecDeque<TestPlaylistPage>,
    #[cfg(test)]
    test_playlist_launches: usize,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TestConnection {
    Pending,
    Completed(ConnectionResult),
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TestPlaylistPage {
    Pending,
    Completed(PlaylistPage),
}

impl AppleMusicCoordinator {
    pub(crate) const fn new() -> Self {
        Self {
            session: AppleMusicSession::new(),
            connection: None,
            playlist_task: None,
            playlist_page_request_id: None,
            playlist_load_id: None,
            playlist_pager: None,
            request_sequence: 0,
            error: None,
            #[cfg(test)]
            test_connection: None,
            #[cfg(test)]
            test_connection_launches: 0,
            #[cfg(test)]
            test_playlist_pages: VecDeque::new(),
            #[cfg(test)]
            test_playlist_launches: 0,
        }
    }

    pub(crate) const fn session(&self) -> &AppleMusicSession {
        &self.session
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn connect(&mut self) {
        if self.connection.is_some() {
            return;
        }
        let request_id = self.next_request_id();
        if let Err(error) = self.session.begin_authorisation(request_id) {
            self.error = Some(error.to_string());
            return;
        }

        match self.start_connection(request_id) {
            Ok(connection) => {
                self.connection = Some(connection);
                self.error = None;
            }
            Err(error) => {
                let _ = self
                    .session
                    .resolve_connection(request_id, ConnectionResult::Unknown);
                self.error = Some(error.to_string());
            }
        }
    }

    pub(crate) fn poll(&mut self) -> bool {
        if self.connection.is_some() {
            self.poll_connection();
            return self.connection.is_some() || self.playlist_task.is_some();
        }
        self.poll_playlist_page();
        self.playlist_task.is_some()
    }

    fn poll_connection(&mut self) {
        let event = match self.connection.as_ref().map(ConnectionTask::try_recv) {
            Some(Ok(event)) => Some(event),
            Some(Err(TryRecvError::Empty)) | None => return,
            Some(Err(TryRecvError::Disconnected)) => {
                self.fail_active("Apple Music connection channel disconnected".to_owned());
                self.connection.take();
                return;
            }
        };
        let Some(event) = event else { return };

        match event {
            Ok(response) => {
                let result = response.result();
                match self
                    .session
                    .resolve_connection(response.request_id(), result)
                {
                    Ok(TransitionOutcome::Applied) => {
                        self.error = None;
                        if matches!(result, ConnectionResult::Authorised(_)) {
                            self.begin_playlist_loading();
                        }
                    }
                    Ok(TransitionOutcome::IgnoredStale) => {}
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            Err(error) => self.fail_active(error.to_string()),
        }
        self.connection.take();
    }

    fn begin_playlist_loading(&mut self) {
        let request_id = self.next_request_id();
        if let Err(error) = self.session.begin_playlist_load(request_id) {
            self.error = Some(error.to_string());
            return;
        }
        self.playlist_load_id = Some(request_id);
        self.playlist_pager = Some(PlaylistPager::new());
        self.start_playlist_page(request_id, None);
    }

    fn poll_playlist_page(&mut self) {
        let event = match self.playlist_task.as_ref().map(PlaylistTask::try_recv) {
            Some(Ok(event)) => Some(event),
            Some(Err(TryRecvError::Empty)) | None => return,
            Some(Err(TryRecvError::Disconnected)) => {
                self.fail_playlist_loading("Apple Music playlist channel disconnected".to_owned());
                return;
            }
        };
        let Some(event) = event else { return };
        self.playlist_task.take();

        let response = match event {
            Ok(response) => response,
            Err(error) => {
                self.fail_playlist_loading(error.to_string());
                return;
            }
        };
        if Some(response.request_id()) != self.playlist_page_request_id {
            self.fail_playlist_loading("Apple Music returned an obsolete playlist page".to_owned());
            return;
        }
        let requested_cursor = response.requested_cursor().cloned();
        let Some(pager) = self.playlist_pager.as_mut() else {
            self.fail_playlist_loading(
                "Apple Music playlist paging state is unavailable".to_owned(),
            );
            return;
        };
        let progress = match pager.accept_page(requested_cursor.as_ref(), response.into_page()) {
            Ok(progress) => progress,
            Err(error) => {
                self.fail_playlist_loading(error.to_string());
                return;
            }
        };

        match progress {
            PageProgress::More(cursor) => {
                let request_id = self.next_request_id();
                self.start_playlist_page(request_id, Some(cursor));
            }
            PageProgress::Complete(playlists) => {
                let Some(load_id) = self.playlist_load_id.take() else {
                    self.fail_playlist_loading(
                        "Apple Music playlist load identity is unavailable".to_owned(),
                    );
                    return;
                };
                match self.session.complete_playlist_load(load_id, playlists) {
                    Ok(TransitionOutcome::Applied | TransitionOutcome::IgnoredStale) => {
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
                self.playlist_page_request_id = None;
                self.playlist_pager = None;
            }
        }
    }

    fn start_playlist_page(&mut self, request_id: RequestId, requested_cursor: Option<PageCursor>) {
        match self.create_playlist_task(request_id, requested_cursor.as_ref()) {
            Ok(task) => {
                self.playlist_page_request_id = Some(request_id);
                self.playlist_task = Some(task);
            }
            Err(error) => self.fail_playlist_loading(error.to_string()),
        }
    }

    fn fail_playlist_loading(&mut self, error: String) {
        if let Some(load_id) = self.playlist_load_id.take() {
            let _ = self.session.fail_playlist_load(load_id);
        }
        self.playlist_task.take();
        self.playlist_page_request_id = None;
        self.playlist_pager = None;
        self.error = Some(error);
    }

    fn next_request_id(&mut self) -> RequestId {
        self.request_sequence = self.request_sequence.saturating_add(1);
        RequestId::new(self.request_sequence)
    }

    fn fail_active(&mut self, error: String) {
        if let AppleMusicState::Authorising { request_id } = self.session.state() {
            let _ = self
                .session
                .resolve_connection(*request_id, ConnectionResult::Unknown);
        }
        self.error = Some(error);
    }

    #[cfg(not(test))]
    fn start_connection(
        &mut self,
        request_id: RequestId,
    ) -> Result<ConnectionTask, BridgeCallError> {
        ConnectionTask::start(request_id)
    }

    #[cfg(test)]
    fn start_connection(
        &mut self,
        request_id: RequestId,
    ) -> Result<ConnectionTask, BridgeCallError> {
        self.test_connection_launches = self.test_connection_launches.saturating_add(1);
        Ok(
            match self.test_connection.unwrap_or(TestConnection::Pending) {
                TestConnection::Pending => ConnectionTask::pending_for_test(),
                TestConnection::Completed(result) => {
                    ConnectionTask::completed_for_test(request_id, result)
                }
            },
        )
    }

    #[cfg(not(test))]
    fn create_playlist_task(
        &mut self,
        request_id: RequestId,
        requested_cursor: Option<&PageCursor>,
    ) -> Result<PlaylistTask, BridgeCallError> {
        PlaylistTask::start(request_id, requested_cursor)
    }

    #[cfg(test)]
    fn create_playlist_task(
        &mut self,
        request_id: RequestId,
        requested_cursor: Option<&PageCursor>,
    ) -> Result<PlaylistTask, BridgeCallError> {
        self.test_playlist_launches = self.test_playlist_launches.saturating_add(1);
        Ok(
            match self
                .test_playlist_pages
                .pop_front()
                .unwrap_or(TestPlaylistPage::Pending)
            {
                TestPlaylistPage::Pending => PlaylistTask::pending_for_test(),
                TestPlaylistPage::Completed(page) => {
                    PlaylistTask::completed_for_test(request_id, requested_cursor.cloned(), page)
                }
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn use_test_connection(&mut self, connection: TestConnection) {
        self.test_connection = Some(connection);
    }

    #[cfg(test)]
    pub(crate) fn use_test_playlist_pages(&mut self, pages: Vec<TestPlaylistPage>) {
        self.test_playlist_pages = pages.into();
    }

    #[cfg(test)]
    pub(crate) const fn test_connection_launches(&self) -> usize {
        self.test_connection_launches
    }

    #[cfg(test)]
    pub(crate) const fn test_playlist_launches(&self) -> usize {
        self.test_playlist_launches
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppleMusicCoordinator, AppleMusicId, AppleMusicSession, AppleMusicState, ApplePlaylist,
        AuthorisationOutcome, AuthorisationStatus, CatalogPlayback, CloudLibrary, ConnectionResult,
        ConnectionSubscription, PageCursor, PageProgress, PaginationError, PlaylistPage,
        PlaylistPager, RequestId, SubscriptionCapabilities, SubscriptionOffer, SubscriptionState,
        TestConnection, TestPlaylistPage, TransitionOutcome,
    };

    fn authorised_session() -> AppleMusicSession {
        let mut session = AppleMusicSession::new();
        let request_id = RequestId::new(1);
        session
            .begin_authorisation(request_id)
            .expect("authorisation should start");
        session
            .resolve_authorisation(request_id, AuthorisationOutcome::Authorised)
            .expect("authorisation should complete");
        session
    }

    #[test]
    fn unauthorised_session_can_begin_native_authorisation() {
        let mut session = AppleMusicSession::new();
        let request_id = RequestId::new(7);

        assert_eq!(session.state(), &AppleMusicState::Unauthorised);

        session
            .begin_authorisation(request_id)
            .expect("an unauthorised session should begin authorisation");

        assert_eq!(
            session.state(),
            &AppleMusicState::Authorising { request_id }
        );
    }

    #[test]
    fn stale_authorisation_result_cannot_replace_the_active_request() {
        let mut session = AppleMusicSession::new();
        let active_request = RequestId::new(8);
        session
            .begin_authorisation(active_request)
            .expect("authorisation should start");

        let outcome = session
            .resolve_authorisation(RequestId::new(7), AuthorisationOutcome::Authorised)
            .expect("a stale result should be safely classified");

        assert_eq!(outcome, TransitionOutcome::IgnoredStale);
        assert_eq!(
            session.state(),
            &AppleMusicState::Authorising {
                request_id: active_request,
            },
            "an obsolete callback must not replace the active request state"
        );
    }

    #[test]
    fn denied_authorisation_can_be_retried_with_a_new_request() {
        let mut session = AppleMusicSession::new();
        session
            .begin_authorisation(RequestId::new(9))
            .expect("authorisation should start");

        session
            .resolve_authorisation(RequestId::new(9), AuthorisationOutcome::Denied)
            .expect("the active denial should be applied");
        assert_eq!(session.state(), &AppleMusicState::AccessDenied);

        let retry_request = RequestId::new(10);
        session
            .begin_authorisation(retry_request)
            .expect("the user should be able to retry after denial");
        assert_eq!(
            session.state(),
            &AppleMusicState::Authorising {
                request_id: retry_request,
            }
        );
    }

    #[test]
    fn restricted_authorisation_has_a_distinct_recoverable_state() {
        let mut session = AppleMusicSession::new();
        let request_id = RequestId::new(15);
        session
            .begin_authorisation(request_id)
            .expect("authorisation should start");

        session
            .resolve_authorisation(request_id, AuthorisationOutcome::Restricted)
            .expect("the active restriction should be applied");

        assert_eq!(session.state(), &AppleMusicState::AccessRestricted);
    }

    #[test]
    fn catalog_subscription_capabilities_do_not_disable_library_access() {
        let mut session = authorised_session();
        let request_id = RequestId::new(16);
        session
            .begin_subscription_check(request_id)
            .expect("an authorised session should inspect subscription capabilities");

        assert_eq!(
            session.subscription(),
            &SubscriptionState::Checking { request_id }
        );

        let capabilities = SubscriptionCapabilities::new(
            CatalogPlayback::Unavailable,
            SubscriptionOffer::Available,
            CloudLibrary::Enabled,
        );
        session
            .resolve_subscription_check(request_id, capabilities)
            .expect("the active subscription result should be applied");

        assert_eq!(session.state(), &AppleMusicState::Ready);
        assert_eq!(
            session.subscription(),
            &SubscriptionState::Available(capabilities)
        );
    }

    #[test]
    fn failed_subscription_check_does_not_revoke_library_access() {
        let mut session = authorised_session();
        let request_id = RequestId::new(17);
        session
            .begin_subscription_check(request_id)
            .expect("an authorised session should inspect subscription capabilities");

        session
            .fail_subscription_check(request_id)
            .expect("the active subscription failure should be applied");

        assert_eq!(session.state(), &AppleMusicState::Ready);
        assert_eq!(session.subscription(), &SubscriptionState::Unavailable);
    }

    #[test]
    fn subscription_capabilities_preserve_each_apple_flag_independently() {
        let capabilities = SubscriptionCapabilities::new(
            CatalogPlayback::Available,
            SubscriptionOffer::Unavailable,
            CloudLibrary::Disabled,
        );

        assert_eq!(capabilities.catalog_playback(), CatalogPlayback::Available);
        assert_eq!(
            capabilities.subscription_offer(),
            SubscriptionOffer::Unavailable
        );
        assert_eq!(capabilities.cloud_library(), CloudLibrary::Disabled);
    }

    #[test]
    fn observing_current_authorisation_never_enters_the_prompting_state() {
        let mut session = AppleMusicSession::new();

        session.observe_authorisation(AuthorisationStatus::Restricted);

        assert_eq!(session.state(), &AppleMusicState::AccessRestricted);
        assert!(!matches!(
            session.state(),
            AppleMusicState::Authorising { .. }
        ));
    }

    #[test]
    fn authorised_connection_applies_subscription_capabilities_atomically() {
        let mut session = AppleMusicSession::new();
        let request_id = RequestId::new(18);
        session
            .begin_authorisation(request_id)
            .expect("the user-triggered connection should begin");
        let capabilities = SubscriptionCapabilities::new(
            CatalogPlayback::Available,
            SubscriptionOffer::Unavailable,
            CloudLibrary::Enabled,
        );

        let outcome = session
            .resolve_connection(
                request_id,
                ConnectionResult::Authorised(ConnectionSubscription::Available(capabilities)),
            )
            .expect("the matching native result should apply");

        assert_eq!(outcome, TransitionOutcome::Applied);
        assert_eq!(session.state(), &AppleMusicState::Ready);
        assert_eq!(
            session.subscription(),
            &SubscriptionState::Available(capabilities)
        );
    }

    #[test]
    fn unavailable_connection_can_be_retried_by_the_user() {
        let mut session = AppleMusicSession::new();
        let failed_request = RequestId::new(19);
        session
            .begin_authorisation(failed_request)
            .expect("the first connection should begin");
        session
            .resolve_connection(failed_request, ConnectionResult::Unknown)
            .expect("the bridge failure should become unavailable");

        let retry_request = RequestId::new(20);
        session
            .begin_authorisation(retry_request)
            .expect("an unavailable bridge result should remain retryable");

        assert_eq!(
            session.state(),
            &AppleMusicState::Authorising {
                request_id: retry_request
            }
        );
    }

    #[test]
    fn stale_connection_result_cannot_replace_the_active_request() {
        let mut session = AppleMusicSession::new();
        let active_request = RequestId::new(22);
        session
            .begin_authorisation(active_request)
            .expect("the active connection should begin");

        let outcome = session
            .resolve_connection(RequestId::new(21), ConnectionResult::Denied)
            .expect("a stale connection result should be classified safely");

        assert_eq!(outcome, TransitionOutcome::IgnoredStale);
        assert_eq!(
            session.state(),
            &AppleMusicState::Authorising {
                request_id: active_request
            }
        );
    }

    #[test]
    fn apple_identifiers_are_non_empty_but_otherwise_remain_opaque() {
        assert!(
            AppleMusicId::try_from("  \n").is_err(),
            "blank identifiers must not enter the domain"
        );

        let source = "p.ABC-123_/opaque";
        let id = AppleMusicId::try_from(source).expect("opaque Apple identifier should be valid");

        assert_eq!(id.as_str(), source);
    }

    #[test]
    fn an_authorised_empty_library_has_an_explicit_empty_state() {
        let mut session = authorised_session();
        let request_id = RequestId::new(11);

        session
            .begin_playlist_load(request_id)
            .expect("an authorised session should load playlists");
        assert_eq!(
            session.state(),
            &AppleMusicState::LoadingPlaylists { request_id }
        );

        let outcome = session
            .complete_playlist_load(request_id, Vec::new())
            .expect("an empty library is a successful result");

        assert_eq!(outcome, TransitionOutcome::Applied);
        assert_eq!(session.state(), &AppleMusicState::LibraryEmpty);
    }

    #[test]
    fn a_loaded_library_preserves_opaque_identity_and_optional_metadata() {
        let mut session = authorised_session();
        let request_id = RequestId::new(12);
        session
            .begin_playlist_load(request_id)
            .expect("an authorised session should load playlists");
        let playlist = ApplePlaylist::new(
            AppleMusicId::try_from("p.cosmic-orbit").expect("fixture id should be valid"),
            Some("Cosmic Orbit".to_owned()),
        );

        session
            .complete_playlist_load(request_id, vec![playlist])
            .expect("a non-empty library should load");

        let AppleMusicState::LibraryLoaded { playlists } = session.state() else {
            panic!("expected loaded playlists, got {:?}", session.state());
        };
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].id().as_str(), "p.cosmic-orbit");
        assert_eq!(playlists[0].name(), Some("Cosmic Orbit"));
    }

    #[test]
    fn revoked_access_clears_library_data_and_can_reauthorise() {
        let mut session = authorised_session();
        let load_request = RequestId::new(13);
        session
            .begin_playlist_load(load_request)
            .expect("playlist loading should start");
        session
            .complete_playlist_load(
                load_request,
                vec![ApplePlaylist::new(
                    AppleMusicId::try_from("p.private-library")
                        .expect("fixture id should be valid"),
                    None,
                )],
            )
            .expect("playlist loading should complete");

        session.revoke_access();

        assert_eq!(session.state(), &AppleMusicState::AccessRevoked);
        let retry_request = RequestId::new(14);
        session
            .begin_authorisation(retry_request)
            .expect("revoked access should be recoverable through authorisation");
        assert_eq!(
            session.state(),
            &AppleMusicState::Authorising {
                request_id: retry_request,
            }
        );
    }

    #[test]
    fn repeated_playlist_cursor_is_rejected_without_accepting_its_items() {
        let first = ApplePlaylist::new(
            AppleMusicId::try_from("p.first").expect("fixture id should be valid"),
            Some("First".to_owned()),
        );
        let rejected = ApplePlaylist::new(
            AppleMusicId::try_from("p.rejected").expect("fixture id should be valid"),
            Some("Rejected".to_owned()),
        );
        let final_playlist = ApplePlaylist::new(
            AppleMusicId::try_from("p.final").expect("fixture id should be valid"),
            Some("Final".to_owned()),
        );
        let cursor = PageCursor::try_from("cursor-1").expect("fixture cursor should be valid");
        let mut pager = PlaylistPager::new();

        let progress = pager
            .accept_page(None, PlaylistPage::new(vec![first], Some(cursor.clone())))
            .expect("the first page should request the next cursor");
        assert_eq!(progress, PageProgress::More(cursor.clone()));

        let error = pager
            .accept_page(
                Some(&cursor),
                PlaylistPage::new(vec![rejected], Some(cursor.clone())),
            )
            .expect_err("a cursor must not point back to a page already requested");
        assert_eq!(error, PaginationError::RepeatedCursor(cursor.clone()));

        let progress = pager
            .accept_page(Some(&cursor), PlaylistPage::new(vec![final_playlist], None))
            .expect("the valid final page should still be accepted after rejection");
        let PageProgress::Complete(playlists) = progress else {
            panic!("expected completed pagination, got {progress:?}");
        };
        let ids: Vec<&str> = playlists
            .iter()
            .map(|playlist| playlist.id().as_str())
            .collect();
        assert_eq!(ids, ["p.first", "p.final"]);
    }

    #[test]
    fn connected_coordinator_loads_every_playlist_page_before_exposing_the_library() {
        let capabilities = SubscriptionCapabilities::new(
            CatalogPlayback::Available,
            SubscriptionOffer::Unavailable,
            CloudLibrary::Enabled,
        );
        let next_cursor = PageCursor::try_from("1").expect("fixture cursor should be valid");
        let first = ApplePlaylist::new(
            AppleMusicId::try_from("p.first").expect("fixture id should be valid"),
            Some("First Orbit".to_owned()),
        );
        let second = ApplePlaylist::new(
            AppleMusicId::try_from("p.second").expect("fixture id should be valid"),
            Some("Second Orbit".to_owned()),
        );
        let mut coordinator = AppleMusicCoordinator::new();
        coordinator.use_test_connection(TestConnection::Completed(ConnectionResult::Authorised(
            ConnectionSubscription::Available(capabilities),
        )));
        coordinator.use_test_playlist_pages(vec![
            TestPlaylistPage::Completed(PlaylistPage::new(vec![first], Some(next_cursor))),
            TestPlaylistPage::Completed(PlaylistPage::new(vec![second], None)),
        ]);

        coordinator.connect();
        assert!(
            coordinator.poll(),
            "playlist loading should follow connection"
        );
        assert!(matches!(
            coordinator.session().state(),
            AppleMusicState::LoadingPlaylists { .. }
        ));
        assert!(coordinator.poll(), "a next page should remain active");
        assert!(matches!(
            coordinator.session().state(),
            AppleMusicState::LoadingPlaylists { .. }
        ));
        assert!(
            !coordinator.poll(),
            "the final page should complete loading"
        );

        let AppleMusicState::LibraryLoaded { playlists } = coordinator.session().state() else {
            panic!(
                "expected a complete playlist library, got {:?}",
                coordinator.session().state()
            );
        };
        let names: Vec<Option<&str>> = playlists.iter().map(ApplePlaylist::name).collect();
        assert_eq!(names, [Some("First Orbit"), Some("Second Orbit")]);
        assert_eq!(coordinator.test_playlist_launches(), 2);
    }
}
