//! Reading and writing the persistent user profile: a default notebook path
//! and a timezone setting, recorded outside any notebook.
//!
//! This module only knows the file's on-disk shape and mechanics. It does not
//! decide where the file lives on a real machine (that is the composition
//! root's job, since it means reading `XDG_CONFIG_HOME`, `HOME`, or `APPDATA`)
//! and it does not interpret the timezone string (that is `fieldnotes-cli`'s
//! job, since resolving a named zone to a numeric offset needs a timezone
//! database). Keeping both out of this crate is what lets every test here run
//! against an explicit temporary path instead of a developer's real profile.
//!
//! The format is a small hand-editable `key = value` text file, one setting
//! per line, blank lines and `#` comments ignored:
//!
//! ```text
//! # Fieldnotes user profile
//! notebook = /home/user/notebooks/work
//! timezone = Europe/Zurich
//! artifact_max_bytes = 26214400
//! artifact_media_types = application/pdf,image/png,text/plain
//! ```
//!
//! An unrecognized key, a duplicate key, an empty value, or a line that is not
//! `key = value` is rejected with the offending line number rather than
//! silently ignored: the file is meant to be hand-edited, and a config format
//! that quietly drops a typo would hide the very mistake a user needs to see.
//! A missing file is not an error — it simply means no profile has been
//! recorded yet.

use std::path::{Path, PathBuf};

use crate::atomic;
use crate::error::StoreError;

/// The settings a Fieldnotes user profile may record.
///
/// Values are kept as close to the written text as this crate can interpret
/// them without taking a dependency it should not have. Interpreting
/// `timezone` (a fixed offset, `system`, or an IANA zone name) is left to the
/// caller, so this crate never needs a timezone-database dependency, and
/// `artifact_media_types` stays raw comma-separated text because the media-type
/// matcher grammar belongs to the Field-protocol crate, which storage must not
/// depend on. `artifact_max_bytes` is parsed here, because an integer needs no
/// outside vocabulary and a value that is not one is a malformed profile the
/// user should be told about by line number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// The default notebook path, when recorded.
    pub notebook: Option<PathBuf>,
    /// The default timezone setting, when recorded, exactly as written.
    pub timezone: Option<String>,
    /// The configured single-artifact retention threshold in bytes, when
    /// recorded.
    ///
    /// A2 section 14 makes this a configurable *default* rather than a
    /// ceiling: a notebook may move it in either direction between the
    /// product's minimum and the frozen 512 MiB ceiling. Checking it against
    /// that ceiling needs the protocol crate's own limit table, so it happens
    /// where the collection request is built, not here.
    pub artifact_max_bytes: Option<u64>,
    /// The configured media-type retention include set, as written: a
    /// comma-separated list of exact `type/subtype` media types or subtype
    /// wildcards such as `image/*`.
    pub artifact_media_types: Option<String>,
    /// How many days a first collection run reaches back.
    ///
    /// Only a run with no durable cursor is bounded this way: once a Field has
    /// a cursor, its own incremental mechanism decides what is new, so sending
    /// a window would replace incremental collection with a repeated bounded
    /// read. Validating the value against the protocol's own bounds happens
    /// where the collection request is built, not here.
    pub window_days: Option<u32>,
}

/// Reads a user profile from `path`.
///
/// A missing file returns [`Profile::default`] rather than an error, since no
/// profile having been recorded yet is normal, expected state for a first-time
/// user. A present but malformed file fails with [`StoreError::InvalidProfile`]
/// instead of silently falling back to unset settings.
pub fn read_profile(path: &Path) -> Result<Profile, StoreError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Profile::default());
        }
        Err(error) => return Err(StoreError::io("read profile", path, error)),
    };
    parse_profile(&bytes).map_err(|message| StoreError::InvalidProfile {
        path: path.to_path_buf(),
        message,
    })
}

