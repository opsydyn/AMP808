use std::path::Path;

/// Repeat mode for playlist looping behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    /// Cycle through Off → All → One → Off.
    pub fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }
}

impl std::fmt::Display for RepeatMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepeatMode::Off => write!(f, "Off"),
            RepeatMode::All => write!(f, "All"),
            RepeatMode::One => write!(f, "One"),
        }
    }
}

/// Metadata for a remote playlist (e.g. Navidrome/Subsonic).
#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub id: String,
    pub name: String,
    pub track_count: usize,
}

/// A provider that supplies browsable playlists from an external service.
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn playlists(&self) -> anyhow::Result<Vec<PlaylistInfo>>;
    fn tracks(&self, playlist_id: &str) -> anyhow::Result<Vec<Track>>;
}

/// A single audio track (local file or HTTP stream).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub stream: bool,
}

impl Track {
    /// Formatted display string: "Artist - Title" or just "Title".
    pub fn display_name(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artist, self.title)
        }
    }
}

/// Check if a path is an HTTP/HTTPS URL.
pub fn is_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// Check if a URL points to a site supported by yt-dlp.
pub fn is_ytdl(path: &str) -> bool {
    if !is_url(path) {
        return false;
    }
    let Ok(url) = url::Url::parse(path) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host);

    matches!(
        host,
        "soundcloud.com" | "youtube.com" | "youtu.be" | "music.youtube.com" | "bandcamp.com"
    ) || host.ends_with(".bandcamp.com")
}

/// Check if a URL points to a podcast RSS/XML feed.
pub fn is_feed(path: &str) -> bool {
    if !is_url(path) {
        return false;
    }
    let Ok(url) = url::Url::parse(path) else {
        return false;
    };
    let ext = Path::new(url.path())
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(ext.as_str(), "xml" | "rss" | "atom")
}

/// Check if a URL points to an M3U playlist file.
pub fn is_m3u(path: &str) -> bool {
    if !is_url(path) {
        return false;
    }
    let Ok(url) = url::Url::parse(path) else {
        return false;
    };
    let ext = Path::new(url.path())
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(ext.as_str(), "m3u" | "m3u8")
}

/// Create a Track from a file path or URL.
/// Supports "Artist - Title" filename format.
pub fn track_from_path(path: &str) -> Track {
    if is_url(path) {
        return track_from_url(path);
    }
    let base = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let name = Path::new(base)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or(base);

    if let Some((artist, title)) = name.split_once(" - ") {
        Track {
            path: path.to_string(),
            artist: artist.trim().to_string(),
            title: title.trim().to_string(),
            stream: false,
        }
    } else {
        Track {
            path: path.to_string(),
            artist: String::new(),
            title: name.to_string(),
            stream: false,
        }
    }
}

/// Create a Track from an HTTP/HTTPS URL.
fn track_from_url(raw_url: &str) -> Track {
    let mut t = Track {
        path: raw_url.to_string(),
        title: raw_url.to_string(),
        artist: String::new(),
        stream: true,
    };

    let Ok(url) = url::Url::parse(raw_url) else {
        return t;
    };

    let base = Path::new(url.path())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if !base.is_empty() && base != "." && base != "/" {
        let name = Path::new(base)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if !name.is_empty() && name != "stream" && name != "rest" {
            t.title = name.to_string();
            return t;
        }
    }

    // Fallback: use hostname
    if let Some(host) = url.host_str() {
        t.title = host.to_string();
    }
    t
}

