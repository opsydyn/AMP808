//! macOS security-scoped bookmark adapter.

use std::fs;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{
    NSData, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
};

use super::PresetFolderError;

// ADR-0034: Foundation and security-scope ownership stay inside this adapter.
#[derive(Debug)]
pub(crate) struct PresetFolderAccess {
    url: Retained<NSURL>,
    path: PathBuf,
    started: bool,
}

impl PresetFolderAccess {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PresetFolderAccess {
    fn drop(&mut self) {
        if self.started {
            // SAFETY: `started` is set only after the matching start call returned true, and this
            // guard owns exactly that one activation.
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }
}

#[derive(Debug)]
pub(crate) struct PreparedPresetFolder {
    access: PresetFolderAccess,
    bookmark: Vec<u8>,
}

impl PreparedPresetFolder {
    pub(crate) fn access(&self) -> &PresetFolderAccess {
        &self.access
    }

    pub(crate) fn bookmark(&self) -> &[u8] {
        &self.bookmark
    }

    pub(crate) fn into_access(self) -> PresetFolderAccess {
        self.access
    }
}

#[derive(Debug)]
pub(crate) struct RestoredPresetFolder {
    access: PresetFolderAccess,
    refreshed_bookmark: Option<Vec<u8>>,
}

impl RestoredPresetFolder {
    pub(crate) fn access(&self) -> &PresetFolderAccess {
        &self.access
    }

    pub(crate) fn refreshed_bookmark(&self) -> Option<&[u8]> {
        self.refreshed_bookmark.as_deref()
    }

    pub(crate) fn into_access(self) -> PresetFolderAccess {
        self.access
    }
}

pub(crate) fn create_bookmark(path: &Path) -> Result<PreparedPresetFolder, PresetFolderError> {
    let url =
        NSURL::from_directory_path(path).ok_or_else(|| PresetFolderError::InvalidFolderPath {
            path: path.to_path_buf(),
        })?;
    let bookmark = bookmark_bytes(&url)?;
    let access = start_access(url)?;
    Ok(PreparedPresetFolder { access, bookmark })
}

pub(crate) fn restore_bookmark(bookmark: &[u8]) -> Result<RestoredPresetFolder, PresetFolderError> {
    let data = NSData::with_bytes(bookmark);
    let mut stale = Bool::NO;
    // SAFETY: `stale` is a valid writable Objective-C BOOL for the duration of the synchronous
    // Foundation call, and the other borrowed arguments outlive the call.
    let url = unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            &data,
            NSURLBookmarkResolutionOptions::WithSecurityScope
                | NSURLBookmarkResolutionOptions::WithoutUI
                | NSURLBookmarkResolutionOptions::WithoutImplicitStartAccessing,
            None,
            &mut stale,
        )
    }
    .map_err(|error| {
        PresetFolderError::foundation("resolve preset folder bookmark", error.to_string())
    })?;
    let access = start_access(url)?;
    let refreshed_bookmark = if stale.as_bool() {
        Some(bookmark_bytes(&access.url)?)
    } else {
        None
    };
    Ok(RestoredPresetFolder {
        access,
        refreshed_bookmark,
    })
}

fn bookmark_bytes(url: &NSURL) -> Result<Vec<u8>, PresetFolderError> {
    url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
        NSURLBookmarkCreationOptions::WithSecurityScope
            | NSURLBookmarkCreationOptions::SecurityScopeAllowOnlyReadAccess,
        None,
        None,
    )
    .map(|data| data.to_vec())
    .map_err(|error| {
        PresetFolderError::foundation("create preset folder bookmark", error.to_string())
    })
}

fn start_access(url: Retained<NSURL>) -> Result<PresetFolderAccess, PresetFolderError> {
    let path = url.to_file_path().ok_or_else(|| {
        PresetFolderError::foundation("resolve preset folder path", "not a file URL")
    })?;
    // SAFETY: `url` is a live resolved or user-selected file URL. A true result is balanced by
    // `PresetFolderAccess::drop`; a false result is never stopped.
    let started = unsafe { url.startAccessingSecurityScopedResource() };
    if !started {
        fs::read_dir(&path).map_err(|source| PresetFolderError::AccessDenied {
            path: path.clone(),
            source,
        })?;
    }
    Ok(PresetFolderAccess { url, path, started })
}

#[cfg(test)]
mod tests {
    use super::restore_bookmark;

    #[test]
    fn corrupt_bookmark_is_rejected_without_panicking() {
        let error = restore_bookmark(&[0xff, 0x00, 0xfe])
            .expect_err("corrupt Foundation bookmark must be rejected");

        assert!(
            error.to_string().contains("resolve preset folder bookmark"),
            "error identifies the failed operation: {error}"
        );
    }
}
