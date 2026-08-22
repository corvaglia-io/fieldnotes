//! Notebook layout: creation, discovery, and the reserved directory set.
//!
//! Every path is derived from the notebook root with [`Path::join`], never by
//! string concatenation, so separators and drive-relative paths behave the same
//! on Windows, macOS, and Linux.

use std::path::{Component, Path, PathBuf};

use crate::atomic;
use crate::error::StoreError;

/// Resolves a caller-supplied path to an absolute, lexically normalized path.
///
/// Notebook roots are reported back to users and used to derive every other
/// path, so `.` and `..` are resolved here rather than leaking into output or
/// into a parent-directory lookup. Normalization is lexical on purpose: it does
/// not follow symlinks, so it behaves identically on Windows, macOS, and Linux
/// and cannot fail for a path that does not exist yet.
fn absolute_path(path: &Path) -> Result<PathBuf, StoreError> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| StoreError::io("read the working directory", path, error))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

/// The private operational directory inside a notebook.
pub const PRIVATE_DIR: &str = ".fieldnotes";

/// The instance metadata filename inside [`PRIVATE_DIR`].
pub const INSTANCE_FILE: &str = "instance.yaml";

/// The public record directories reserved by the notebook contract, in
/// ascending order.
///
/// `0.1.0` only writes `notes/` and `artifacts/`; the rest are created so that
/// the tree a later release fills in is already the approved shape and so
/// discovery never has to guess.
pub const RESERVED_DIRECTORIES: [&str; 9] = [
    "artifacts",
    "conflicts",
    "entities",
    "extractions",
    "notes",
    "observations",
    "packages",
    "proposals",
    "relationships",
];

/// The private subdirectories: Field configuration, operational sync state,
/// and the freely deletable cache.
const PRIVATE_SUBDIRECTORIES: [&[&str]; 3] = [&["fields"], &["state", "sync"], &["cache"]];

/// Whether `create` initialized a new notebook or adopted an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitState {
    /// The notebook layout and instance metadata were created.
    Created,
    /// A valid notebook already existed, so creation was a no-op.
    AlreadyInitialized,
}

/// A located notebook root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notebook {
    root: PathBuf,
}

impl Notebook {
    /// Creates the notebook directory layout under `root`.
    ///
    /// Creation is idempotent for an already-initialized notebook and refuses
    /// to adopt a directory that holds anything else, so a mistyped path never
    /// scatters notebook directories through an unrelated tree.
    pub fn create(root: &Path) -> Result<(Notebook, InitState), StoreError> {
        let root = absolute_path(root)?;
        if root.exists() && !root.is_dir() {
            return Err(StoreError::NotADirectory { path: root });
        }
        let notebook = Notebook { root };
        let state = if notebook.instance_path().is_file() {
            InitState::AlreadyInitialized
        } else {
            notebook.refuse_unexpected_tree()?;
            InitState::Created
        };
        notebook.create_directories()?;
        Ok((notebook, state))
    }

    /// Opens an existing notebook root.
    pub fn open(root: &Path) -> Result<Notebook, StoreError> {
        let notebook = Notebook {
            root: absolute_path(root)?,
        };
        if notebook.instance_path().is_file() {
            Ok(notebook)
        } else {
            let start = notebook.root;
            Err(StoreError::NotANotebook { start })
        }
    }

    /// Finds the notebook containing `start` by walking up the directory chain.
    ///
    /// A notebook is identified by `.fieldnotes/instance.yaml`, which is the
    /// only file the contract guarantees at the root.
    pub fn discover(start: &Path) -> Result<Notebook, StoreError> {
        let absolute = absolute_path(start)?;
        for ancestor in absolute.ancestors() {
            let candidate = Notebook {
                root: ancestor.to_path_buf(),
            };
            if candidate.instance_path().is_file() {
                return Ok(candidate);
            }
        }
        Err(StoreError::NotANotebook { start: absolute })
    }

    /// The notebook root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The `notes/` directory.
    #[must_use]
    pub fn notes_dir(&self) -> PathBuf {
        self.root.join("notes")
    }

    /// The `artifacts/` directory.
    #[must_use]
    pub fn artifacts_dir(&self) -> PathBuf {
        self.root.join("artifacts")
    }

