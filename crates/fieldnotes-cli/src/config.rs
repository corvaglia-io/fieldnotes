//! The persistent user profile: where it lives on a real machine, and the
//! precedence between a CLI flag, an environment variable, the profile, and
//! Fieldnotes' existing default behavior.
//!
//! This module is the *only* place that decides the real filesystem location
//! of the profile — every other crate, and every test in this one, is handed
//! an explicit path instead, so a test can never read or write a developer's
//! actual profile.
//!
//! # Precedence
//!
//! Every setting except the notebook resolves through the same four-tier
//! order, implemented once in [`pick`] rather than re-derived per setting:
//!
//! 1. an explicit CLI flag (`--offset`, and `sync`'s `--max-artifact-bytes` /
//!    `--media-type` / `--window`);
//! 2. an environment variable (`FIELDNOTES_TIMEZONE`, or the legacy
//!    `FIELDNOTES_UTC_OFFSET` if the newer name is unset; `FIELDNOTES_WINDOW_DAYS`
//!    for the window);
//! 3. the profile setting;
//! 4. an existing documented default: UTC for the offset, and — for the two
//!    artifact retention settings and the collection window — the protocol
//!    crate's own approved defaults (seven days for the window).
//!
//! The window's profile tier is implemented in [`resolve_window_days`] and
//! exercised by this module's own tests, but has no real file-backed profile
//! setting to read from yet: see that function's documentation.
//!
//! The notebook is the one exception, implemented once in
//! [`resolve_notebook`] rather than in [`pick`]:
//!
//! 1. an explicit `--notebook` flag;
//! 2. the `FIELDNOTES_NOTEBOOK` environment variable;
//! 3. discovery by walking up from the working directory;
//! 4. the profile's recorded default notebook.
//!
//! Discovery outranks the profile default here, unlike every other setting,
//! because standing inside a notebook is a strong, locally evident statement
//! of intent, whereas the profile default is a convenience for when the
//! working directory is nowhere near a notebook. A command that silently
//! operated on the profile's notebook while the caller stood inside a
//! different one was a real footgun this order removes.
//!
//! # Every notebook path is normalized before it is used or reported
//!
//! The three tiers reach this module in three different spellings of the same
//! kind of thing: a path a user typed, a path an earlier session recorded in
//! the profile, and the working directory as the operating system reports it.
//! Those spellings differ in ways that have nothing to do with which directory
//! is meant — a macOS `/var` symlink, a Windows 8.3 short component or letter
//! case, the `\\?\` verbatim prefix — so each one goes through
//! [`fieldnotes_app::paths::normalize`] before it is handed to discovery and
//! before it is recorded in the profile. The notebook root a command reports is
//! therefore the same string whichever tier resolved it, and two spellings of
//! one notebook are one notebook.

use std::path::{Path, PathBuf};

use fieldnotes_app::paths;
use fieldnotes_store::{Notebook, Profile, StoreError, write_profile};

/// Environment variable naming an explicit profile file path.
///
/// Set by every test in this workspace, so a test run never touches a real
/// user's profile: with this set, [`resolve_profile_path`] never looks at
/// `HOME`, `XDG_CONFIG_HOME`, or `APPDATA`.
pub const CONFIG_ENV: &str = "FIELDNOTES_CONFIG";

/// Environment variable naming the default notebook path.
pub const NOTEBOOK_ENV: &str = "FIELDNOTES_NOTEBOOK";

/// Environment variable naming the timezone setting.
///
/// Accepts the same grammar as `--offset` and the profile's `timezone` key:
/// `system`, a fixed `+HH:MM`/`-HH:MM`/`utc` offset, or an IANA zone name.
pub const TIMEZONE_ENV: &str = "FIELDNOTES_TIMEZONE";

