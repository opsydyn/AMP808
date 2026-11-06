use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, params};

use crate::media::probe_metadata;

const SCHEMA_VERSION: i32 = 1;
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "aac", "alac", "flac", "m4a", "m4b", "mp3", "ogg", "opus", "wav", "wma",
];

#[derive(Clone, Debug)]
pub(crate) struct LibraryCatalogue {
    database_path: PathBuf,
}

impl LibraryCatalogue {
    pub(crate) fn open_default() -> Result<Self, LibraryError> {
        let data_directory = dirs::data_dir().ok_or(LibraryError::DataDirectoryUnavailable)?;
        Self::open(data_directory.join("AMP808-R3TR0").join("library.sqlite3"))
    }

    pub(crate) fn open(database_path: impl AsRef<Path>) -> Result<Self, LibraryError> {
        let database_path = database_path.as_ref().to_path_buf();
        if let Some(parent) = database_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LibraryError::io(parent.to_path_buf(), error))?;
        }
        let catalogue = Self { database_path };
        let connection = catalogue.connect()?;
        migrate(&connection)?;
        Ok(catalogue)
    }

    pub(crate) fn scan_root(&self, root: impl AsRef<Path>) -> Result<ScanSummary, LibraryError> {
        let root = root.as_ref();
        let canonical_root =
            fs::canonicalize(root).map_err(|error| LibraryError::io(root.to_path_buf(), error))?;
        if !canonical_root.is_dir() {
            return Err(LibraryError::invalid_root(canonical_root));
        }

        let mut paths = Vec::new();
        collect_audio_files(&canonical_root, &mut paths)?;
        paths.sort();

        let mut connection = self.connect()?;
        let cached_tracks = load_cached_tracks(&connection, &canonical_root)?;

        let mut discovered = Vec::new();
        let mut skipped = 0;
        for path in paths {
            let file_metadata =
                fs::metadata(&path).map_err(|error| LibraryError::io(path.clone(), error))?;
            let file_size = i64::try_from(file_metadata.len())
                .map_err(|_| LibraryError::FileTooLarge(path.clone()))?;
            let modified_nanos = modified_nanos(&file_metadata);
            if let Some(cached) = cached_tracks.get(&path)
                && cached.file_size == file_size
                && cached.modified_nanos == modified_nanos
            {
                discovered.push(cached.clone());
                continue;
            }
            match probe_metadata(&path) {
                Ok(metadata) => discovered.push(DiscoveredTrack {
                    path,
                    title: metadata.title().to_owned(),
                    artist: metadata.artist().map(str::to_owned),
                    album: metadata.album().map(str::to_owned),
                    duration_seconds: metadata.duration_seconds(),
                    sample_rate: metadata.sample_rate(),
                    channels: metadata.channels(),
                    file_size,
                    modified_nanos,
                }),
                Err(_) => skipped += 1,
            }
        }

        let transaction = connection.transaction()?;
        let root_path = canonical_root.to_string_lossy();
        transaction.execute(
            "INSERT INTO library_roots(path, scan_generation) VALUES (?1, 0) \
             ON CONFLICT(path) DO NOTHING",
            [root_path.as_ref()],
        )?;
        let (root_id, generation): (i64, i64) = transaction.query_row(
            "SELECT id, scan_generation + 1 FROM library_roots WHERE path = ?1",
            [root_path.as_ref()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        transaction.execute(
            "UPDATE library_roots SET scan_generation = ?2 WHERE id = ?1",
            params![root_id, generation],
        )?;

        for track in &discovered {
            transaction.execute(
                "INSERT INTO library_tracks(
                    root_id, path, title, artist, album, duration_seconds, sample_rate, channels,
                    file_size, modified_nanos, seen_generation
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(path) DO UPDATE SET
                    root_id = excluded.root_id,
                    title = excluded.title,
                    artist = excluded.artist,
                    album = excluded.album,
                    duration_seconds = excluded.duration_seconds,
                    sample_rate = excluded.sample_rate,
                    channels = excluded.channels,
                    file_size = excluded.file_size,
                    modified_nanos = excluded.modified_nanos,
                    seen_generation = excluded.seen_generation",
                params![
                    root_id,
                    track.path.to_string_lossy().as_ref(),
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_seconds,
                    i64::from(track.sample_rate),
                    i64::try_from(track.channels).unwrap_or(i64::MAX),
                    track.file_size,
                    track.modified_nanos,
                    generation,
                ],
            )?;
        }
        let removed = transaction.execute(
            "DELETE FROM library_tracks WHERE root_id = ?1 AND seen_generation != ?2",
            params![root_id, generation],
        )?;
        transaction.commit()?;

        Ok(ScanSummary {
            discovered: discovered.len(),
            skipped,
            removed,
        })
    }

    pub(crate) fn search(&self, query: &str) -> Result<Vec<LibraryTrack>, LibraryError> {
        let connection = self.connect()?;
        let pattern = format!("%{}%", query.to_lowercase());
        let mut statement = connection.prepare(
            "SELECT id, path, title, artist, album, duration_seconds, sample_rate, channels
             FROM library_tracks
             WHERE ?1 = '%%'
                OR LOWER(title) LIKE ?1
                OR LOWER(COALESCE(artist, '')) LIKE ?1
                OR LOWER(COALESCE(album, '')) LIKE ?1
             ORDER BY LOWER(COALESCE(artist, '')), LOWER(COALESCE(album, '')), LOWER(title), path",
        )?;
        let rows = statement.query_map([pattern], |row| {
            Ok(LibraryTrack {
                id: LocalTrackId(row.get(0)?),
                path: PathBuf::from(row.get::<_, String>(1)?),
                title: row.get(2)?,
                artist: row.get(3)?,
                album: row.get(4)?,
                duration_seconds: row.get(5)?,
                sample_rate: row.get::<_, u32>(6)?,
                channels: usize::try_from(row.get::<_, u32>(7)?)
                    .expect("u32 channel count fits usize on supported desktop targets"),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn connect(&self) -> Result<Connection, LibraryError> {
        let connection = Connection::open(&self.database_path)?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(connection)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LocalTrackId(i64);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LibraryTrack {
    id: LocalTrackId,
    path: PathBuf,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_seconds: Option<f64>,
    sample_rate: u32,
    channels: usize,
}

impl LibraryTrack {
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

    pub(crate) const fn duration_seconds(&self) -> Option<f64> {
        self.duration_seconds
    }

    pub(crate) const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) const fn channels(&self) -> usize {
        self.channels
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScanSummary {
    discovered: usize,
    skipped: usize,
    removed: usize,
}

impl ScanSummary {
    pub(crate) const fn discovered(self) -> usize {
        self.discovered
    }

    pub(crate) const fn skipped(self) -> usize {
        self.skipped
    }

    pub(crate) const fn removed(self) -> usize {
        self.removed
    }
}

#[derive(Debug)]
pub(crate) enum LibraryError {
    Database(rusqlite::Error),
    DataDirectoryUnavailable,
    FileTooLarge(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidRoot(PathBuf),
    UnsupportedSchemaVersion(i32),
}

impl LibraryError {
    fn io(path: PathBuf, source: std::io::Error) -> Self {
        Self::Io { path, source }
    }

    fn invalid_root(path: PathBuf) -> Self {
        Self::InvalidRoot(path)
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "library database: {error}"),
            Self::DataDirectoryUnavailable => {
                write!(formatter, "macOS application data directory is unavailable")
            }
            Self::FileTooLarge(path) => write!(
                formatter,
                "audio file size cannot be indexed by SQLite: {}",
                path.display()
            ),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::InvalidRoot(path) => {
                write!(
                    formatter,
                    "library root is not a directory: {}",
                    path.display()
                )
            }
            Self::UnsupportedSchemaVersion(version) => write!(
                formatter,
                "library database schema version {version} is newer than supported version {SCHEMA_VERSION}"
            ),
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::DataDirectoryUnavailable => None,
            Self::FileTooLarge(_) => None,
            Self::Io { source, .. } => Some(source),
            Self::InvalidRoot(_) | Self::UnsupportedSchemaVersion(_) => None,
        }
    }
}

impl From<rusqlite::Error> for LibraryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone, Debug)]
struct DiscoveredTrack {
    path: PathBuf,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_seconds: Option<f64>,
    sample_rate: u32,
    channels: usize,
    file_size: i64,
    modified_nanos: i64,
}

fn load_cached_tracks(
    connection: &Connection,
    root: &Path,
) -> Result<HashMap<PathBuf, DiscoveredTrack>, LibraryError> {
    let mut statement = connection.prepare(
        "SELECT tracks.path, tracks.title, tracks.artist, tracks.album, tracks.duration_seconds,
                tracks.sample_rate, tracks.channels, tracks.file_size, tracks.modified_nanos
         FROM library_tracks AS tracks
         INNER JOIN library_roots AS roots ON roots.id = tracks.root_id
         WHERE roots.path = ?1",
    )?;
    let rows = statement.query_map([root.to_string_lossy().as_ref()], |row| {
        let path = PathBuf::from(row.get::<_, String>(0)?);
        Ok((
            path.clone(),
            DiscoveredTrack {
                path,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                duration_seconds: row.get(4)?,
                sample_rate: row.get(5)?,
                channels: usize::try_from(row.get::<_, u32>(6)?)
                    .expect("u32 channel count fits usize on supported desktop targets"),
                file_size: row.get(7)?,
                modified_nanos: row.get(8)?,
            },
        ))
    })?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(Into::into)
}

fn migrate(connection: &Connection) -> Result<(), LibraryError> {
    let version: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 0 {
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE library_roots(
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                scan_generation INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE library_tracks(
                id INTEGER PRIMARY KEY,
                root_id INTEGER NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
                path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                artist TEXT,
                album TEXT,
                duration_seconds REAL,
                sample_rate INTEGER NOT NULL,
                channels INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                modified_nanos INTEGER NOT NULL,
                seen_generation INTEGER NOT NULL
             );
             CREATE INDEX library_tracks_search
                ON library_tracks(title, artist, album, path);
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    } else if version != SCHEMA_VERSION {
        return Err(LibraryError::UnsupportedSchemaVersion(version));
    }
    Ok(())
}

fn collect_audio_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), LibraryError> {
    let entries = fs::read_dir(directory)
        .map_err(|error| LibraryError::io(directory.to_path_buf(), error))?;
    for entry in entries {
        let entry = entry.map_err(|error| LibraryError::io(directory.to_path_buf(), error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| LibraryError::io(path.clone(), error))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_audio_files(&path, files)?;
        } else if file_type.is_file() && is_supported_audio(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| SUPPORTED_EXTENSIONS.contains(&extension.as_str()))
}

fn modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use tempfile::tempdir;

    use super::{LibraryCatalogue, LibraryTrack};

    #[test]
    fn scanned_track_survives_catalogue_reopen() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        let track_path = music.join("Signal Bloom.wav");
        write_stereo_wav(&track_path, 8_000, 8_000);
        let database_path = directory.path().join("library.sqlite3");

        let catalogue = LibraryCatalogue::open(&database_path).expect("catalogue opens");
        let summary = catalogue.scan_root(&music).expect("music root scans");
        assert_eq!(summary.discovered(), 1);
        drop(catalogue);

        let reopened = LibraryCatalogue::open(&database_path).expect("catalogue reopens");
        let tracks = reopened.search("").expect("persisted tracks load");

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title(), "Signal Bloom");
        assert_eq!(
            tracks[0].path(),
            fs::canonicalize(track_path).expect("track fixture path canonicalizes")
        );
    }

    #[test]
    fn scan_is_recursive_skips_corrupt_audio_and_does_not_follow_directory_symlinks() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        let nested = music.join("Ambient");
        fs::create_dir_all(&nested).expect("nested music fixture directory is created");
        write_stereo_wav(&nested.join("Signal Bloom.WAV"), 8_000, 8_000);
        fs::write(music.join("Broken.mp3"), b"not an audio stream")
            .expect("corrupt media fixture is written");
        fs::write(music.join("notes.txt"), b"ignore me").expect("unrelated fixture is written");
        #[cfg(unix)]
        symlink(&nested, music.join("Ambient again"))
            .expect("directory symlink fixture is created");
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");

        let summary = catalogue.scan_root(&music).expect("music root scans");
        let tracks = catalogue.search("").expect("tracks load");

        assert_eq!(summary.discovered(), 1);
        assert_eq!(summary.skipped(), 1);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title(), "Signal Bloom");
    }

    #[test]
    fn successful_rescan_removes_missing_tracks() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        let first = music.join("First Light.wav");
        let second = music.join("Second Sun.wav");
        write_stereo_wav(&first, 8_000, 8_000);
        write_stereo_wav(&second, 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("initial scan succeeds");
        fs::remove_file(&first).expect("first track is removed from the fixture root");

        let summary = catalogue.scan_root(&music).expect("rescan succeeds");
        let tracks = catalogue.search("").expect("tracks load");

        assert_eq!(summary.discovered(), 1);
        assert_eq!(summary.removed(), 1);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title(), "Second Sun");
    }

    #[test]
    fn failed_root_rescan_preserves_last_known_good_catalogue() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Last Known Good.wav"), 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("initial scan succeeds");
        fs::remove_dir_all(&music).expect("fixture root is removed");

        assert!(catalogue.scan_root(&music).is_err());
        let tracks = catalogue.search("").expect("last known-good tracks load");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title(), "Last Known Good");
    }

    #[test]
    fn search_is_case_insensitive_and_deterministic() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("Zeta Signal.wav"), 8_000, 8_000);
        write_stereo_wav(&music.join("alpha signal.wav"), 8_000, 8_000);
        write_stereo_wav(&music.join("Unrelated.wav"), 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music root scans");

        let tracks = catalogue.search("SIGNAL").expect("search succeeds");

        assert_eq!(
            tracks.iter().map(LibraryTrack::title).collect::<Vec<_>>(),
            ["alpha signal", "Zeta Signal"]
        );
    }

    #[test]
    fn unchanged_files_reuse_their_indexed_metadata() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        let track = music.join("Signal Bloom.wav");
        write_stereo_wav(&track, 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("initial scan succeeds");
        catalogue
            .connect()
            .expect("test database connects")
            .execute(
                "UPDATE library_tracks SET title = 'Cached Signal Bloom' WHERE path = ?1",
                [fs::canonicalize(&track)
                    .expect("track canonicalizes")
                    .to_string_lossy()
                    .as_ref()],
            )
            .expect("test marks the indexed metadata");

        catalogue
            .scan_root(&music)
            .expect("unchanged rescan succeeds");
        let tracks = catalogue.search("").expect("tracks load");

        assert_eq!(tracks[0].title(), "Cached Signal Bloom");
    }

    #[test]
    fn changed_files_refresh_their_indexed_metadata() {
        let directory = tempdir().expect("temporary library directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        let track = music.join("Mutable Signal.wav");
        write_stereo_wav(&track, 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("initial scan succeeds");

        write_stereo_wav(&track, 16_000, 16_000);
        catalogue
            .scan_root(&music)
            .expect("changed rescan succeeds");
        let tracks = catalogue.search("").expect("tracks load");

        assert_eq!(tracks[0].sample_rate(), 16_000);
        assert_eq!(tracks[0].duration_seconds(), Some(1.0));
    }

    fn write_stereo_wav(path: &Path, sample_rate: u32, frame_count: u32) {
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

        for frame in 0..frame_count {
            let left = ((frame % 512) as i16).saturating_mul(32);
            let right = -left;
            bytes.extend_from_slice(&left.to_le_bytes());
            bytes.extend_from_slice(&right.to_le_bytes());
        }

        fs::write(path, bytes).expect("WAV fixture is written");
    }
}