    /// The private `.fieldnotes/` directory.
    #[must_use]
    pub fn private_dir(&self) -> PathBuf {
        self.root.join(PRIVATE_DIR)
    }

    /// The `.fieldnotes/instance.yaml` path.
    #[must_use]
    pub fn instance_path(&self) -> PathBuf {
        self.private_dir().join(INSTANCE_FILE)
    }

    /// A notebook-relative display path for a file inside the notebook.
    ///
    /// Falls back to the full path when the file is outside the notebook.
    #[must_use]
    pub fn relative_display(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        // Notebook-relative paths are reported with forward slashes so output
        // is comparable across platforms.
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join("/")
    }

    /// Creates every reserved directory, ignoring ones that already exist.
    fn create_directories(&self) -> Result<(), StoreError> {
        let mut directories = vec![self.root.clone(), self.private_dir()];
        for name in RESERVED_DIRECTORIES {
            directories.push(self.root.join(name));
        }
        for segments in PRIVATE_SUBDIRECTORIES {
            let mut path = self.private_dir();
            for segment in segments {
                path = path.join(segment);
            }
            directories.push(path);
        }
        for directory in &directories {
            std::fs::create_dir_all(directory)
                .map_err(|error| StoreError::io("create directory", directory, error))?;
        }
        // Make the new directory entries durable where the platform allows it.
        // A filesystem root has no parent to synchronize.
        if let Some(parent) = self.root.parent().filter(|parent| parent.is_dir()) {
            atomic::sync_directory(parent)?;
        }
        atomic::sync_directory(&self.root)?;
        Ok(())
    }

    /// Rejects a non-notebook directory that already contains something.
    fn refuse_unexpected_tree(&self) -> Result<(), StoreError> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(StoreError::io("read directory", &self.root, error)),
        };
        for entry in entries {
            let entry =
                entry.map_err(|error| StoreError::io("read directory", &self.root, error))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let expected = name == PRIVATE_DIR
                || RESERVED_DIRECTORIES.contains(&name.as_str())
                || name == "README.md"
                || name == "fieldnotes.base";
            if !expected {
                return Err(StoreError::UnexpectedTree {
                    path: self.root.clone(),
                    entry: name,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_test_support::TempDir;

    fn temp(label: &str) -> Result<TempDir, StoreError> {
        TempDir::new(label)
            .map_err(|error| StoreError::io("create temporary directory", ".", error))
    }

    #[test]
    fn create_lays_out_the_reserved_directories() -> Result<(), StoreError> {
        let temp = temp("layout")?;
        let root = temp.path().join("notebook");
        let (notebook, state) = Notebook::create(&root)?;
        assert_eq!(state, InitState::Created);
        for name in RESERVED_DIRECTORIES {
            assert!(root.join(name).is_dir(), "missing {name}");
        }
        assert!(notebook.private_dir().join("state").join("sync").is_dir());
        assert!(notebook.private_dir().join("cache").is_dir());
        Ok(())
    }

    #[test]
    fn create_refuses_an_unrelated_non_empty_directory() -> Result<(), StoreError> {
        let temp = temp("refuse")?;
        let root = temp.path().join("busy");
        std::fs::create_dir_all(&root)
            .map_err(|error| StoreError::io("create directory", &root, error))?;
        std::fs::write(root.join("unrelated.txt"), b"x")
            .map_err(|error| StoreError::io("write", &root, error))?;
        assert!(matches!(
            Notebook::create(&root),
            Err(StoreError::UnexpectedTree { .. })
        ));
        Ok(())
    }

    #[test]
    fn discovery_walks_up_and_reports_a_missing_notebook() -> Result<(), StoreError> {
        let temp = temp("discover")?;
        let root = temp.path().join("notebook");
        Notebook::create(&root)?;
        // Instance metadata is what identifies a notebook, so write one.
        std::fs::write(root.join(PRIVATE_DIR).join(INSTANCE_FILE), b"placeholder\n")
            .map_err(|error| StoreError::io("write", &root, error))?;
        let nested = root.join("notes");
        assert_eq!(Notebook::discover(&nested)?.root(), root.as_path());
        assert!(matches!(
            Notebook::discover(temp.path()),
            Err(StoreError::NotANotebook { .. })
        ));
        Ok(())
    }
}
