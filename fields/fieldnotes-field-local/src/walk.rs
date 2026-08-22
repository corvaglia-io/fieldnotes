//! A deterministic, containment-safe walk of the configured root.
//!
//! # Containment
//!
//! The configured root is the only reachable region. Every directory is
//! entered only by recursing from the root through directory entries this
//! Field itself just listed -- never from an externally supplied path -- and
//! every symlink encountered, in any position, is skipped rather than
//! followed. Because the walk never follows a symlink, it is structurally
//! impossible for it to leave the root, regardless of what a symlink inside
//! the root points at. A path is always joined with [`Path::join`] against a
//! name this Field itself read from [`std::fs::read_dir`], never built by
//! string concatenation, so a traversal sequence cannot appear in a path this
//! Field constructs.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// One regular file found inside the configured root.
#[derive(Debug, Clone)]
pub(crate) struct WalkEntry {
    /// The file's path relative to the root, rendered with `/` separators so
    /// it is stable across platforms. Display evidence only, never a path
    /// core would treat as a destination.
    pub(crate) relative_path: String,
    /// The file's absolute path, for reading.
    pub(crate) absolute_path: PathBuf,
    /// The file's last-modified instant, in whole Unix-epoch seconds, from
    /// file metadata alone.
    pub(crate) modified_unix_seconds: i64,
}

/// Something the walk could not collect as a regular file.
#[derive(Debug, Clone)]
pub(crate) enum WalkIssue {
    /// A symlink was encountered and skipped rather than followed, wherever
    /// it appeared: this Field never leaves the configured root.
    SymlinkSkipped {
        /// The symlink's path relative to the root.
        relative_path: String,
    },
    /// A directory or file could not be read.
    Unreadable {
        /// The path relative to the root, when known.
        relative_path: Option<String>,
        /// The operating-system error, in reviewable terms.
        reason: String,
    },
}

/// The result of one walk.
#[derive(Debug, Clone, Default)]
pub(crate) struct WalkOutcome {
    /// Every regular file found, sorted by relative path for determinism.
    pub(crate) entries: Vec<WalkEntry>,
    /// Everything the walk could not collect as a regular file.
    pub(crate) issues: Vec<WalkIssue>,
}

impl WalkOutcome {
    /// Whether the walk enumerated its whole scope without an error. A
    /// skipped symlink does not disqualify completeness: this Field
    /// declares upfront that it never follows one, so skipping it is
    /// expected behavior, not a failure to enumerate.
    #[must_use]
    pub(crate) fn is_complete(&self) -> bool {
        self.issues
            .iter()
            .all(|issue| matches!(issue, WalkIssue::SymlinkSkipped { .. }))
    }
}

fn to_posix(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn non_empty_posix(path: &Path) -> Option<String> {
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(to_posix(path))
    }
}

fn modified_seconds(metadata: &fs::Metadata) -> i64 {
    match metadata.modified().and_then(|modified| {
        modified
            .duration_since(UNIX_EPOCH)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
    }) {
        Ok(since_epoch) => i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX),
        // Either the platform does not report a modification time, or it
        // reported one before the epoch. Either way this is metadata this
        // Field cannot read a meaningful instant from; the oldest
        // representable instant keeps the file eligible for collection
        // rather than silently excluding it.
        Err(_) => 0,
    }
}