/// Writes a user profile atomically.
///
/// This reuses the same staged-file/rename machinery a notebook's own records
/// use ([`atomic::write_atomic`]) rather than a second write path, so a crash
/// mid-write leaves either the previous profile or the new one, never a
/// truncated file. The containing directory is created if it does not exist
/// yet, since a freshly installed Fieldnotes has no `~/.config/fieldnotes` (or
/// platform equivalent) directory at all.
pub fn write_profile(path: &Path, profile: &Profile) -> Result<(), StoreError> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(directory)
        .map_err(|error| StoreError::io("create profile directory", directory, error))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StoreError::InvalidProfile {
            path: path.to_path_buf(),
            message: "profile path has no filename component".to_owned(),
        })?;
    let text = render_profile(profile);
    atomic::write_atomic(directory, filename, text.as_bytes())?;
    Ok(())
}

/// Parses the hand-editable profile text format.
fn parse_profile(bytes: &[u8]) -> Result<Profile, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| "profile file is not valid UTF-8".to_owned())?;
    let mut profile = Profile::default();
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {line_number}: expected `key = value`, found `{raw_line}`"
            ));
        };
        let key = key.trim();
        let value = value.trim();
        if value.is_empty() {
            return Err(format!("line {line_number}: `{key}` has no value"));
        }
        if !SETTINGS.contains(&key) {
            return Err(format!(
                "line {line_number}: unknown setting `{key}`; recognized settings are {}",
                SETTINGS.join(", ")
            ));
        }
        if !seen.insert(key) {
            return Err(format!("line {line_number}: `{key}` is set more than once"));
        }
        match key {
            "notebook" => profile.notebook = Some(PathBuf::from(value)),
            "timezone" => profile.timezone = Some(value.to_owned()),
            "artifact_max_bytes" => {
                profile.artifact_max_bytes = Some(value.parse::<u64>().map_err(|_| {
                    format!(
                        "line {line_number}: `artifact_max_bytes` is a byte count, not `{value}`"
                    )
                })?);
            }
            "artifact_media_types" => profile.artifact_media_types = Some(value.to_owned()),
            "window_days" => {
                profile.window_days = Some(value.parse::<u32>().map_err(|_| {
                    format!("line {line_number}: `window_days` is a day count, not `{value}`")
                })?);
            }
            // Unreachable: `SETTINGS` above is the single source of the
            // recognized set, and an unrecognized key already returned.
            other => return Err(format!("line {line_number}: unhandled setting `{other}`")),
        }
    }
    Ok(profile)
}

/// Every recognized profile setting name, in the order `show` reports them.
const SETTINGS: [&str; 5] = [
    "notebook",
    "timezone",
    "artifact_max_bytes",
    "artifact_media_types",
    "window_days",
];