/// Ordered track list with shuffle, repeat, and queue support.
pub struct Playlist {
    tracks: Vec<Track>,
    order: Vec<usize>,
    pos: usize,
    shuffle: bool,
    repeat: RepeatMode,
    queue: Vec<usize>,
    queued_idx: Option<usize>,
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            order: Vec::new(),
            pos: 0,
            shuffle: false,
            repeat: RepeatMode::Off,
            queue: Vec::new(),
            queued_idx: None,
        }
    }

    /// Append tracks to the playlist.
    pub fn add(&mut self, tracks: &[Track]) {
        let start = self.tracks.len();
        self.tracks.extend_from_slice(tracks);
        for i in start..self.tracks.len() {
            self.order.push(i);
        }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Current track and its index. Returns None if empty.
    pub fn current(&self) -> Option<(&Track, usize)> {
        if self.tracks.is_empty() {
            return None;
        }
        if let Some(qi) = self.queued_idx {
            return Some((&self.tracks[qi], qi));
        }
        let idx = self.order[self.pos];
        Some((&self.tracks[idx], idx))
    }

    /// Track index of the current position.
    pub fn index(&self) -> Option<usize> {
        if self.order.is_empty() {
            return None;
        }
        if let Some(qi) = self.queued_idx {
            return Some(qi);
        }
        Some(self.order[self.pos])
    }

    /// Advance to the next track. Returns None if at end with repeat off.
    /// Queued tracks are played first.
    pub fn next(&mut self) -> Option<(&Track, bool)> {
        if self.tracks.is_empty() {
            return None;
        }
        // Play from queue first
        if let Some(idx) = self.queue.first().copied() {
            self.queue.remove(0);
            self.queued_idx = Some(idx);
            return Some((&self.tracks[idx], true));
        }
        self.queued_idx = None;

        if self.repeat == RepeatMode::One {
            let idx = self.order[self.pos];
            return Some((&self.tracks[idx], true));
        }
        if self.pos + 1 < self.order.len() {
            self.pos += 1;
            let idx = self.order[self.pos];
            return Some((&self.tracks[idx], true));
        }
        if self.repeat == RepeatMode::All {
            self.pos = 0;
            if self.shuffle {
                self.do_shuffle();
            }
            let idx = self.order[self.pos];
            return Some((&self.tracks[idx], true));
        }
        None
    }

    /// Peek at the next track without advancing.
    pub fn peek_next(&self) -> Option<&Track> {
        if self.tracks.is_empty() {
            return None;
        }
        if let Some(&idx) = self.queue.first() {
            return Some(&self.tracks[idx]);
        }
        if self.repeat == RepeatMode::One {
            let idx = if let Some(qi) = self.queued_idx {
                qi
            } else {
                self.order[self.pos]
            };
            return Some(&self.tracks[idx]);
        }
        if self.pos + 1 < self.order.len() {
            return Some(&self.tracks[self.order[self.pos + 1]]);
        }
        if self.repeat == RepeatMode::All && !self.shuffle {
            return Some(&self.tracks[self.order[0]]);
        }
        None
    }

    /// Move to the previous track. Returns None if empty.
    pub fn prev(&mut self) -> Option<&Track> {
        self.queued_idx = None;
        if self.tracks.is_empty() {
            return None;
        }
        if self.pos > 0 {
            self.pos -= 1;
            return Some(&self.tracks[self.order[self.pos]]);
        }
        if self.repeat == RepeatMode::All {
            self.pos = self.order.len() - 1;
            return Some(&self.tracks[self.order[self.pos]]);
        }
        Some(&self.tracks[self.order[self.pos]])
    }

    /// Set current position to the given track index.
    pub fn set_index(&mut self, track_idx: usize) {
        self.queued_idx = None;
        for (pos, &idx) in self.order.iter().enumerate() {
            if idx == track_idx {
                self.pos = pos;
                return;
            }
        }
    }

    /// Replace the track at a given index.
    pub fn set_track(&mut self, i: usize, track: Track) {
        if i < self.tracks.len() {
            self.tracks[i] = track;
        }
    }

    /// Queue a track to play next.
    pub fn queue(&mut self, track_idx: usize) {
        if track_idx < self.tracks.len() {
            self.queue.push(track_idx);
        }
    }

    /// Remove a track from the queue. Returns true if found.
    pub fn dequeue(&mut self, track_idx: usize) -> bool {
        if let Some(pos) = self.queue.iter().position(|&i| i == track_idx) {
            self.queue.remove(pos);
            true
        } else {
            false
        }
    }

    /// 1-based queue position, or 0 if not queued.
    pub fn queue_position(&self, track_idx: usize) -> usize {
        self.queue
            .iter()
            .position(|&i| i == track_idx)
            .map(|p| p + 1)
            .unwrap_or(0)
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// All tracks in the playlist.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Toggle shuffle mode. Fisher-Yates shuffle preserving current track at pos 0.
    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.tracks.is_empty() {
            self.order.clear();
            self.pos = 0;
            self.queued_idx = None;
            return;
        }
        if self.shuffle {
            self.do_shuffle();
        } else {
            let cur = self.order[self.pos];
            self.order = (0..self.tracks.len()).collect();
            self.pos = cur;
        }
    }

    fn do_shuffle(&mut self) {
        use rand::Rng;
        let cur = self.order[self.pos];
        let mut others: Vec<usize> = (0..self.tracks.len()).filter(|&i| i != cur).collect();
        let mut rng = rand::rng();
        // Fisher-Yates
        for i in (1..others.len()).rev() {
            let j = rng.random_range(0..=i);
            others.swap(i, j);
        }
        self.order = Vec::with_capacity(self.tracks.len());
        self.order.push(cur);
        self.order.extend(others);
        self.pos = 0;
    }

    /// Cycle repeat mode.
    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
    }

    pub fn shuffled(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn make_track(title: &str) -> Track {
        Track {
            path: format!("/music/{title}.mp3"),
            title: title.to_string(),
            artist: String::new(),
            stream: false,
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct PlaylistSnapshot {
        order: Vec<usize>,
        pos: usize,
        shuffle: bool,
        repeat: RepeatMode,
        queue: Vec<usize>,
        queued_idx: Option<usize>,
    }

    #[derive(Clone, Debug)]
    enum PlaylistOp {
        ToggleShuffle,
        CycleRepeat,
        Next,
        Prev,
        PeekNext,
        SetIndex(usize),
        Queue(usize),
        Dequeue(usize),
    }

    fn snapshot(playlist: &Playlist) -> PlaylistSnapshot {
        PlaylistSnapshot {
            order: playlist.order.clone(),
            pos: playlist.pos,
            shuffle: playlist.shuffle,
            repeat: playlist.repeat,
            queue: playlist.queue.clone(),
            queued_idx: playlist.queued_idx,
        }
    }

    fn op_strategy() -> impl Strategy<Value = PlaylistOp> {
        prop_oneof![
            Just(PlaylistOp::ToggleShuffle),
            Just(PlaylistOp::CycleRepeat),
            Just(PlaylistOp::Next),
            Just(PlaylistOp::Prev),
            Just(PlaylistOp::PeekNext),
            (0usize..32).prop_map(PlaylistOp::SetIndex),
            (0usize..32).prop_map(PlaylistOp::Queue),
            (0usize..32).prop_map(PlaylistOp::Dequeue),
        ]
    }

    fn make_tracks(count: usize) -> Vec<Track> {
        (0..count).map(|i| make_track(&format!("T{i}"))).collect()
    }

    fn assert_playlist_invariants(playlist: &Playlist) {
        let len = playlist.tracks.len();
        assert_eq!(playlist.order.len(), len);
        assert_eq!(playlist.queue_len(), playlist.queue.len());
        assert!(playlist.queue.iter().all(|&idx| idx < len));

        if len == 0 {
            assert!(playlist.order.is_empty());
            assert_eq!(playlist.pos, 0);
            assert!(playlist.current().is_none());
            assert!(playlist.index().is_none());
            assert!(playlist.queue.is_empty());
            assert!(playlist.queued_idx.is_none());
            return;
        }

        assert!(playlist.pos < len);

        let mut seen = vec![false; len];
        for &idx in &playlist.order {
            assert!(idx < len);
            assert!(!seen[idx]);
            seen[idx] = true;
        }
        assert!(seen.into_iter().all(std::convert::identity));

        if !playlist.shuffle {
            let expected: Vec<usize> = (0..len).collect();
            assert_eq!(playlist.order, expected);
        }

        let expected_idx = playlist.queued_idx.unwrap_or(playlist.order[playlist.pos]);
        assert_eq!(playlist.index(), Some(expected_idx));
        let (_, current_idx) = playlist.current().expect("non-empty playlist has current");
        assert_eq!(current_idx, expected_idx);

        for idx in 0..len {
            let pos = playlist.queue_position(idx);
            if pos == 0 {
                assert!(!playlist.queue.contains(&idx));
            } else {
                assert_eq!(playlist.queue[pos - 1], idx);
            }
        }
    }

    fn apply_op(playlist: &mut Playlist, op: PlaylistOp) {
        match op {
            PlaylistOp::ToggleShuffle => playlist.toggle_shuffle(),
            PlaylistOp::CycleRepeat => playlist.cycle_repeat(),
            PlaylistOp::Next => {
                let _ = playlist.next();
            }
            PlaylistOp::Prev => {
                let _ = playlist.prev();
            }
            PlaylistOp::PeekNext => {
                let before = snapshot(playlist);
                let _ = playlist.peek_next();
                assert_eq!(snapshot(playlist), before);
            }
            PlaylistOp::SetIndex(idx) => playlist.set_index(idx),
            PlaylistOp::Queue(idx) => playlist.queue(idx),
            PlaylistOp::Dequeue(idx) => {
                let _ = playlist.dequeue(idx);
            }
        }
    }

    // --- URL detection tests ---

    #[test]
    fn test_is_url() {
        assert!(is_url("http://example.com/song.mp3"));
        assert!(is_url("https://example.com/song.mp3"));
        assert!(!is_url("/home/user/song.mp3"));
        assert!(!is_url("file:///song.mp3"));
    }

    #[test]
    fn test_is_ytdl() {
        assert!(is_ytdl("https://youtube.com/watch?v=abc123"));
        assert!(is_ytdl("https://www.youtube.com/watch?v=abc123"));
        assert!(is_ytdl("https://youtu.be/abc123"));
        assert!(is_ytdl("https://music.youtube.com/watch?v=abc123"));
        assert!(is_ytdl("https://soundcloud.com/artist/track"));
        assert!(is_ytdl("https://bandcamp.com/album"));
        assert!(is_ytdl("https://artist.bandcamp.com/album/cool"));
        assert!(is_ytdl("https://m.youtube.com/watch?v=abc123"));
        assert!(!is_ytdl("https://example.com/song.mp3"));
        assert!(!is_ytdl("/local/file.mp3"));
    }

    #[test]
    fn test_is_feed() {
        assert!(is_feed("https://example.com/podcast/feed.xml"));
        assert!(is_feed("https://example.com/feed.rss"));
        assert!(is_feed("https://example.com/feed.atom"));
        assert!(!is_feed("https://example.com/song.mp3"));
        assert!(!is_feed("/local/feed.xml"));
    }

    #[test]
    fn test_is_m3u() {
        assert!(is_m3u("https://example.com/playlist.m3u"));
        assert!(is_m3u("https://example.com/playlist.m3u8"));
        assert!(!is_m3u("https://example.com/song.mp3"));
        assert!(!is_m3u("/local/playlist.m3u"));
    }

    // --- Track creation tests ---

    #[test]
    fn test_track_from_path_with_artist() {
        let t = track_from_path("/music/Pink Floyd - Comfortably Numb.mp3");
        assert_eq!(t.artist, "Pink Floyd");
        assert_eq!(t.title, "Comfortably Numb");
        assert!(!t.stream);
    }

    #[test]
    fn test_track_from_path_without_artist() {
        let t = track_from_path("/music/song.flac");
        assert_eq!(t.artist, "");
        assert_eq!(t.title, "song");
        assert!(!t.stream);
    }

    #[test]
    fn test_track_from_url() {
        let t = track_from_path("https://example.com/audio/cool-song.mp3");
        assert_eq!(t.title, "cool-song");
        assert!(t.stream);
    }

    #[test]
    fn test_track_from_url_stream_fallback() {
        let t = track_from_path("https://radio.example.com/stream");
        assert_eq!(t.title, "radio.example.com");
        assert!(t.stream);
    }

    #[test]
    fn test_display_name() {
        let t = Track {
            path: String::new(),
            title: "Title".into(),
            artist: "Artist".into(),
            stream: false,
        };
        assert_eq!(t.display_name(), "Artist - Title");

        let t2 = Track {
            path: String::new(),
            title: "Title".into(),
            artist: String::new(),
            stream: false,
        };
        assert_eq!(t2.display_name(), "Title");
    }

    // --- Playlist navigation tests ---

    #[test]
    fn test_empty_playlist() {
        let pl = Playlist::new();
        assert!(pl.is_empty());
        assert_eq!(pl.len(), 0);
        assert!(pl.current().is_none());
        assert!(pl.index().is_none());
    }

    #[test]
    fn test_toggle_shuffle_on_empty_playlist_is_safe() {
        let mut pl = Playlist::new();
        pl.toggle_shuffle();
        assert!(pl.shuffled());
        assert_playlist_invariants(&pl);

        pl.toggle_shuffle();
        assert!(!pl.shuffled());
        assert_playlist_invariants(&pl);
    }

    #[test]
    fn test_add_and_current() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B"), make_track("C")]);
        assert_eq!(pl.len(), 3);
        let (track, idx) = pl.current().unwrap();
        assert_eq!(track.title, "A");
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_next_sequential() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B"), make_track("C")]);

        let (t, ok) = pl.next().unwrap();
        assert!(ok);
        assert_eq!(t.title, "B");

        let (t, ok) = pl.next().unwrap();
        assert!(ok);
        assert_eq!(t.title, "C");

        // At end with repeat off
        assert!(pl.next().is_none());
    }

    #[test]
    fn test_next_repeat_all() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B")]);
        pl.set_repeat(RepeatMode::All);

        pl.next(); // B
        let (t, _) = pl.next().unwrap(); // wraps to A
        assert_eq!(t.title, "A");
    }

    #[test]
    fn test_next_repeat_one() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B")]);
        pl.set_repeat(RepeatMode::One);

        let (t, _) = pl.next().unwrap();
        assert_eq!(t.title, "A"); // stays on A

        let (t, _) = pl.next().unwrap();
        assert_eq!(t.title, "A"); // still A
    }

    #[test]
    fn test_prev() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B"), make_track("C")]);
        pl.next(); // B
        pl.next(); // C

        let t = pl.prev().unwrap();
        assert_eq!(t.title, "B");

        let t = pl.prev().unwrap();
        assert_eq!(t.title, "A");

        // At start, stays on A
        let t = pl.prev().unwrap();
        assert_eq!(t.title, "A");
    }

    #[test]
    fn test_prev_repeat_all_wraps() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B")]);
        pl.set_repeat(RepeatMode::All);

        // At pos 0, prev wraps to last
        let t = pl.prev().unwrap();
        assert_eq!(t.title, "B");
    }

    #[test]
    fn test_peek_next() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B"), make_track("C")]);

        let t = pl.peek_next().unwrap();
        assert_eq!(t.title, "B");

        // Peek doesn't advance
        let (cur, _) = pl.current().unwrap();
        assert_eq!(cur.title, "A");
    }

    #[test]
    fn test_peek_next_at_end() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A")]);
        assert!(pl.peek_next().is_none()); // repeat off, no next
    }

    #[test]
    fn test_peek_next_repeat_all() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B")]);
        pl.set_repeat(RepeatMode::All);
        pl.next(); // move to B

        let t = pl.peek_next().unwrap();
        assert_eq!(t.title, "A"); // wraps
    }

    #[test]
    fn test_set_index() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B"), make_track("C")]);

        pl.set_index(2);
        let (t, idx) = pl.current().unwrap();
        assert_eq!(t.title, "C");
        assert_eq!(idx, 2);
    }

    #[test]
    fn test_set_track() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A")]);

        let replacement = Track {
            path: "/tmp/downloaded.mp3".into(),
            title: "Downloaded".into(),
            artist: "Artist".into(),
            stream: false,
        };
        pl.set_track(0, replacement);

        let (t, _) = pl.current().unwrap();
        assert_eq!(t.title, "Downloaded");
        assert_eq!(t.artist, "Artist");
    }

    // --- Queue tests ---

    #[test]
    fn test_queue_plays_first() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B"), make_track("C")]);

        pl.queue(2); // queue track C
        let (t, _) = pl.next().unwrap();
        assert_eq!(t.title, "C"); // queued track plays first

        let (t, _) = pl.next().unwrap();
        assert_eq!(t.title, "B"); // then normal order resumes
    }

    #[test]
    fn test_dequeue() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B")]);

        pl.queue(1);
        assert_eq!(pl.queue_len(), 1);
        assert_eq!(pl.queue_position(1), 1);

        assert!(pl.dequeue(1));
        assert_eq!(pl.queue_len(), 0);
        assert_eq!(pl.queue_position(1), 0);

        assert!(!pl.dequeue(1)); // already removed
    }

    // --- Shuffle tests ---

    #[test]
    fn test_toggle_shuffle_preserves_current() {
        let mut pl = Playlist::new();
        pl.add(&[
            make_track("A"),
            make_track("B"),
            make_track("C"),
            make_track("D"),
        ]);
        pl.next(); // move to B

        let (before, _) = pl.current().unwrap();
        let before_title = before.title.clone();

        pl.toggle_shuffle();

        let (after, _) = pl.current().unwrap();
        assert_eq!(after.title, before_title); // current track preserved
        assert!(pl.shuffled());
    }

    #[test]
    fn test_toggle_shuffle_off_restores_order() {
        let mut pl = Playlist::new();
        pl.add(&[make_track("A"), make_track("B"), make_track("C")]);

        pl.toggle_shuffle(); // on
        pl.toggle_shuffle(); // off

        assert!(!pl.shuffled());
        // After unshuffle, order is restored to sequential
        let (t, idx) = pl.current().unwrap();
        assert_eq!(idx, 0);
        assert_eq!(t.title, "A");
    }

    // --- Repeat cycle test ---

    #[test]
    fn test_cycle_repeat() {
        let mut pl = Playlist::new();
        assert_eq!(pl.repeat(), RepeatMode::Off);

        pl.cycle_repeat();
        assert_eq!(pl.repeat(), RepeatMode::All);

        pl.cycle_repeat();
        assert_eq!(pl.repeat(), RepeatMode::One);

        pl.cycle_repeat();
        assert_eq!(pl.repeat(), RepeatMode::Off);
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 64,
            .. ProptestConfig::default()
        })]

        #[test]
        fn prop_shuffle_round_trips_current_track(
            track_count in 0usize..20,
            current_idx in 0usize..64,
        ) {
            let mut playlist = Playlist::new();
            playlist.add(&make_tracks(track_count));
            if track_count > 0 {
                playlist.set_index(current_idx % track_count);
            }
            let before_idx = playlist.index();
            let before_repeat = playlist.repeat();

            playlist.toggle_shuffle();
            assert_playlist_invariants(&playlist);
            assert_eq!(playlist.repeat(), before_repeat);
            assert_eq!(playlist.index(), before_idx);

            playlist.toggle_shuffle();
            assert_playlist_invariants(&playlist);
            assert_eq!(playlist.repeat(), before_repeat);
            assert_eq!(playlist.index(), before_idx);
        }

        #[test]
        fn prop_playlist_ops_preserve_invariants(
            track_count in 0usize..20,
            ops in prop::collection::vec(op_strategy(), 0..128),
        ) {
            let mut playlist = Playlist::new();
            playlist.add(&make_tracks(track_count));
            assert_playlist_invariants(&playlist);

            for op in ops {
                apply_op(&mut playlist, op);
                assert_playlist_invariants(&playlist);
            }
        }
    }
}
