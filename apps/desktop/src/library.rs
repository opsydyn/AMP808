use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, params};

use crate::media::probe_metadata;

const SCHEMA_VERSION: i32 = 2;
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
            "SELECT id, path, title, artist, album, duration_seconds, sample_rate, channels,
                    file_size, modified_nanos
             FROM library_tracks
             WHERE ?1 = '%%'
                OR LOWER(title) LIKE ?1
                OR LOWER(COALESCE(artist, '')) LIKE ?1
                OR LOWER(COALESCE(album, '')) LIKE ?1
             ORDER BY LOWER(COALESCE(artist, '')), LOWER(COALESCE(album, '')), LOWER(title), path",
        )?;
        let rows = statement.query_map([pattern], read_library_track)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(crate) fn create_playlist(&self, name: &str) -> Result<LocalPlaylist, LibraryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::InvalidPlaylistName);
        }
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_playlists WHERE name = ?1)",
            [name],
            |row| row.get(0),
        )?;
        if exists {
            return Err(LibraryError::PlaylistNameTaken(name.to_owned()));
        }
        transaction.execute("INSERT INTO local_playlists(name) VALUES (?1)", [name])?;
        let id = LocalPlaylistId(transaction.last_insert_rowid());
        transaction.commit()?;
        Ok(LocalPlaylist {
            id,
            name: name.to_owned(),
            track_count: 0,
        })
    }

    pub(crate) fn playlists(&self) -> Result<Vec<LocalPlaylist>, LibraryError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT playlists.id, playlists.name, COUNT(entries.track_id)
             FROM local_playlists AS playlists
             LEFT JOIN local_playlist_entries AS entries
                ON entries.playlist_id = playlists.id
             GROUP BY playlists.id, playlists.name
             ORDER BY LOWER(playlists.name), playlists.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(LocalPlaylist {
                id: LocalPlaylistId(row.get(0)?),
                name: row.get(1)?,
                track_count: usize::try_from(row.get::<_, i64>(2)?)
                    .expect("SQLite playlist count fits usize on supported desktop targets"),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(crate) fn rename_playlist(
        &self,
        playlist_id: LocalPlaylistId,
        name: &str,
    ) -> Result<(), LibraryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::InvalidPlaylistName);
        }
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM local_playlists WHERE name = ?1 AND id != ?2
             )",
            params![name, playlist_id.0],
            |row| row.get(0),
        )?;
        if exists {
            return Err(LibraryError::PlaylistNameTaken(name.to_owned()));
        }
        let changed = transaction.execute(
            "UPDATE local_playlists SET name = ?2 WHERE id = ?1",
            params![playlist_id.0, name],
        )?;
        if changed == 0 {
            return Err(LibraryError::PlaylistNotFound(playlist_id));
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn delete_playlist(&self, playlist_id: LocalPlaylistId) -> Result<(), LibraryError> {
        let connection = self.connect()?;
        let changed =
            connection.execute("DELETE FROM local_playlists WHERE id = ?1", [playlist_id.0])?;
        if changed == 0 {
            return Err(LibraryError::PlaylistNotFound(playlist_id));
        }
        Ok(())
    }

    pub(crate) fn append_to_playlist(
        &self,
        playlist_id: LocalPlaylistId,
        track_id: LocalTrackId,
    ) -> Result<(), LibraryError> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0)
             FROM local_playlist_entries
             WHERE playlist_id = ?1",
            [playlist_id.0],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO local_playlist_entries(playlist_id, position, track_id)
             VALUES (?1, ?2, ?3)",
            params![playlist_id.0, position, track_id.0],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn remove_playlist_position(
        &self,
        playlist_id: LocalPlaylistId,
        position: usize,
    ) -> Result<bool, LibraryError> {
        let position = i64::try_from(position)
            .expect("playlist position fits SQLite on supported desktop targets");
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let removed = transaction.execute(
            "DELETE FROM local_playlist_entries
             WHERE playlist_id = ?1 AND position = ?2",
            params![playlist_id.0, position],
        )?;
        if removed == 0 {
            return Ok(false);
        }
        let maximum_position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), -1)
             FROM local_playlist_entries
             WHERE playlist_id = ?1",
            [playlist_id.0],
            |row| row.get(0),
        )?;
        let offset = maximum_position + 1;
        transaction.execute(
            "UPDATE local_playlist_entries
             SET position = position + ?3
             WHERE playlist_id = ?1 AND position > ?2",
            params![playlist_id.0, position, offset],
        )?;
        transaction.execute(
            "UPDATE local_playlist_entries
             SET position = position - ?3 - 1
             WHERE playlist_id = ?1 AND position > ?2",
            params![playlist_id.0, maximum_position, offset],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub(crate) fn replace_playlist_tracks(
        &self,
        playlist_id: LocalPlaylistId,
        track_ids: &[LocalTrackId],
    ) -> Result<(), LibraryError> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_playlists WHERE id = ?1)",
            [playlist_id.0],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(LibraryError::PlaylistNotFound(playlist_id));
        }
        transaction.execute(
            "DELETE FROM local_playlist_entries WHERE playlist_id = ?1",
            [playlist_id.0],
        )?;
        for (position, track_id) in track_ids.iter().enumerate() {
            let position = i64::try_from(position)
                .expect("playlist position fits SQLite on supported desktop targets");
            transaction.execute(
                "INSERT INTO local_playlist_entries(playlist_id, position, track_id)
                 VALUES (?1, ?2, ?3)",
                params![playlist_id.0, position, track_id.0],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn playlist_tracks(
        &self,
        playlist_id: LocalPlaylistId,
    ) -> Result<Vec<LibraryTrack>, LibraryError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT tracks.id, tracks.path, tracks.title, tracks.artist, tracks.album,
                    tracks.duration_seconds, tracks.sample_rate, tracks.channels,
                    tracks.file_size, tracks.modified_nanos
             FROM local_playlist_entries AS entries
             INNER JOIN library_tracks AS tracks ON tracks.id = entries.track_id
             WHERE entries.playlist_id = ?1
             ORDER BY entries.position",
        )?;
        let rows = statement.query_map([playlist_id.0], read_library_track)?;
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
    file_size: i64,
    modified_nanos: i64,
}

