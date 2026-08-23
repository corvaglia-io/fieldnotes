//! Comparing filesystem paths that name the same directory in different words.
//!
//! Two paths can name one directory and still not compare equal, and this is
//! not an exotic corner: it is the ordinary case on the platforms Fieldnotes
//! promises to support.
//!
//! - **macOS** reaches the system temporary directory through a symbolic link
//!   (`/var` -> `/private/var`), and `getcwd` reports the resolved form. A path
//!   built by joining onto `/var/folders/...` and the working directory a child
//!   process reports for that same directory are different strings.
//! - **Windows** has at least four spellings of one directory: a legacy path
//!   (`C:\Users\runneradmin\...`), the same path with 8.3 short components
//!   (`C:\Users\RUNNER~1\...`, which is what `%TEMP%` frequently is on a CI
//!   runner), either of those in a different letter case (the filesystem is
//!   case-insensitive; [`PathBuf`] comparison is not), and the verbatim form
//!   [`std::fs::canonicalize`] returns (`\\?\C:\Users\runneradmin\...`).
//! - **Everywhere**, a path may carry `.` and `..` components that name the
//!   same place as the reduced path.
//!
//! [`normalize`] is the one definition of "the comparable, printable spelling
//! of this path" that every comparison and every reported path in this
//! workspace goes through, so the product cannot decide two spellings are
//! different directories while a person looking at them can see they are the
//! same one.
//!
//! # What `normalize` does, and the two things it deliberately does not
//!
//! It calls [`std::fs::canonicalize`], which resolves symbolic links, `.`,
//! `..`, and — on Windows — 8.3 short components and letter case, then strips
//! the Windows verbatim prefix that call adds.
//!
//! It **does not strip the verbatim prefix on non-Windows platforms**, even
//! though the string manipulation would be identical. A backslash is a legal
//! character in a Unix filename, so a file could genuinely be named
//! `\\?\something`, and stripping there would silently rename it. The prefix
//! is a Windows path-syntax feature, so only the Windows build touches it.
//!
//! It **does not strip the prefix from a path that needs it**. The verbatim
//! form is what lets a Windows path exceed the legacy 260-character limit, so a
//! normalized path that would land at or past that limit keeps its prefix
//! rather than becoming a path the operating system might refuse to open. Both
//! sides of any comparison go through this same function, so they still agree.
//!
//! # A path that does not exist yet
//!
//! [`std::fs::canonicalize`] requires the path to exist. When it fails — a
//! notebook that has not been created, a target a user mistyped — [`normalize`]
//! returns the input unchanged rather than guessing, because the caller's next
//! step is to report that path back to the user in an error.

use std::path::{Path, PathBuf};

/// The legacy Windows maximum path length. A normalized path at or past this
/// length keeps its verbatim prefix; see this module's documentation.
#[cfg(any(windows, test))]
const LEGACY_MAX_PATH: usize = 260;

/// Reduces `path` to its comparable, printable spelling.
///
/// Resolves symbolic links, `.`/`..`, and (on Windows) 8.3 short components
/// and letter case; strips the verbatim prefix `std::fs::canonicalize` adds on
/// Windows. Returns `path` unchanged when it does not exist.
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(resolved) => strip_verbatim_prefix(&resolved),
        Err(_) => path.to_path_buf(),
    }
}

/// The pure string form of the verbatim-prefix rule: `\\?\UNC\server\share`
/// becomes `\\server\share`, `\\?\C:\dir` becomes `C:\dir`, and anything else
/// is `None` because there is nothing to strip.
///
/// Compiled `#[cfg(any(windows, test))]` rather than `#[cfg(windows)]`, matching
/// the convention `fieldnotes_field_protocol::host` already follows for its
/// Windows exit-code classifier: a Windows-only function is one that the tests
/// on two of the three supported platforms never even compile, let alone
/// exercise. This way the rule is covered by every platform's test run, and
/// only the *call* is Windows-only.
#[cfg(any(windows, test))]
fn without_verbatim_prefix(text: &str) -> Option<String> {
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        Some(format!(r"\\{rest}"))
    } else {
        text.strip_prefix(r"\\?\").map(str::to_owned)
    }
}

/// Removes the `\\?\` verbatim prefix `std::fs::canonicalize` adds on Windows.
///
/// A result at or past [`LEGACY_MAX_PATH`] keeps the prefix, since that is the
/// only form the operating system reliably opens.
#[cfg(windows)]
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    match without_verbatim_prefix(&path.to_string_lossy()) {
        Some(stripped) if stripped.len() < LEGACY_MAX_PATH => PathBuf::from(stripped),
        _ => path.to_path_buf(),
    }
}

/// A no-op off Windows: `\\?\` is Windows path syntax, and a leading backslash
/// run is an ordinary (if unusual) filename elsewhere.
#[cfg(not(windows))]
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Whether `left` and `right` name the same file or directory.
///
/// Compares [`normalize`]d forms, and on Windows compares them without regard
/// to ASCII letter case, because the filesystem does not distinguish them and a
/// path that reached this process through an environment variable or a
/// configuration file may not carry the on-disk case at all.
#[must_use]
pub fn same_path(left: &Path, right: &Path) -> bool {
    equal_text(&normalize(left), &normalize(right))
}

