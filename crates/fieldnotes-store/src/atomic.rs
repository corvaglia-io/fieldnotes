//! Crash-safe file installation: stage in the destination directory, make the
//! bytes durable, then rename into place.
//!
//! The sequence is the same on every supported platform:
//!
//! 1. create a uniquely named temporary file **in the destination directory**,
//!    so the rename is always within one directory and one filesystem;
//! 2. write the complete bytes and `fsync` the file;
//! 3. rename the temporary file onto the final name, which is atomic on POSIX
//!    and on Windows (`MoveFileEx` with replace semantics);
//! 4. do the strongest available durability step for the directory entry.
//!
//! Step 4 is where platforms differ. On Unix the containing directory is
//! opened and `fsync`ed, which is what makes the new name survive a crash. On
//! Windows a directory handle cannot be opened for synchronization through the
//! standard library, and the rename is already ordered against the file data,
//! so the step is a documented no-op rather than a compile error or a silent
//! pretence of a guarantee.
//!
//! A partially written temporary file is never a Note: its name carries the
//! reserved [`TEMP_PREFIX`] and a `.part` suffix instead of the canonical
//! `.md` filename, so nothing scanning a notebook can mistake it for a record,
//! and the destination name does not exist until the rename succeeds.

// `File` is only needed by the Unix directory-sync path below; importing it
// unconditionally is an unused import on Windows, where CI denies warnings.
#[cfg(unix)]
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::StoreError;

/// The reserved filename prefix of in-progress staged files.
pub const TEMP_PREFIX: &str = ".fieldnotes-staged-";

/// The reserved filename suffix of in-progress staged files.
pub const TEMP_SUFFIX: &str = ".part";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique staging filename. The process ID separates concurrent processes and
/// the counter separates writes inside one process, so no clock or random
/// source is required.
fn staged_name() -> String {
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{TEMP_PREFIX}{}-{unique}{TEMP_SUFFIX}", std::process::id())
}

/// Whether a filename is a staging file rather than notebook content.
#[must_use]
pub fn is_staged_name(name: &str) -> bool {
    name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX)
}

/// `fsync`s a directory where the platform supports it.
///
/// On Unix this is what makes a rename durable. On other platforms it is a
/// no-op, which is the strongest thing those platforms offer through the
/// standard library.
pub fn sync_directory(dir: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        let handle =
            File::open(dir).map_err(|error| StoreError::io("open directory", dir, error))?;
        handle
            .sync_all()
            .map_err(|error| StoreError::io("synchronize directory", dir, error))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Durable bytes staged in their destination directory, waiting to be renamed
/// into place.
///
/// Dropping a staged file without installing it removes the temporary file, so
/// an interrupted operation leaves neither a partial record nor litter.
#[derive(Debug)]
pub struct StagedFile {
    directory: PathBuf,
    temp_path: PathBuf,
    installed: bool,
}

impl StagedFile {
    /// Writes `bytes` to a uniquely named temporary file in `directory` and
    /// makes them durable.
    ///
    /// The destination name is not touched, so a crash at any point before
    /// [`StagedFile::install`] leaves the destination absent.
    pub fn create(directory: &Path, bytes: &[u8]) -> Result<Self, StoreError> {
        let temp_path = directory.join(staged_name());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|error| StoreError::io("create staging file", &temp_path, error))?;
        let staged = StagedFile {
            directory: directory.to_path_buf(),
            temp_path,
            installed: false,
        };
        file.write_all(bytes)
            .map_err(|error| StoreError::io("write staging file", &staged.temp_path, error))?;
        file.sync_all().map_err(|error| {
            StoreError::io("synchronize staging file", &staged.temp_path, error)
        })?;
        Ok(staged)
    }

    /// The staging path, for recovery and crash-safety tests.
    #[must_use]
    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }

    /// Renames the staged file onto `filename` in the same directory and makes
    /// the directory entry durable.
    ///
    /// The rename replaces an existing file with the same name, which is how
    /// current-state replacement stays atomic: readers see either the previous
    /// complete record or the new complete record.
    pub fn install(mut self, filename: &str) -> Result<PathBuf, StoreError> {
        let destination = self.directory.join(filename);
        std::fs::rename(&self.temp_path, &destination)
            .map_err(|error| StoreError::io("install file", &destination, error))?;
        self.installed = true;
        sync_directory(&self.directory)?;
        Ok(destination)
    }

    /// Removes the staged file without installing it.
    pub fn discard(mut self) -> Result<(), StoreError> {
        self.installed = true;
        std::fs::remove_file(&self.temp_path)
            .map_err(|error| StoreError::io("remove staging file", &self.temp_path, error))
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.installed {
            // Best effort: a failed cleanup must not mask the original error.
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

/// Stages and installs `bytes` as `filename` inside `directory`.
pub fn write_atomic(directory: &Path, filename: &str, bytes: &[u8]) -> Result<PathBuf, StoreError> {
    StagedFile::create(directory, bytes)?.install(filename)
}

/// Lists leftover staging files in a directory.
///
/// Startup recovery surfaces these rather than silently deleting them: they are
/// evidence of an interrupted write, and they are never valid records.
pub fn staged_files(directory: &Path) -> Result<Vec<PathBuf>, StoreError> {
    let mut found = Vec::new();
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(StoreError::io("read directory", directory, error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io("read directory", directory, error))?;
        let name = entry.file_name();
        if let Some(name) = name.to_str()
            && is_staged_name(name)
        {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_test_support::TempDir;

    #[test]
    fn install_replaces_atomically_and_staging_leaves_no_destination() -> Result<(), StoreError> {
        let temp = TempDir::new("atomic").map_err(|error| StoreError::io("create", ".", error))?;
        let dir = temp.path();
        write_atomic(dir, "target.md", b"first\n")?;
        assert_eq!(
            std::fs::read(dir.join("target.md"))
                .map_err(|error| StoreError::io("read", dir, error))?,
            b"first\n"
        );

        // A staged write that is abandoned before install leaves the previous
        // content in place and removes its own temporary file.
        let staged = StagedFile::create(dir, b"second\n")?;
        let temp_path = staged.temp_path().to_path_buf();
        assert!(temp_path.exists());
        assert!(!temp_path.ends_with("target.md"));
        drop(staged);
        assert!(!temp_path.exists());
        assert_eq!(
            std::fs::read(dir.join("target.md"))
                .map_err(|error| StoreError::io("read", dir, error))?,
            b"first\n"
        );

        // Installing over an existing name replaces it.
        write_atomic(dir, "target.md", b"third\n")?;
        assert_eq!(
            std::fs::read(dir.join("target.md"))
                .map_err(|error| StoreError::io("read", dir, error))?,
            b"third\n"
        );
        Ok(())
    }

    #[test]
    fn staged_files_are_recognizable_and_listable() -> Result<(), StoreError> {
        let temp = TempDir::new("staged").map_err(|error| StoreError::io("create", ".", error))?;
        let dir = temp.path();
        assert!(staged_files(dir)?.is_empty());
        let staged = StagedFile::create(dir, b"partial")?;
        let listed = staged_files(dir)?;
        assert_eq!(listed, vec![staged.temp_path().to_path_buf()]);
        assert!(is_staged_name(
            staged
                .temp_path()
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
        ));
        staged.discard()?;
        assert!(staged_files(dir)?.is_empty());
        Ok(())
    }
}