impl LibraryTrack {
    pub(crate) const fn id(&self) -> LocalTrackId {
        self.id
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

    pub(crate) const fn duration_seconds(&self) -> Option<f64> {
        self.duration_seconds
    }

    pub(crate) const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) const fn channels(&self) -> usize {
        self.channels
    }

    pub(crate) const fn file_fingerprint(&self) -> (i64, i64) {
        (self.file_size, self.modified_nanos)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LocalPlaylistId(i64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalPlaylist {
    id: LocalPlaylistId,
    name: String,
    track_count: usize,
}

impl LocalPlaylist {
    pub(crate) const fn id(&self) -> LocalPlaylistId {
        self.id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn track_count(&self) -> usize {
        self.track_count
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

fn read_library_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryTrack> {
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
        file_size: row.get(8)?,
        modified_nanos: row.get(9)?,
    })
}

#[derive(Debug)]
pub(crate) enum LibraryError {
    Database(rusqlite::Error),
    DataDirectoryUnavailable,
    FileTooLarge(PathBuf),
    InvalidPlaylistName,
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidRoot(PathBuf),
    PlaylistNameTaken(String),
    PlaylistNotFound(LocalPlaylistId),
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
            Self::InvalidPlaylistName => write!(formatter, "playlist name cannot be empty"),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::InvalidRoot(path) => {
                write!(
                    formatter,
                    "library root is not a directory: {}",
                    path.display()
                )
            }
            Self::PlaylistNameTaken(name) => {
                write!(formatter, "playlist name is already in use: {name}")
            }
            Self::PlaylistNotFound(id) => {
                write!(formatter, "playlist does not exist: {}", id.0)
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
            Self::InvalidPlaylistName => None,
            Self::Io { source, .. } => Some(source),
            Self::InvalidRoot(_)
            | Self::PlaylistNameTaken(_)
            | Self::PlaylistNotFound(_)
            | Self::UnsupportedSchemaVersion(_) => None,
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
             CREATE TABLE local_playlists(
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL COLLATE NOCASE UNIQUE
                    CHECK(length(trim(name)) > 0)
             );
             CREATE TABLE local_playlist_entries(
                playlist_id INTEGER NOT NULL
                    REFERENCES local_playlists(id) ON DELETE CASCADE,
                position INTEGER NOT NULL CHECK(position >= 0),
                track_id INTEGER NOT NULL
                    REFERENCES library_tracks(id) ON DELETE CASCADE,
                PRIMARY KEY(playlist_id, position)
             );
             CREATE INDEX local_playlist_entries_track
                ON local_playlist_entries(track_id);
             PRAGMA user_version = 2;
             COMMIT;",
        )?;
    } else if version == 1 {
        // ADR-0028: named playlists extend the local catalogue; the session queue stays in memory.
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE local_playlists(
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL COLLATE NOCASE UNIQUE
                    CHECK(length(trim(name)) > 0)
             );
             CREATE TABLE local_playlist_entries(
                playlist_id INTEGER NOT NULL
                    REFERENCES local_playlists(id) ON DELETE CASCADE,
                position INTEGER NOT NULL CHECK(position >= 0),
                track_id INTEGER NOT NULL
                    REFERENCES library_tracks(id) ON DELETE CASCADE,
                PRIMARY KEY(playlist_id, position)
             );
             CREATE INDEX local_playlist_entries_track
                ON local_playlist_entries(track_id);
             PRAGMA user_version = 2;
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

    use super::{LibraryCatalogue, LibraryError, LibraryTrack};

    #[test]
    fn schema_one_catalogue_migrates_without_losing_tracks() {
        let directory = tempdir().expect("temporary catalogue directory is created");
        let database_path = directory.path().join("library.sqlite3");
        let connection =
            rusqlite::Connection::open(&database_path).expect("schema-one fixture database opens");
        connection
            .execute_batch(
                "CREATE TABLE library_roots(
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
                 INSERT INTO library_roots(id, path, scan_generation)
                    VALUES (1, '/Music', 1);
                 INSERT INTO library_tracks(
                    id, root_id, path, title, artist, album, duration_seconds, sample_rate,
                    channels, file_size, modified_nanos, seen_generation
                 ) VALUES (
                    7, 1, '/Music/Signal Bloom.wav', 'Signal Bloom', 'Aurora Unit',
                    'Night Transit', 60.0, 44100, 2, 1024, 99, 1
                 );
                 PRAGMA user_version = 1;",
            )
            .expect("literal schema-one fixture is created");
        drop(connection);

        let catalogue = LibraryCatalogue::open(&database_path).expect("catalogue migrates");
        let tracks = catalogue.search("").expect("migrated tracks load");
        let version: i32 = catalogue
            .connect()
            .expect("migrated database connects")
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("schema version loads");

        assert_eq!(version, 2);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title(), "Signal Bloom");
        assert_eq!(tracks[0].file_fingerprint(), (1024, 99));
    }

    #[test]
    fn named_playlist_preserves_order_and_duplicates_across_reopen() {
        let directory = tempdir().expect("temporary catalogue directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        write_stereo_wav(&music.join("First Light.wav"), 8_000, 8_000);
        write_stereo_wav(&music.join("Second Sun.wav"), 8_000, 8_000);
        let database_path = directory.path().join("library.sqlite3");
        let catalogue = LibraryCatalogue::open(&database_path).expect("catalogue opens");
        catalogue.scan_root(&music).expect("music root scans");
        let tracks = catalogue.search("").expect("tracks load");
        let playlist = catalogue
            .create_playlist("Night Run")
            .expect("playlist is created");
        catalogue
            .append_to_playlist(playlist.id(), tracks[0].id())
            .expect("first track is appended");
        catalogue
            .append_to_playlist(playlist.id(), tracks[1].id())
            .expect("second track is appended");
        catalogue
            .append_to_playlist(playlist.id(), tracks[0].id())
            .expect("duplicate track is appended");
        drop(catalogue);

        let reopened = LibraryCatalogue::open(&database_path).expect("catalogue reopens");
        let playlists = reopened.playlists().expect("playlists load");
        let entries = reopened
            .playlist_tracks(playlists[0].id())
            .expect("playlist entries load");

        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name(), "Night Run");
        assert_eq!(
            entries.iter().map(LibraryTrack::title).collect::<Vec<_>>(),
            ["First Light", "Second Sun", "First Light"]
        );
    }

    #[test]
    fn playlist_names_are_trimmed_nonempty_and_case_insensitively_unique() {
        let directory = tempdir().expect("temporary catalogue directory is created");
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");

        let playlist = catalogue
            .create_playlist("  Night Run  ")
            .expect("trimmed playlist is created");
        let empty_error = catalogue
            .create_playlist("   ")
            .expect_err("empty playlist name is rejected");
        let duplicate_error = catalogue
            .create_playlist("night run")
            .expect_err("case-insensitive duplicate name is rejected");

        assert_eq!(playlist.name(), "Night Run");
        assert!(matches!(empty_error, LibraryError::InvalidPlaylistName));
        assert!(matches!(
            duplicate_error,
            LibraryError::PlaylistNameTaken(name) if name == "night run"
        ));
    }

    #[test]
    fn playlist_can_be_renamed_and_deleted() {
        let directory = tempdir().expect("temporary catalogue directory is created");
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        let playlist = catalogue
            .create_playlist("Night Run")
            .expect("playlist is created");

        catalogue
            .rename_playlist(playlist.id(), "Morning Signal")
            .expect("playlist is renamed");
        let renamed = catalogue.playlists().expect("renamed playlist loads");
        catalogue
            .delete_playlist(playlist.id())
            .expect("playlist is deleted");

        assert_eq!(renamed[0].name(), "Morning Signal");
        assert!(catalogue.playlists().expect("playlists load").is_empty());
    }

    #[test]
    fn playlist_entries_can_be_removed_replaced_and_cascade_with_library_tracks() {
        let directory = tempdir().expect("temporary catalogue directory is created");
        let music = directory.path().join("Music");
        fs::create_dir(&music).expect("music fixture directory is created");
        let first_path = music.join("First Light.wav");
        let second_path = music.join("Second Sun.wav");
        let third_path = music.join("Third Orbit.wav");
        write_stereo_wav(&first_path, 8_000, 8_000);
        write_stereo_wav(&second_path, 8_000, 8_000);
        write_stereo_wav(&third_path, 8_000, 8_000);
        let catalogue = LibraryCatalogue::open(directory.path().join("library.sqlite3"))
            .expect("catalogue opens");
        catalogue.scan_root(&music).expect("music root scans");
        let tracks = catalogue.search("").expect("tracks load");
        let playlist = catalogue
            .create_playlist("Signal Path")
            .expect("playlist is created");
        for track in &tracks {
            catalogue
                .append_to_playlist(playlist.id(), track.id())
                .expect("track is appended");
        }

        assert!(
            catalogue
                .remove_playlist_position(playlist.id(), 1)
                .expect("middle entry is removed")
        );
        let after_remove = catalogue
            .playlist_tracks(playlist.id())
            .expect("playlist entries load");
        assert_eq!(
            after_remove
                .iter()
                .map(LibraryTrack::title)
                .collect::<Vec<_>>(),
            ["First Light", "Third Orbit"]
        );

        catalogue
            .replace_playlist_tracks(
                playlist.id(),
                &[tracks[2].id(), tracks[0].id(), tracks[2].id()],
            )
            .expect("playlist order is replaced");
        fs::remove_file(&third_path).expect("third track fixture is removed");
        catalogue.scan_root(&music).expect("music root rescans");
        let after_cascade = catalogue
            .playlist_tracks(playlist.id())
            .expect("playlist entries load after cascade");

        assert_eq!(
            after_cascade
                .iter()
                .map(LibraryTrack::title)
                .collect::<Vec<_>>(),
            ["First Light"]
        );
    }

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
