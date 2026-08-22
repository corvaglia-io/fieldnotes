//! Durable, non-secret per-Field configuration, and the on-disk locations of
//! the operational sync state a later `sync` phase will populate.
//!
//! Four state classes apply to a Fieldnotes notebook (see
//! `docs/operations.md`), and this module deliberately keeps two of them
//! apart:
//!
//! - **durable local intent/config** — one JSON file per configured external
//!   Field under `.fieldnotes/fields/<field_id>.json`, holding how to invoke
//!   it, its non-secret connector configuration, whether it is enabled, and
//!   its snapshotted `describe` manifest. This is what `fields add/list/
//!   remove` and manifest snapshotting operate on;
//! - **operational sync state** — an opaque per-Field cursor file and a
//!   forward-compatible last-run summary under `.fieldnotes/state/sync/`,
//!   named by convention here so a later `sync` implementation can start
//!   writing them without a schema change to this module. Nothing in this
//!   crate ever writes those files; only their paths and read side are
//!   provided.
//!
//! Field configuration is not part of the public notebook contract A1
//! governs, so this module is free to choose its own on-disk shape. JSON
//! (via `serde`/`serde_json`, already workspace dependencies) is used instead
//! of the user profile's hand-rolled `key = value` grammar because a Field's
//! configuration genuinely nests: a flat scalar map, and a stored `describe`
//! manifest that is itself a deep structure the format crate does not know
//! about — this crate stores it as an opaque JSON value and leaves typed
//! interpretation to `fieldnotes-app`, which is the crate allowed to depend
//! on the Field protocol.
//!
//! Every write goes through [`crate::atomic::write_atomic`], so a crash mid
//! write leaves either the previous complete configuration file or the new
//! one, never a truncated one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic;
use crate::error::StoreError;
use crate::layout::Notebook;

/// The private subdirectory holding one configuration file per external
/// Field.
const FIELDS_DIR: &str = "fields";

/// The private subdirectory segments holding operational sync state.
const SYNC_STATE_DIR: [&str; 2] = ["state", "sync"];

/// Configuration key names refused by name because they are unambiguously
/// meant to hold a credential, never a scan of the values themselves.
///
/// Field configuration has no credential field: a `0.1.3` credential-profile
/// *reference* is a name, not a secret, and belongs in `config` like any
/// other non-secret setting. These names are refused because, by their own
/// name, they could hold nothing else.
const FORBIDDEN_CONFIG_KEYS: &[&str] = &[
    "password",
    "passwd",
    "token",
    "access_token",
    "refresh_token",
    "secret",
    "client_secret",
    "api_key",
    "apikey",
    "credential",
    "credentials",
    "private_key",
    "cookie",
];

/// A path serialized and parsed as display text, matching
/// [`crate::profile`]'s convention rather than relying on `PathBuf`'s default
/// `serde` representation, which is not guaranteed for non-UTF-8 paths.
mod path_as_text {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&path.display().to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        let text = String::deserialize(deserializer)?;
        Ok(PathBuf::from(text))
    }
}

/// Durable, non-secret configuration for one configured external Field.
///
/// `manifest` is the last accepted `describe` manifest, stored verbatim as an
/// opaque JSON value. This crate does not interpret it; `fieldnotes-app`
/// decodes it into a typed manifest and compares it against a freshly
/// reported one when a migration check is needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldConfig {
    /// The validated Field ID (`<registered-stem>_<label>`).
    pub id: String,
    /// Whether this Field currently participates in sync.
    pub enabled: bool,
    /// The pinned path to the Field's executable.
    ///
    /// Fieldnotes never searches `PATH` or otherwise discovers a Field
    /// executable implicitly (see `docs/security.md`), so this path is
    /// always explicit and always supplied by whoever configured the Field.
    #[serde(with = "path_as_text")]
    pub executable: PathBuf,
    /// Flat, non-secret connector configuration.
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    /// The last accepted `describe` manifest, verbatim, or `None` until one
    /// has been recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<serde_json::Value>,
}

impl FieldConfig {
    /// Builds a new, enabled Field configuration with no manifest snapshot
    /// yet and no connector configuration yet.
    #[must_use]
    pub fn new(id: impl Into<String>, executable: impl Into<PathBuf>) -> Self {
        FieldConfig {
            id: id.into(),
            enabled: true,
            executable: executable.into(),
            config: BTreeMap::new(),
            manifest: None,
        }
    }
}

