//! A dependency-free temporary directory for filesystem tests.
//!
//! Store and application tests need a real directory on a real filesystem to
//! exercise same-directory staging, atomic rename, and platform path
//! behaviour. The name is derived from the process ID and a per-process
//! counter, so it needs neither a clock nor an OS random source, and it is
//! unique across concurrently running test binaries.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A directory created under the platform temporary directory and removed when
/// the value is dropped.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a new uniquely named temporary directory.
    ///
    /// `label` appears in the directory name to make stray directories
    /// attributable to a test.
    pub fn new(label: &str) -> io::Result<Self> {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "fieldnotes-{label}-{}-{unique}",
            std::process::id()
        ));
        // A pre-existing directory would let one test observe another's files.
        std::fs::create_dir_all(&path)?;
        Ok(TempDir { path })
    }

    /// The directory path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Cleanup is best effort: a failure here must not mask a test result.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::TempDir;

    #[test]
    fn creates_and_removes_a_unique_directory() -> std::io::Result<()> {
        let first = TempDir::new("selftest")?;
        let second = TempDir::new("selftest")?;
        assert!(first.path().is_dir());
        assert_ne!(first.path(), second.path());
        let remembered = first.path().to_path_buf();
        drop(first);
        assert!(!remembered.exists());
        Ok(())
    }
}