/// Environment variable naming `sync`'s bounded collection window, in days.
///
/// Resolved through [`pick`] exactly like every other setting here: `--window`
/// wins, then this variable, then the profile (once [`Profile`] carries a
/// `window_days` setting — see this module's own precedence note below), then
/// [`fieldnotes_app::DEFAULT_WINDOW_DAYS`].
pub const WINDOW_ENV: &str = "FIELDNOTES_WINDOW_DAYS";

/// The profile filename inside its platform-specific directory.
const PROFILE_FILENAME: &str = "config";

/// Resolves the real user profile path.
///
/// [`CONFIG_ENV`] wins outright. Otherwise the conventional per-platform
/// location is used: `%APPDATA%\fieldnotes\config` on Windows, `~/Library/
/// Application Support/fieldnotes/config` on macOS, and
/// `$XDG_CONFIG_HOME/fieldnotes/config` (falling back to `~/.config`) on
/// Linux and other Unix systems. `None` means neither the override nor the
/// platform's home/config variable is available, which a caller should treat
/// as "no profile can be located" rather than guessing a path.
#[must_use]
pub fn resolve_profile_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(CONFIG_ENV) {
        return Some(PathBuf::from(path));
    }
    default_profile_path()
}

/// The conventional per-platform profile path, ignoring [`CONFIG_ENV`].
fn default_profile_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("fieldnotes")
                .join(PROFILE_FILENAME)
        })
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|dir| PathBuf::from(dir).join("fieldnotes").join(PROFILE_FILENAME))
    } else {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
        config_home.map(|dir| dir.join("fieldnotes").join(PROFILE_FILENAME))
    }
}

/// Chooses the highest-precedence value among a flag, an environment
/// variable, and a profile setting.
///
/// This is the one function that encodes "flag, then environment, then
/// profile" for every setting this feature adds, so the order is implemented
/// once instead of being re-derived per setting.
fn pick<T>(flag: Option<T>, env: Option<T>, profile: Option<T>) -> Option<T> {
    flag.or(env).or(profile)
}

/// Where the notebook a command operates on came from.
///
/// Distinguished from the other settings' plain "which value won" outcome
/// because [`Profile`] is the one source worth telling a user about: it is
/// not locally evident the way an explicit flag or the working directory
/// is, so a command that resolved to it can silently touch a notebook other
/// than the one the caller is standing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotebookSource {
    /// An explicit `--notebook` flag, or the `FIELDNOTES_NOTEBOOK`
    /// environment variable when the flag is absent.
    Explicit,
    /// Discovered by walking up from the working directory.
    WorkingDirectory,
    /// The profile's recorded default notebook.
    Profile,
}

/// Resolves the notebook a command should operate on, and how that choice
/// was made.
///
/// Precedence, highest to lowest:
///
/// 1. `explicit` — the `--notebook` flag, or [`NOTEBOOK_ENV`] when the flag
///    is absent; the caller has already merged those two, since they share a
///    tier;
/// 2. discovery by walking up from `cwd`;
/// 3. `profile_notebook`, the profile's recorded default.
///
/// This is the single place the notebook's precedence is decided; every
/// caller that needs a notebook goes through this function rather than
/// re-deriving the order. `explicit` and `profile_notebook` are each
/// re-discovered from (rather than assumed to already be) a notebook root,
/// matching how `--notebook` has always tolerated naming a subdirectory of
/// the notebook.
///
/// On failure, the error is discovery's own `cwd`-rooted failure when no
/// profile default exists to fall back to, since that is the message that
/// matches what the caller actually tried.
///
/// Each tier's path is normalized ([`paths::normalize`]) before discovery walks
/// it, so the notebook root this returns carries one spelling regardless of
/// which tier supplied it and regardless of how that tier happened to spell it.
/// [`Notebook::discover`] returns an ancestor of the path it was given, so
/// normalizing the input is what normalizes the root.
pub fn resolve_notebook(
    explicit: Option<&Path>,
    cwd: &Path,
    profile_notebook: Option<&Path>,
) -> Result<(Notebook, NotebookSource), StoreError> {
    if let Some(path) = explicit {
        return Notebook::discover(&paths::normalize(path))
            .map(|notebook| (notebook, NotebookSource::Explicit));
    }
    match Notebook::discover(&paths::normalize(cwd)) {
        Ok(notebook) => Ok((notebook, NotebookSource::WorkingDirectory)),
        Err(discovery_error) => match profile_notebook {
            Some(path) => Notebook::discover(&paths::normalize(path))
                .map(|notebook| (notebook, NotebookSource::Profile)),
            None => Err(discovery_error),
        },
    }
}