/// Renders a profile back to text, in a fixed key order so writes of the same
/// settings always produce the same bytes.
fn render_profile(profile: &Profile) -> String {
    let mut text = String::new();
    if let Some(notebook) = &profile.notebook {
        text.push_str("notebook = ");
        text.push_str(&notebook.display().to_string());
        text.push('\n');
    }
    if let Some(timezone) = &profile.timezone {
        text.push_str("timezone = ");
        text.push_str(timezone);
        text.push('\n');
    }
    if let Some(bytes) = profile.artifact_max_bytes {
        text.push_str("artifact_max_bytes = ");
        text.push_str(&bytes.to_string());
        text.push('\n');
    }
    if let Some(media_types) = &profile.artifact_media_types {
        text.push_str("artifact_media_types = ");
        text.push_str(media_types);
        text.push('\n');
    }
    if let Some(days) = profile.window_days {
        text.push_str("window_days = ");
        text.push_str(&days.to_string());
        text.push('\n');
    }
    text
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
    fn a_missing_profile_is_not_an_error() -> Result<(), StoreError> {
        let temp = temp("profile-missing")?;
        let path = temp.path().join("config");
        assert_eq!(read_profile(&path)?, Profile::default());
        Ok(())
    }

    #[test]
    fn an_empty_profile_parses_as_default() -> Result<(), StoreError> {
        let temp = temp("profile-empty")?;
        let path = temp.path().join("config");
        std::fs::write(&path, b"").map_err(|error| StoreError::io("write", &path, error))?;
        assert_eq!(read_profile(&path)?, Profile::default());

        // Comments and blank lines are also empty in effect.
        std::fs::write(&path, b"# just a comment\n\n  \n")
            .map_err(|error| StoreError::io("write", &path, error))?;
        assert_eq!(read_profile(&path)?, Profile::default());
        Ok(())
    }

    #[test]
    fn set_then_show_round_trips_through_an_atomic_write() -> Result<(), StoreError> {
        let temp = temp("profile-roundtrip")?;
        let path = temp.path().join("nested").join("config");
        let profile = Profile {
            notebook: Some(PathBuf::from("/notebooks/work")),
            timezone: Some("Europe/Zurich".to_owned()),
            artifact_max_bytes: Some(26_214_400),
            window_days: Some(7),
            artifact_media_types: Some("application/pdf,image/*".to_owned()),
        };
        write_profile(&path, &profile)?;
        assert_eq!(read_profile(&path)?, profile);

        // Writing again replaces the file atomically rather than appending.
        let updated = Profile {
            notebook: Some(PathBuf::from("/notebooks/home")),
            timezone: Some("system".to_owned()),
            artifact_max_bytes: None,
            window_days: None,
            artifact_media_types: None,
        };
        write_profile(&path, &updated)?;
        assert_eq!(read_profile(&path)?, updated);

        // No staging litter survives a successful write.
        let entries = std::fs::read_dir(path.parent().unwrap_or(temp.path()))
            .map_err(|error| StoreError::io("read directory", &path, error))?;
        for entry in entries {
            let entry = entry.map_err(|error| StoreError::io("read directory", &path, error))?;
            let name = entry.file_name();
            assert!(!atomic::is_staged_name(&name.to_string_lossy()));
        }
        Ok(())
    }

    /// Asserts that reading `contents` fails as [`StoreError::InvalidProfile`]
    /// whose message contains `needle`, without `.expect_err()` (banned by the
    /// workspace's clippy configuration).
    fn assert_rejected(path: &Path, contents: &[u8], needle: &str) -> Result<(), StoreError> {
        std::fs::write(path, contents).map_err(|error| StoreError::io("write", path, error))?;
        match read_profile(path) {
            Ok(profile) => panic!("expected `{needle}` to be rejected, parsed as {profile:?}"),
            Err(StoreError::InvalidProfile { message, .. }) => {
                assert!(
                    message.contains(needle),
                    "message `{message}` should contain `{needle}`"
                );
                Ok(())
            }
            Err(other) => panic!("expected InvalidProfile, got {other}"),
        }
    }

    #[test]
    fn a_malformed_profile_fails_loudly_with_the_offending_line() -> Result<(), StoreError> {
        let temp = temp("profile-malformed")?;
        let path = temp.path().join("config");

        assert_rejected(&path, b"notebook /no/equals/sign\n", "line 1")?;
        assert_rejected(
            &path,
            b"credential = do-not-support-this\n",
            "unknown setting",
        )?;
        assert_rejected(&path, b"notebook = /a\nnotebook = /b\n", "more than once")?;
        assert_rejected(&path, b"timezone =   \n", "no value")?;
        assert_rejected(
            &path,
            b"artifact_max_bytes = twenty-five megabytes\n",
            "byte count",
        )?;
        Ok(())
    }

    #[test]
    fn the_retention_settings_round_trip() -> Result<(), StoreError> {
        let temp = temp("profile-retention")?;
        let path = temp.path().join("config");
        std::fs::write(
            &path,
            b"artifact_max_bytes = 1048576\nartifact_media_types = image/*, application/pdf\n",
        )
        .map_err(|error| StoreError::io("write", &path, error))?;
        let profile = read_profile(&path)?;
        assert_eq!(profile.artifact_max_bytes, Some(1_048_576));
        assert_eq!(
            profile.artifact_media_types.as_deref(),
            Some("image/*, application/pdf")
        );
        Ok(())
    }
}