/// Rejects a `config` map that uses a credential-shaped key name.
///
/// This checks key names only, never values: Fieldnotes performs no secret
/// scanning of stored content (ADR 0006 ruling 3). It exists only because the
/// configuration schema itself has no credential field, so a key that could
/// only ever hold a secret is refused before it reaches disk.
pub fn reject_credential_shaped_keys(config: &BTreeMap<String, String>) -> Result<(), StoreError> {
    for key in config.keys() {
        let normalized = key.to_ascii_lowercase().replace(['-', ' '], "_");
        if FORBIDDEN_CONFIG_KEYS.contains(&normalized.as_str()) {
            return Err(StoreError::CredentialShapedConfigKey { key: key.clone() });
        }
    }
    Ok(())
}

fn fields_dir(notebook: &Notebook) -> PathBuf {
    notebook.private_dir().join(FIELDS_DIR)
}

fn field_config_filename(field_id: &str) -> String {
    format!("{field_id}.json")
}

/// The `.fieldnotes/fields/<field_id>.json` path for `field_id`.
#[must_use]
pub fn field_config_path(notebook: &Notebook, field_id: &str) -> PathBuf {
    fields_dir(notebook).join(field_config_filename(field_id))
}

/// Renders a [`FieldConfig`] to stable, pretty-printed JSON text.
fn render_field_config(config: &FieldConfig) -> Result<String, StoreError> {
    let mut text =
        serde_json::to_string_pretty(config).map_err(|error| StoreError::InvalidFieldConfig {
            path: PathBuf::from(field_config_filename(&config.id)),
            message: format!("could not serialize configuration: {error}"),
        })?;
    text.push('\n');
    Ok(text)
}

/// Writes one Field's configuration atomically.
///
/// Refuses a `config` map carrying a credential-shaped key name before
/// touching the filesystem at all.
pub fn write_field_config(notebook: &Notebook, config: &FieldConfig) -> Result<(), StoreError> {
    reject_credential_shaped_keys(&config.config)?;
    let directory = fields_dir(notebook);
    std::fs::create_dir_all(&directory).map_err(|error| {
        StoreError::io("create Field configuration directory", &directory, error)
    })?;
    let text = render_field_config(config)?;
    atomic::write_atomic(
        &directory,
        &field_config_filename(&config.id),
        text.as_bytes(),
    )?;
    Ok(())
}

/// Reads one Field's configuration.
///
/// `Ok(None)` means no Field is configured under that ID; a present but
/// malformed file fails with [`StoreError::InvalidFieldConfig`] rather than
/// silently defaulting to a disabled or empty Field.
pub fn read_field_config(
    notebook: &Notebook,
    field_id: &str,
) -> Result<Option<FieldConfig>, StoreError> {
    let path = field_config_path(notebook, field_id);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::io("read Field configuration", &path, error)),
    };
    let config = parse_field_config(&bytes)
        .map_err(|message| StoreError::InvalidFieldConfig { path, message })?;
    Ok(Some(config))
}