/// Resolves the raw timezone spec text from the flag/env/profile tiers,
/// leaving the UTC fallback (tier four) to the caller.
///
/// `env_timezone` is [`TIMEZONE_ENV`]; `env_legacy_offset` is the older
/// `FIELDNOTES_UTC_OFFSET` variable, kept working when the newer name is
/// unset so existing scripts are not broken by this feature.
#[must_use]
pub fn resolve_timezone_text(
    flag: Option<String>,
    env_timezone: Option<String>,
    env_legacy_offset: Option<String>,
    profile_timezone: Option<String>,
) -> Option<String> {
    pick(flag, env_timezone.or(env_legacy_offset), profile_timezone)
}

/// Resolves `sync`'s bounded-window length in days from the flag/env/profile
/// tiers, leaving the [`fieldnotes_app::DEFAULT_WINDOW_DAYS`] fallback (tier
/// four) to the caller — the same shape [`resolve_timezone_text`] uses.
///
/// `profile_window_days` comes from [`Profile`]'s `window_days` setting, so
/// all four tiers are live: a user can record a window once rather than
/// passing it on every run.
#[must_use]
pub fn resolve_window_days(
    flag: Option<u64>,
    env: Option<u64>,
    profile_window_days: Option<u64>,
) -> Option<u64> {
    pick(flag, env, profile_window_days)
}

/// Validates that `path` is (or is inside) an initialized Fieldnotes
/// notebook, and returns that notebook's root.
///
/// Discovery (walking up from `path`) is used rather than requiring an exact
/// root, matching how `--notebook` already behaves elsewhere in the CLI. The
/// returned root is normalized, so what lands in the profile is the spelling
/// every later comparison will produce rather than whatever the caller typed.
pub fn validate_notebook_path(path: &Path) -> Result<PathBuf, StoreError> {
    Notebook::discover(&paths::normalize(path)).map(|notebook| notebook.root().to_path_buf())
}

/// Records `notebook_root` as the profile's default notebook after
/// validating it, replacing any existing notebook setting.
pub fn set_notebook(
    profile_path: &Path,
    profile: &Profile,
    path: &Path,
) -> Result<Profile, StoreError> {
    let root = validate_notebook_path(path)?;
    let mut updated = profile.clone();
    updated.notebook = Some(root);
    write_profile(profile_path, &updated)?;
    Ok(updated)
}

/// Records `timezone` as the profile's default timezone, replacing any
/// existing timezone setting. The caller is responsible for having already
/// validated `timezone` with [`crate::timezone::TimeZoneSpec::parse`].
pub fn set_timezone(
    profile_path: &Path,
    profile: &Profile,
    timezone: &str,
) -> Result<Profile, StoreError> {
    let mut updated = profile.clone();
    updated.timezone = Some(timezone.to_owned());
    write_profile(profile_path, &updated)?;
    Ok(updated)
}

/// Records `bytes` as the profile's default single-artifact retention
/// threshold.
///
/// A2 section 14 makes this a configurable *default*, not a ceiling: a notebook
/// may move it in either direction between the product's minimum and the frozen
/// 512 MiB ceiling. The caller validates the value against that ceiling before
/// calling this.
pub fn set_artifact_max_bytes(
    profile_path: &Path,
    profile: &Profile,
    bytes: u64,
) -> Result<Profile, StoreError> {
    let mut updated = profile.clone();
    updated.artifact_max_bytes = Some(bytes);
    write_profile(profile_path, &updated)?;
    Ok(updated)
}

