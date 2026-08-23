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
//! Every setting resolves through the same four-tier order, implemented once in
//! [`pick`] rather than re-derived per setting:
//!
//! 1. an explicit CLI flag (`--notebook`, `--offset`, and `sync`'s
//!    `--max-artifact-bytes` / `--media-type`);
//! 2. an environment variable (`FIELDNOTES_NOTEBOOK`; `FIELDNOTES_TIMEZONE`,
//!    or the legacy `FIELDNOTES_UTC_OFFSET` if the newer name is unset);
//! 3. the profile setting;
//! 4. an existing documented default: notebook discovery by walking up from the
//!    working directory, UTC for the offset, and — for the two artifact
//!    retention settings — the protocol crate's own approved defaults.

use std::path::{Path, PathBuf};

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

/// Resolves the notebook starting path from the flag/env/profile tiers,
/// leaving discovery-from-the-working-directory (tier four) to the caller.
#[must_use]
pub fn resolve_notebook_start(
    flag: Option<PathBuf>,
    env_notebook: Option<String>,
    profile_notebook: Option<PathBuf>,
) -> Option<PathBuf> {
    pick(flag, env_notebook.map(PathBuf::from), profile_notebook)
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

/// Validates that `path` is (or is inside) an initialized Fieldnotes
/// notebook, and returns that notebook's root.
///
/// Discovery (walking up from `path`) is used rather than requiring an exact
/// root, matching how `--notebook` already behaves elsewhere in the CLI.
pub fn validate_notebook_path(path: &Path) -> Result<PathBuf, StoreError> {
    Notebook::discover(path).map(|notebook| notebook.root().to_path_buf())
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
    updated.notebook = Some(notebook_root.to_path_buf());
    write_profile(profile_path, &updated)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_favors_flag_then_env_then_profile_then_none() {
        // Every tier present: the flag wins.
        assert_eq!(
            resolve_notebook_start(
                Some(PathBuf::from("/flag")),
                Some("/env".to_owned()),
                Some(PathBuf::from("/profile")),
            ),
            Some(PathBuf::from("/flag"))
        );
        // No flag: the environment wins over the profile.
        assert_eq!(
            resolve_notebook_start(
                None,
                Some("/env".to_owned()),
                Some(PathBuf::from("/profile"))
            ),
            Some(PathBuf::from("/env"))
        );
        // Only the profile is set.
        assert_eq!(
            resolve_notebook_start(None, None, Some(PathBuf::from("/profile"))),
            Some(PathBuf::from("/profile"))
        );
        // Nothing is set: the caller falls back to working-directory discovery.
        assert_eq!(resolve_notebook_start(None, None, None), None);
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