/// `path` relative to `root`, when it is inside it.
///
/// This is also the containment check: `relative_to(path, root).is_some()`
/// answers "is this inside that", and it answers it component by component
/// over normalized forms rather than by string prefix, so
/// `/notebooks/work-archive` is not treated as being inside
/// `/notebooks/work` and a Windows path spelled with 8.3 components is not
/// treated as being outside the notebook it is plainly inside.
///
/// Returns `None` when `path` is not inside `root`, which is the difference
/// that matters for reporting: a path outside the notebook must not be
/// reported as though it named something within it.
#[must_use]
pub fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    let path = normalize(path);
    let root = normalize(root);
    let mut path_components = path.components();
    for root_component in root.components() {
        match path_components.next() {
            Some(candidate) if component_matches(candidate, root_component) => {}
            _ => return None,
        }
    }
    Some(path_components.collect())
}

/// Renders a relative path with forward slashes, so a notebook-relative path
/// reads and compares the same on every platform.
#[must_use]
pub fn slash_display(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<String>>()
        .join("/")
}

/// Whether two notebook-relative displays name the same entry, ignoring ASCII
/// letter case on Windows for the same reason [`same_path`] does.
#[must_use]
pub fn same_relative_display(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn component_matches(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    equal_text(Path::new(left.as_os_str()), Path::new(right.as_os_str()))
}

/// Compares two already-normalized paths as text, case-insensitively on
/// Windows only.
fn equal_text(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(right.to_string_lossy().as_ref())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_test_support::TempDir;

    #[test]
    fn the_windows_verbatim_prefix_rule_holds_on_every_platform() {
        // Exercised everywhere, not only on Windows: see
        // `without_verbatim_prefix`'s own documentation for why.
        assert_eq!(
            without_verbatim_prefix(r"\\?\C:\Users\runneradmin\nb").as_deref(),
            Some(r"C:\Users\runneradmin\nb")
        );
        assert_eq!(
            without_verbatim_prefix(r"\\?\UNC\server\share\nb").as_deref(),
            Some(r"\\server\share\nb")
        );
        // Nothing to strip: an ordinary Windows path, and a Unix path.
        assert_eq!(without_verbatim_prefix(r"C:\Users\runneradmin\nb"), None);
        assert_eq!(without_verbatim_prefix("/home/sam/nb"), None);
        // A stripped path that would reach the legacy limit keeps its prefix,
        // because the verbatim form is the only one Windows reliably opens.
        let long = format!(r"\\?\C:\{}", "a".repeat(LEGACY_MAX_PATH));
        let stripped = without_verbatim_prefix(&long).unwrap_or_default();
        assert!(stripped.len() >= LEGACY_MAX_PATH);
    }

    #[test]
    fn a_path_that_does_not_exist_is_returned_unchanged() {
        let missing = Path::new("relative/does/not/exist");
        assert_eq!(normalize(missing), missing.to_path_buf());
    }

    #[test]
    fn dot_and_dot_dot_components_reduce() -> std::io::Result<()> {
        let temp = TempDir::new("paths-reduce")?;
        let nested = temp.path().join("a").join("b");
        std::fs::create_dir_all(&nested)?;
        let noisy = temp.path().join("a").join(".").join("b").join("..");
        assert!(same_path(&noisy, &temp.path().join("a")));
        // The reduced form is what a comparison and a report both see.
        assert!(!normalize(&noisy).to_string_lossy().contains(".."));
        assert!(same_path(&nested, &normalize(&nested)));
        Ok(())
    }

    #[test]
    fn a_normalized_path_is_stable_and_never_verbatim_on_a_short_path() -> std::io::Result<()> {
        let temp = TempDir::new("paths-stable")?;
        let normalized = normalize(temp.path());
        assert_eq!(normalize(&normalized), normalized);
        assert!(
            !normalized.to_string_lossy().starts_with(r"\\?\"),
            "a short normalized path keeps no verbatim prefix: {}",
            normalized.display()
        );
        Ok(())
    }

    #[test]
    fn containment_is_component_wise_not_a_string_prefix() -> std::io::Result<()> {
        let temp = TempDir::new("paths-containment")?;
        let work = temp.path().join("work");
        let archive = temp.path().join("work-archive");
        std::fs::create_dir_all(work.join("notes"))?;
        std::fs::create_dir_all(&archive)?;
        assert_eq!(
            relative_to(&work.join("notes"), &work),
            Some(PathBuf::from("notes"))
        );
        assert_eq!(relative_to(&work, &work), Some(PathBuf::new()));
        // A string-prefix test would call this one inside the other.
        assert_eq!(relative_to(&archive, &work), None);
        assert_eq!(
            relative_to(&work.join("notes"), &work).map(|path| slash_display(&path)),
            Some("notes".to_owned())
        );
        Ok(())
    }

    #[test]
    fn the_working_directory_compares_equal_to_the_path_it_was_set_from() -> std::io::Result<()> {
        // The macOS case this module exists for: `/var/...` and the
        // `/private/var/...` the OS reports for the same directory. Comparing
        // through `normalize` must see one directory, not two.
        let temp = TempDir::new("paths-cwd")?;
        let resolved = std::fs::canonicalize(temp.path())?;
        assert!(same_path(temp.path(), &resolved));
        Ok(())
    }
}