/// Records `media_types` as the profile's default artifact media-type
/// retention include set, as written.
///
/// The caller validates each entry against the media-type matcher grammar
/// before calling this.
pub fn set_artifact_media_types(
    profile_path: &Path,
    profile: &Profile,
    media_types: &str,
) -> Result<Profile, StoreError> {
    let mut updated = profile.clone();
    updated.artifact_media_types = Some(media_types.to_owned());
    write_profile(profile_path, &updated)?;
    Ok(updated)
}

/// Splits a comma-separated media-type include set into its entries, dropping
/// empty ones.
#[must_use]
pub fn split_media_types(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Records `notebook_root` as the profile's default notebook only if no
/// notebook is already recorded, or if `force` is set.
///
/// Used by `init`: a brand-new profile is a reasonable place to remember the
/// first notebook a user creates, but `init` must never silently overwrite a
/// default the user (or an earlier `init`) already chose.
///
/// The recorded path is normalized, so a later run comparing the profile
/// default against a working directory or an explicit flag compares one
/// spelling against another spelling of the same shape rather than against
/// whatever `init` happened to be given.
pub fn record_default_notebook_if_absent(
    profile_path: &Path,
    profile: &Profile,
    notebook_root: &Path,
    force: bool,
) -> Result<bool, StoreError> {
    if profile.notebook.is_some() && !force {
        return Ok(false);
    }
    let mut updated = profile.clone();
    updated.notebook = Some(paths::normalize(notebook_root));
    write_profile(profile_path, &updated)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_domain::{Datetime, RecordId};
    use fieldnotes_format::InstanceMetadata;
    use fieldnotes_store::write_instance;
    use fieldnotes_test_support::TempDir;

    fn temp(label: &str) -> Result<TempDir, StoreError> {
        TempDir::new(label)
            .map_err(|error| StoreError::io("create temporary directory", ".", error))
    }

    /// Initializes a notebook under `parent` and returns its root.
    ///
    /// A notebook is identified by `.fieldnotes/instance.yaml`, which
    /// `Notebook::create` alone does not write (that is `fieldnotes-app`'s
    /// `init` use case), so this hand-writes a minimal instance record the
    /// same way the store crate's own tests do.
    fn notebook_at(parent: &Path, name: &str) -> Result<PathBuf, StoreError> {
        let root = parent.join(name);
        let (notebook, _) = Notebook::create(&root)?;
        let instance_id =
            RecordId::parse("fn_01a02837-2de0-7a2b-8c41-f2481851192a").map_err(|_| {
                StoreError::NotANotebook {
                    start: root.clone(),
                }
            })?;
        let created_at =
            Datetime::parse("2026-08-22T08:45:00+02:00").map_err(|_| StoreError::NotANotebook {
                start: root.clone(),
            })?;
        let metadata = InstanceMetadata {
            instance_id,
            created_at,
            name: None,
        };
        write_instance(&notebook, &metadata)?;
        Ok(root)
    }

    #[test]
    fn notebook_resolution_favors_explicit_then_discovery_then_profile() -> Result<(), StoreError> {
        let temp = temp("notebook-resolution")?;
        let explicit_notebook = notebook_at(temp.path(), "explicit")?;
        let cwd_notebook = notebook_at(temp.path(), "cwd")?;
        let profile_notebook = notebook_at(temp.path(), "profile")?;

        // Every notebook identity below is asserted with `paths::same_path`
        // rather than `==`: resolution reports the normalized spelling, and a
        // temporary directory is reached through a symlink on macOS and can
        // carry 8.3 components on Windows. The assertion is about *which
        // notebook* won, which is exactly what a normalized comparison
        // answers; a raw `PathBuf` comparison would answer "which spelling".
        let names = |notebook: &Notebook, expected: &Path| {
            assert!(
                paths::same_path(notebook.root(), expected),
                "expected the notebook at {}, got {}",
                expected.display(),
                notebook.root().display()
            );
        };

        // Tier 1: the explicit flag/environment path wins over everything
        // else, even when standing inside a different notebook.
        let (notebook, source) = resolve_notebook(
            Some(&explicit_notebook),
            &cwd_notebook,
            Some(&profile_notebook),
        )?;
        names(&notebook, &explicit_notebook);
        assert_eq!(source, NotebookSource::Explicit);

        // Tier 2: no explicit path, so discovery from the working directory
        // wins over the profile default. This is the motivating case: a
        // profile default pointing elsewhere must never outrank the
        // notebook the caller is standing in.
        let (notebook, source) = resolve_notebook(None, &cwd_notebook, Some(&profile_notebook))?;
        names(&notebook, &cwd_notebook);
        assert_eq!(source, NotebookSource::WorkingDirectory);

        // Tier 3: the working directory is not inside any notebook, so the
        // profile default is used as the final fallback.
        let unrelated = temp.path().join("unrelated-cwd");
        std::fs::create_dir_all(&unrelated)
            .map_err(|error| StoreError::io("create directory", &unrelated, error))?;
        let (notebook, source) = resolve_notebook(None, &unrelated, Some(&profile_notebook))?;
        names(&notebook, &profile_notebook);
        assert_eq!(source, NotebookSource::Profile);

        // Nothing resolves: the working directory is not in a notebook and
        // there is no profile default, so discovery's own error is reported.
        assert!(matches!(
            resolve_notebook(None, &unrelated, None),
            Err(StoreError::NotANotebook { .. })
        ));
        Ok(())
    }

    #[test]
    fn timezone_precedence_prefers_the_new_environment_name_over_the_legacy_one() {
        assert_eq!(
            resolve_timezone_text(
                None,
                Some("Europe/Zurich".to_owned()),
                Some("+02:00".to_owned()),
                None,
            ),
            Some("Europe/Zurich".to_owned())
        );
        // The legacy variable still works when the new one is unset, so
        // existing scripts setting FIELDNOTES_UTC_OFFSET are not broken.
        assert_eq!(
            resolve_timezone_text(None, None, Some("+02:00".to_owned()), None),
            Some("+02:00".to_owned())
        );
        // A flag beats both environment variables.
        assert_eq!(
            resolve_timezone_text(
                Some("system".to_owned()),
                Some("Europe/Zurich".to_owned()),
                Some("+02:00".to_owned()),
                Some("+05:00".to_owned()),
            ),
            Some("system".to_owned())
        );
        assert_eq!(resolve_timezone_text(None, None, None, None), None);
    }

    #[test]
    fn window_days_precedence_is_flag_then_environment_then_profile_then_default() {
        // Nothing set anywhere: the caller falls back to the documented
        // default (`fieldnotes_app::DEFAULT_WINDOW_DAYS`), which this
        // function itself does not know about — it only decides which of
        // the three sources it was given wins.
        assert_eq!(resolve_window_days(None, None, None), None);

        // Tier 3: only the profile is set.
        assert_eq!(resolve_window_days(None, None, Some(14)), Some(14));

        // Tier 2: the environment variable beats the profile.
        assert_eq!(resolve_window_days(None, Some(3), Some(14)), Some(3));

        // Tier 1: an explicit flag beats both the environment and the
        // profile.
        assert_eq!(resolve_window_days(Some(1), Some(3), Some(14)), Some(1));
    }

    #[test]
    fn the_default_profile_path_is_platform_shaped() {
        // A smoke test rather than an exhaustive one: the real per-platform
        // paths are exercised by the end-to-end binary tests through
        // `FIELDNOTES_CONFIG`, which every test uses instead of a real HOME.
        if let Some(path) = default_profile_path() {
            // `Path::ends_with` matches components, so this holds regardless
            // of the platform's own separator character.
            assert!(path.ends_with("fieldnotes/config"));
        }
    }
}
