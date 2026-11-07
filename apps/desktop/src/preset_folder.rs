//! Persistent access to the user-selected MilkDrop preset folder.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::{PresetFolderAccess, create_bookmark, restore_bookmark};

pub(crate) const MAX_BOOKMARK_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct PresetFolderBookmarkStore {
    path: PathBuf,
}

impl PresetFolderBookmarkStore {
    #[cfg_attr(
        test,
        expect(
            dead_code,
            reason = "production startup owns the default bookmark location"
        )
    )]
    pub(crate) fn open_default() -> Result<Self, PresetFolderError> {
        let data_directory = dirs::data_dir().ok_or(PresetFolderError::DataDirectoryUnavailable)?;
        Ok(Self::new(
            data_directory
                .join("AMP808-R3TR0")
                .join("preset-folder.bookmark"),
        ))
    }

    pub(crate) fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub(crate) fn load(&self) -> Result<Option<Vec<u8>>, PresetFolderError> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PresetFolderError::io(&self.path, "open", error)),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BOOKMARK_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| PresetFolderError::io(&self.path, "read", error))?;
        validate_bookmark_size(bytes.len())?;
        Ok(Some(bytes))
    }

    pub(crate) fn save(&self, bytes: &[u8]) -> Result<(), PresetFolderError> {
        validate_bookmark_size(bytes.len())?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| PresetFolderError::InvalidStorePath {
                path: self.path.clone(),
            })?;
        fs::create_dir_all(parent)
            .map_err(|error| PresetFolderError::io(parent, "create directory", error))?;
        let temporary_path = temporary_path(parent, &self.path);
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary_path).map_err(|error| {
                PresetFolderError::io(&temporary_path, "create temporary bookmark", error)
            })?;
            file.write_all(bytes).map_err(|error| {
                PresetFolderError::io(&temporary_path, "write temporary bookmark", error)
            })?;
            file.sync_all().map_err(|error| {
                PresetFolderError::io(&temporary_path, "sync temporary bookmark", error)
            })?;
            fs::rename(&temporary_path, &self.path)
                .map_err(|error| PresetFolderError::io(&self.path, "replace bookmark", error))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }
}

fn validate_bookmark_size(actual: usize) -> Result<(), PresetFolderError> {
    if actual > MAX_BOOKMARK_BYTES {
        return Err(PresetFolderError::BookmarkTooLarge {
            actual,
            maximum: MAX_BOOKMARK_BYTES,
        });
    }
    Ok(())
}

fn temporary_path(parent: &Path, destination: &Path) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("preset-folder.bookmark");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce))
}

#[derive(Debug)]
pub(crate) enum PresetFolderError {
    #[cfg_attr(
        test,
        expect(
            dead_code,
            reason = "only production startup resolves the data directory"
        )
    )]
    DataDirectoryUnavailable,
    InvalidStorePath {
        path: PathBuf,
    },
    BookmarkTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidFolderPath {
        path: PathBuf,
    },
    Foundation {
        operation: &'static str,
        message: String,
    },
    AccessDenied {
        path: PathBuf,
        source: io::Error,
    },
    Io {
        path: PathBuf,
        operation: &'static str,
        source: io::Error,
    },
}

impl PresetFolderError {
    fn io(path: impl AsRef<Path>, operation: &'static str, source: io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            operation,
            source,
        }
    }

    #[cfg(target_os = "macos")]
    fn foundation(operation: &'static str, message: impl Into<String>) -> Self {
        Self::Foundation {
            operation,
            message: message.into(),
        }
    }
}

impl fmt::Display for PresetFolderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirectoryUnavailable => {
                formatter.write_str("application data directory is unavailable")
            }
            Self::InvalidStorePath { path } => {
                write!(formatter, "bookmark path has no parent: {}", path.display())
            }
            Self::BookmarkTooLarge { actual, maximum } => write!(
                formatter,
                "bookmark is {actual} bytes, exceeding the {maximum} byte limit"
            ),
            Self::InvalidFolderPath { path } => write!(
                formatter,
                "preset folder cannot be represented as a macOS file URL: {}",
                path.display()
            ),
            Self::Foundation { operation, message } => {
                write!(formatter, "could not {operation}: {message}")
            }
            Self::AccessDenied { path, source } => write!(
                formatter,
                "macOS denied access to preset folder {}: {source}",
                path.display()
            ),
            Self::Io {
                path,
                operation,
                source,
            } => write!(
                formatter,
                "could not {operation} {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PresetFolderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::AccessDenied { source, .. } => Some(source),
            Self::DataDirectoryUnavailable
            | Self::InvalidStorePath { .. }
            | Self::BookmarkTooLarge { .. }
            | Self::InvalidFolderPath { .. }
            | Self::Foundation { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{MAX_BOOKMARK_BYTES, PresetFolderBookmarkStore, PresetFolderError};

    #[test]
    fn bookmark_bytes_survive_reopen_and_oversize_does_not_replace_them() {
        let directory = tempdir().expect("temporary bookmark directory is created");
        let path = directory.path().join("preset-folder.bookmark");
        let store = PresetFolderBookmarkStore::new(&path);
        let expected = b"opaque-foundation-bookmark";

        assert_eq!(store.load().expect("missing bookmark is allowed"), None);
        store.save(expected).expect("bookmark is persisted");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mode = std::fs::metadata(&path)
                .expect("bookmark metadata is readable")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "bookmark data is user-only");
        }
        assert_eq!(
            PresetFolderBookmarkStore::new(&path)
                .load()
                .expect("bookmark reopens"),
            Some(expected.to_vec())
        );

        let oversized = vec![0_u8; MAX_BOOKMARK_BYTES + 1];
        assert!(matches!(
            store.save(&oversized),
            Err(PresetFolderError::BookmarkTooLarge { .. })
        ));
        assert_eq!(
            store.load().expect("last working bookmark remains"),
            Some(expected.to_vec())
        );
    }
}
