use std::path::{Path, PathBuf};

use crate::app::transport::TransportState;
use crate::library::LibraryTrack;

// ADR-0028: the queue is disposable session intent; only named playlists are persisted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QueuedTrack {
    path: PathBuf,
    title: String,
    artist: Option<String>,
    album: Option<String>,
}

impl QueuedTrack {
    pub(crate) fn new(
        path: PathBuf,
        title: String,
        artist: Option<String>,
        album: Option<String>,
    ) -> Self {
        Self {
            path,
            title,
            artist,
            album,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    pub(crate) fn album(&self) -> Option<&str> {
        self.album.as_deref()
    }
}

impl From<&LibraryTrack> for QueuedTrack {
    fn from(track: &LibraryTrack) -> Self {
        Self::new(
            track.path().to_path_buf(),
            track.title().to_owned(),
            track.artist().map(str::to_owned),
            track.album().map(str::to_owned),
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PlaybackQueue {
    entries: Vec<QueuedTrack>,
}

impl PlaybackQueue {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(crate) fn add(&mut self, track: QueuedTrack) {
        self.entries.push(track);
    }

    pub(crate) fn play_next(&mut self, track: QueuedTrack) {
        self.entries.insert(0, track);
    }

    pub(crate) fn entries(&self) -> &[QueuedTrack] {
        &self.entries
    }

    pub(crate) fn remove(&mut self, position: usize) -> Option<QueuedTrack> {
        (position < self.entries.len()).then(|| self.entries.remove(position))
    }

    pub(crate) fn take_next(&mut self) -> Option<QueuedTrack> {
        self.remove(0)
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QueueAdvance {
    ended_observed: bool,
}

impl QueueAdvance {
    pub(crate) const fn new() -> Self {
        Self {
            ended_observed: false,
        }
    }

    pub(crate) fn observe(&mut self, transport: &TransportState) -> bool {
        if transport == &TransportState::Ended {
            let should_advance = !self.ended_observed;
            self.ended_observed = true;
            should_advance
        } else {
            self.ended_observed = false;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::app::transport::TransportState;

    use super::{PlaybackQueue, QueueAdvance, QueuedTrack};

    fn track(title: &str) -> QueuedTrack {
        QueuedTrack::new(
            PathBuf::from(format!("/Music/{title}.wav")),
            title.to_owned(),
            None,
            None,
        )
    }

    #[test]
    fn play_next_prepends_while_add_appends_and_duplicates_remain_distinct() {
        let mut queue = PlaybackQueue::new();

        queue.add(track("Second Sun"));
        queue.add(track("Third Orbit"));
        queue.play_next(track("First Light"));
        queue.add(track("Second Sun"));

        assert_eq!(
            queue
                .entries()
                .iter()
                .map(QueuedTrack::title)
                .collect::<Vec<_>>(),
            ["First Light", "Second Sun", "Third Orbit", "Second Sun"]
        );
    }

    #[test]
    fn queue_removal_is_positional_and_take_next_consumes_only_the_front() {
        let mut queue = PlaybackQueue::new();
        queue.add(track("First Light"));
        queue.add(track("Second Sun"));
        queue.add(track("First Light"));

        assert_eq!(
            queue.remove(2).expect("third queue entry exists").title(),
            "First Light"
        );
        assert_eq!(
            queue.take_next().expect("front queue entry exists").title(),
            "First Light"
        );
        assert_eq!(queue.entries()[0].title(), "Second Sun");
        assert!(queue.remove(9).is_none());

        queue.clear();
        assert!(queue.is_empty());
        assert!(queue.take_next().is_none());
    }

    #[test]
    fn natural_end_advances_once_until_transport_leaves_ended() {
        let mut advance = QueueAdvance::new();

        assert!(!advance.observe(&TransportState::Paused));
        assert!(advance.observe(&TransportState::Ended));
        assert!(!advance.observe(&TransportState::Ended));
        assert!(!advance.observe(&TransportState::Playing));
        assert!(advance.observe(&TransportState::Ended));
        assert!(!advance.observe(&TransportState::Failed("decode".to_owned())));
    }
}