/// Walks `root`, never following a symlink and never reading outside it.
#[must_use]
pub(crate) fn walk(root: &Path) -> WalkOutcome {
    let mut entries = Vec::new();
    let mut issues = Vec::new();
    let mut pending: VecDeque<PathBuf> = VecDeque::new();
    pending.push_back(PathBuf::new());

    while let Some(relative_dir) = pending.pop_front() {
        let absolute_dir = if relative_dir.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&relative_dir)
        };
        let listing = match fs::read_dir(&absolute_dir) {
            Ok(listing) => listing,
            Err(error) => {
                issues.push(WalkIssue::Unreadable {
                    relative_path: non_empty_posix(&relative_dir),
                    reason: error.to_string(),
                });
                continue;
            }
        };

        let mut children: Vec<(OsString, fs::FileType)> = Vec::new();
        for item in listing {
            let outcome = item.and_then(|entry| {
                let file_type = entry.file_type()?;
                Ok((entry.file_name(), file_type))
            });
            match outcome {
                Ok(child) => children.push(child),
                Err(error) => issues.push(WalkIssue::Unreadable {
                    relative_path: non_empty_posix(&relative_dir),
                    reason: error.to_string(),
                }),
            }
        }
        children.sort_by(|left, right| left.0.cmp(&right.0));

        for (name, file_type) in children {
            let relative_path = relative_dir.join(&name);
            if file_type.is_symlink() {
                issues.push(WalkIssue::SymlinkSkipped {
                    relative_path: to_posix(&relative_path),
                });
                continue;
            }
            if file_type.is_dir() {
                pending.push_back(relative_path);
                continue;
            }
            if !file_type.is_file() {
                issues.push(WalkIssue::Unreadable {
                    relative_path: Some(to_posix(&relative_path)),
                    reason: "not a regular file, a directory, or a symlink".to_owned(),
                });
                continue;
            }
            let absolute_path = root.join(&relative_path);
            // Re-checked here rather than trusted from the directory
            // listing alone, narrowing the window in which a racing
            // filesystem could swap a symlink in after the listing was
            // read. `crate::record` re-checks again immediately before
            // opening the file, which is where the check-versus-use window
            // actually matters most.
            match fs::symlink_metadata(&absolute_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    issues.push(WalkIssue::SymlinkSkipped {
                        relative_path: to_posix(&relative_path),
                    });
                }
                Ok(metadata) if metadata.file_type().is_file() => {
                    entries.push(WalkEntry {
                        relative_path: to_posix(&relative_path),
                        absolute_path,
                        modified_unix_seconds: modified_seconds(&metadata),
                    });
                }
                Ok(_) => issues.push(WalkIssue::Unreadable {
                    relative_path: Some(to_posix(&relative_path)),
                    reason: "changed to a non-regular entry between listing and read".to_owned(),
                }),
                Err(error) => issues.push(WalkIssue::Unreadable {
                    relative_path: Some(to_posix(&relative_path)),
                    reason: error.to_string(),
                }),
            }
        }
    }

    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    WalkOutcome { entries, issues }
}

#[cfg(test)]
mod tests {
    use super::walk;
    use fieldnotes_test_support::TempDir;
    use std::fs;

    #[test]
    fn walking_an_empty_root_finds_nothing_and_is_complete() -> std::io::Result<()> {
        let temp = TempDir::new("walk-empty")?;
        let outcome = walk(temp.path());
        assert!(outcome.entries.is_empty());
        assert!(outcome.is_complete());
        Ok(())
    }

    #[test]
    fn nested_files_are_found_in_sorted_relative_order() -> std::io::Result<()> {
        let temp = TempDir::new("walk-nested")?;
        fs::create_dir_all(temp.path().join("b"))?;
        fs::write(temp.path().join("b/second.txt"), b"b")?;
        fs::write(temp.path().join("a.txt"), b"a")?;
        let outcome = walk(temp.path());
        let paths: Vec<_> = outcome
            .entries
            .iter()
            .map(|entry| entry.relative_path.clone())
            .collect();
        assert_eq!(paths, vec!["a.txt".to_owned(), "b/second.txt".to_owned()]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_pointing_outside_the_root_is_skipped_not_followed() -> std::io::Result<()> {
        let outside = TempDir::new("walk-outside")?;
        fs::write(outside.path().join("secret.txt"), b"do not collect")?;
        let root = TempDir::new("walk-root")?;
        fs::write(root.path().join("visible.txt"), b"visible")?;
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape"))?;

        let outcome = walk(root.path());
        let paths: Vec<_> = outcome
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect();
        assert_eq!(paths, vec!["visible.txt"]);
        assert!(
            outcome.issues.iter().any(|issue| matches!(
                issue,
                super::WalkIssue::SymlinkSkipped { relative_path } if relative_path == "escape"
            )),
            "the symlink must be reported as skipped: {:?}",
            outcome.issues
        );
        assert!(
            outcome.is_complete(),
            "skipping a symlink is expected behavior, not an enumeration failure"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_inside_the_root_is_also_skipped() -> std::io::Result<()> {
        let root = TempDir::new("walk-inner-symlink")?;
        fs::write(root.path().join("real.txt"), b"real")?;
        std::os::unix::fs::symlink(root.path().join("real.txt"), root.path().join("alias.txt"))?;

        let outcome = walk(root.path());
        let paths: Vec<_> = outcome
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect();
        assert_eq!(paths, vec!["real.txt"]);
        Ok(())
    }

    #[test]
    fn an_unreadable_directory_is_reported_and_does_not_abort_the_rest_of_the_walk()
    -> std::io::Result<()> {
        let root = TempDir::new("walk-unreadable")?;
        fs::write(root.path().join("visible.txt"), b"visible")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = root.path().join("locked");
            fs::create_dir(&locked)?;
            fs::write(locked.join("hidden.txt"), b"hidden")?;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
            let outcome = walk(root.path());
            // Restore permissions so the temp directory can be removed.
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))?;
            assert!(!outcome.is_complete());
            assert!(
                outcome
                    .entries
                    .iter()
                    .any(|entry| entry.relative_path == "visible.txt")
            );
        }
        Ok(())
    }
}