fn parse_field_config(bytes: &[u8]) -> Result<FieldConfig, String> {
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

/// Lists every configured external Field's configuration, sorted by ID.
///
/// The built-in `self` Field is never stored here and is not included; a
/// caller that wants to present `self` alongside configured Fields adds it
/// itself.
pub fn list_field_configs(notebook: &Notebook) -> Result<Vec<FieldConfig>, StoreError> {
    let directory = fields_dir(notebook);
    let mut configs = Vec::new();
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(configs),
        Err(error) => {
            return Err(StoreError::io(
                "read Field configuration directory",
                &directory,
                error,
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            StoreError::io("read Field configuration directory", &directory, error)
        })?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if atomic::is_staged_name(&name) || !name.ends_with(".json") {
            continue;
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| StoreError::io("read Field configuration", &path, error))?;
        let config = parse_field_config(&bytes)
            .map_err(|message| StoreError::InvalidFieldConfig { path, message })?;
        configs.push(config);
    }
    configs.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(configs)
}

/// Removes one Field's configuration file.
///
/// Returns whether a configuration existed to remove. This function touches
/// only the configuration file: it never removes Notes, artifacts, or any
/// other Field's configuration or operational state.
pub fn remove_field_config(notebook: &Notebook, field_id: &str) -> Result<bool, StoreError> {
    let path = field_config_path(notebook, field_id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(StoreError::io("remove Field configuration", &path, error)),
    }
}

fn sync_state_dir(notebook: &Notebook) -> PathBuf {
    let mut path = notebook.private_dir();
    for segment in SYNC_STATE_DIR {
        path = path.join(segment);
    }
    path
}

/// The reserved path for `field_id`'s durable, opaque sync cursor.
///
/// Nothing in this crate writes this file. A2 defines a cursor as an opaque,
/// non-secret, bounded UTF-8 string meaningful only to the Field's own
/// driver, paired with the manifest's declared `cursor_format_version`, so
/// this crate deliberately does not parse it: only a later `sync`
/// implementation, which knows the pairing rule, should read its bytes.
/// [`cursor_exists`] answers the one question `fields status` needs without
/// interpreting the content.
#[must_use]
pub fn cursor_state_path(notebook: &Notebook, field_id: &str) -> PathBuf {
    sync_state_dir(notebook).join(format!("{field_id}.cursor"))
}

/// Whether a durable cursor is currently recorded for `field_id`.
#[must_use]
pub fn cursor_exists(notebook: &Notebook, field_id: &str) -> bool {
    cursor_state_path(notebook, field_id).is_file()
}

/// The reserved path for `field_id`'s last recorded sync outcome.
///
/// Nothing in this crate writes this file; it is reserved so a later `sync`
/// implementation can start recording outcomes here without a schema change
/// to `fields status`.
#[must_use]
pub fn last_sync_path(notebook: &Notebook, field_id: &str) -> PathBuf {
    sync_state_dir(notebook).join(format!("{field_id}.status.json"))
}

/// A minimal, forward-compatible summary of a Field's last sync run.
///
/// Only `outcome` and `at` are interpreted here; every other member a future
/// `sync` implementation writes (counts, checkpoint state, and so on) is
/// preserved in `extra` and passed through uninterpreted, so this reader does
/// not need to change when that shape grows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastSyncOutcome {
    /// A short outcome label, such as `completed`, `partial`, or `failed`.
    /// The exact vocabulary belongs to the `sync` implementation that writes
    /// this file; this crate treats it as opaque display text.
    pub outcome: String,
    /// When the run that produced this outcome finished, as recorded by the
    /// writer. Not reinterpreted here.
    pub at: String,
    /// Every other recorded member, preserved without interpretation.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Reads `field_id`'s last recorded sync outcome, when one has been written.
///
/// `Ok(None)` means no run has been recorded yet, which is the normal state
/// for every Field before `sync` exists. A present but malformed file fails
/// loudly rather than silently reporting no history.
pub fn read_last_sync_outcome(
    notebook: &Notebook,
    field_id: &str,
) -> Result<Option<LastSyncOutcome>, StoreError> {
    let path = last_sync_path(notebook, field_id);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::io("read Field sync status", &path, error)),
    };
    let outcome: LastSyncOutcome =
        serde_json::from_slice(&bytes).map_err(|error| StoreError::InvalidFieldConfig {
            path,
            message: error.to_string(),
        })?;
    Ok(Some(outcome))
}

/// Removes `field_id`'s operational sync state (cursor and last-run summary),
/// if any is present.
///
/// This exists for `fields remove`: dropping a Field's configuration also
/// drops the operational state that only makes sense alongside it, while
/// leaving every Note and artifact the Field produced untouched.
pub fn remove_sync_state(notebook: &Notebook, field_id: &str) -> Result<(), StoreError> {
    for path in [
        cursor_state_path(notebook, field_id),
        last_sync_path(notebook, field_id),
    ] {
        remove_if_present(&path)?;
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::io("remove Field sync state", path, error)),
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
    fn a_missing_field_configuration_is_not_an_error() -> Result<(), StoreError> {
        let temp = temp("fields-missing")?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        assert_eq!(read_field_config(&notebook, "local_work")?, None);
        assert!(list_field_configs(&notebook)?.is_empty());
        Ok(())
    }

    #[test]
    fn write_then_read_round_trips_through_an_atomic_write() -> Result<(), StoreError> {
        let temp = temp("fields-roundtrip")?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        let mut config =
            FieldConfig::new("local_work", PathBuf::from("/usr/local/bin/local-field"));
        config
            .config
            .insert("path".to_owned(), "/Users/joe/reference".to_owned());
        write_field_config(&notebook, &config)?;
        assert_eq!(
            read_field_config(&notebook, "local_work")?,
            Some(config.clone())
        );

        // No staging litter survives a successful write.
        let dir = fields_dir(&notebook);
        for entry in std::fs::read_dir(&dir).map_err(|error| StoreError::io("read", &dir, error))? {
            let entry = entry.map_err(|error| StoreError::io("read", &dir, error))?;
            assert!(!atomic::is_staged_name(
                &entry.file_name().to_string_lossy()
            ));
        }

        // Writing again replaces the file rather than appending.
        let mut updated = config;
        updated.enabled = false;
        write_field_config(&notebook, &updated)?;
        assert_eq!(read_field_config(&notebook, "local_work")?, Some(updated));
        Ok(())
    }

    #[test]
    fn list_is_sorted_and_ignores_staging_litter() -> Result<(), StoreError> {
        let temp = temp("fields-list")?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        write_field_config(
            &notebook,
            &FieldConfig::new("teams_wxs", PathBuf::from("/bin/teams-field")),
        )?;
        write_field_config(
            &notebook,
            &FieldConfig::new("local_work", PathBuf::from("/bin/local-field")),
        )?;
        // A leftover staging file must never be mistaken for a configuration.
        atomic::StagedFile::create(&fields_dir(&notebook), b"partial")?;

        let listed: Vec<String> = list_field_configs(&notebook)?
            .into_iter()
            .map(|config| config.id)
            .collect();
        assert_eq!(
            listed,
            vec!["local_work".to_owned(), "teams_wxs".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn remove_deletes_only_the_named_field() -> Result<(), StoreError> {
        let temp = temp("fields-remove")?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        write_field_config(
            &notebook,
            &FieldConfig::new("local_work", PathBuf::from("/bin/local-field")),
        )?;
        write_field_config(
            &notebook,
            &FieldConfig::new("teams_wxs", PathBuf::from("/bin/teams-field")),
        )?;
        assert!(remove_field_config(&notebook, "local_work")?);
        assert!(!remove_field_config(&notebook, "local_work")?);
        assert_eq!(read_field_config(&notebook, "local_work")?, None);
        assert!(read_field_config(&notebook, "teams_wxs")?.is_some());
        Ok(())
    }

    #[test]
    fn a_malformed_configuration_fails_loudly() -> Result<(), StoreError> {
        let temp = temp("fields-malformed")?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        let path = field_config_path(&notebook, "local_work");
        std::fs::create_dir_all(fields_dir(&notebook))
            .map_err(|error| StoreError::io("create", &path, error))?;
        std::fs::write(&path, b"{ not json")
            .map_err(|error| StoreError::io("write", &path, error))?;
        match read_field_config(&notebook, "local_work") {
            Err(StoreError::InvalidFieldConfig { .. }) => {}
            other => panic!("expected InvalidFieldConfig, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn credential_shaped_keys_are_refused_by_name() -> Result<(), StoreError> {
        let temp = temp("fields-credential")?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        let mut config = FieldConfig::new("local_work", PathBuf::from("/bin/local-field"));
        for key in ["password", "API_KEY", "client-secret", "Cookie"] {
            let mut attempt = config.clone();
            attempt.config.insert(key.to_owned(), "x".to_owned());
            assert!(matches!(
                write_field_config(&notebook, &attempt),
                Err(StoreError::CredentialShapedConfigKey { .. })
            ));
        }
        // An ordinary non-secret key is unaffected.
        config
            .config
            .insert("path".to_owned(), "/tmp/reference".to_owned());
        write_field_config(&notebook, &config)?;
        Ok(())
    }

    #[test]
    fn cursor_and_sync_status_default_to_absent_and_round_trip_when_present()
    -> Result<(), StoreError> {
        let temp = temp("fields-sync-state")?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        assert!(!cursor_exists(&notebook, "local_work"));
        assert_eq!(read_last_sync_outcome(&notebook, "local_work")?, None);

        // A future `sync` implementation writes these files directly; this
        // module only needs to read them back correctly, including members
        // it does not itself define.
        let sync_dir = sync_state_dir(&notebook);
        std::fs::create_dir_all(&sync_dir)
            .map_err(|error| StoreError::io("create", &sync_dir, error))?;
        std::fs::write(
            cursor_state_path(&notebook, "local_work"),
            b"opaque-cursor-bytes",
        )
        .map_err(|error| StoreError::io("write", &sync_dir, error))?;
        std::fs::write(
            last_sync_path(&notebook, "local_work"),
            br#"{"outcome":"completed","at":"2026-08-23T09:00:00+00:00","collected":3}"#,
        )
        .map_err(|error| StoreError::io("write", &sync_dir, error))?;

        assert!(cursor_exists(&notebook, "local_work"));
        let outcome = read_last_sync_outcome(&notebook, "local_work")?
            .unwrap_or_else(|| panic!("expected a recorded sync outcome"));
        assert_eq!(outcome.outcome, "completed");
        assert_eq!(outcome.at, "2026-08-23T09:00:00+00:00");
        assert_eq!(
            outcome
                .extra
                .get("collected")
                .and_then(serde_json::Value::as_i64),
            Some(3)
        );

        remove_sync_state(&notebook, "local_work")?;
        assert!(!cursor_exists(&notebook, "local_work"));
        assert_eq!(read_last_sync_outcome(&notebook, "local_work")?, None);
        Ok(())
    }
}
