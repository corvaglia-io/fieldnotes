//! Field configuration and status: `fields add/list/status/remove`.
//!
//! `sync`, `describe` invocation, cursor writing, and record ingestion are a
//! later phase's work. What lives here is the durable configuration a Field
//! needs before any of that runs, plus enough state provisioning that the
//! later phase can record cursors, manifest snapshots, and sync outcomes
//! without a schema change to what is built here.
//!
//! Removing a Field's configuration never touches Notes or artifacts: they
//! are the notebook's canonical evidence (`docs/roadmap.md`'s invariants) and
//! remain, attributable to their original producer, until an explicit prune
//! (a later release) removes them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fieldnotes_domain::{FieldId, FieldStemRegistry};
use fieldnotes_field_protocol::declared::ManifestSnapshot;
use fieldnotes_field_protocol::message::{Manifest, Validate as ManifestValidate};
use fieldnotes_store::{
    FieldConfig, LastSyncOutcome, Notebook, cursor_exists, list_field_configs, read_field_config,
    read_last_sync_outcome, remove_field_config, remove_sync_state, write_field_config,
};

use crate::error::AppError;
use crate::kernel::SELF_FIELD;

/// Validates a `<type> <label>` pair against the registered Field stem set
/// and constructs the resulting Field ID.
///
/// This never guesses a stem/label split from an already-combined ID string:
/// callers only ever hold the two parts a user typed separately, and
/// [`FieldId::parse`] is the single place that checks `type` against
/// [`FieldStemRegistry::v1`]. `self` is not a registered external stem, so
/// naming it as `type` is rejected the same way an unregistered stem is.
pub fn validate_field_id(field_type: &str, label: &str) -> Result<FieldId, AppError> {
    let candidate = format!("{field_type}_{label}");
    FieldId::parse(&candidate, FieldStemRegistry::v1())
        .map_err(|source| AppError::InvalidFieldId { candidate, source })
}

/// Configures a new external Field.
///
/// Fails if `field_id` is the built-in `self` Field (never addable this way)
/// or if a configuration already exists under that ID (fields.md: a Field ID
/// is immutable once configured, so reconfiguring one in place is refused
/// rather than silently rewriting it — remove it first).
pub fn add_field(
    notebook: &Notebook,
    field_id: &FieldId,
    executable: PathBuf,
    config: BTreeMap<String, String>,
    enabled: bool,
) -> Result<FieldConfig, AppError> {
    if field_id.as_str() == SELF_FIELD {
        return Err(AppError::CannotConfigureSelf);
    }
    if read_field_config(notebook, field_id.as_str())?.is_some() {
        return Err(AppError::FieldAlreadyConfigured {
            id: field_id.as_str().to_owned(),
        });
    }
    let mut record = FieldConfig::new(field_id.as_str(), executable);
    record.enabled = enabled;
    record.config = config;
    write_field_config(notebook, &record)?;
    Ok(record)
}

/// One Field, built-in or configured, as summarized for `fields list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSummary {
    /// The Field ID.
    pub id: String,
    /// Whether this is the built-in `self` Field rather than a configured
    /// external one.
    pub built_in: bool,
    /// Whether the Field currently participates in sync.
    pub enabled: bool,
    /// The configured executable, absent for the built-in `self` Field.
    pub executable: Option<PathBuf>,
}

/// Lists every Field: the built-in `self` Field first, then every configured
/// external Field in ascending ID order.
pub fn list_fields(notebook: &Notebook) -> Result<Vec<FieldSummary>, AppError> {
    let mut summaries = vec![self_summary()];
    for config in list_field_configs(notebook)? {
        summaries.push(FieldSummary {
            id: config.id,
            built_in: false,
            enabled: config.enabled,
            executable: Some(config.executable),
        });
    }
    Ok(summaries)
}

fn self_summary() -> FieldSummary {
    FieldSummary {
        id: SELF_FIELD.to_owned(),
        built_in: true,
        enabled: true,
        executable: None,
    }
}

/// Per-Field state useful to check before and after a sync.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldStatusReport {
    /// The Field ID.
    pub id: String,
    /// Whether this is the built-in `self` Field.
    pub built_in: bool,
    /// Whether the Field currently participates in sync.
    pub enabled: bool,
    /// Whether a durable operational cursor is currently recorded.
    ///
    /// Always `false` for `self`, which never runs the external process
    /// protocol and never has a cursor.
    pub cursor_present: bool,
    /// Whether a `describe` manifest snapshot is currently recorded.
    ///
    /// This phase does not run a Field, so it cannot compare a stored
    /// snapshot against a freshly reported one; [`check_manifest_agreement`]
    /// is the function a later `sync` phase calls with a live manifest to do
    /// that. This report only says whether a snapshot exists yet.
    pub manifest_present: bool,
    /// The last recorded sync outcome, when a `sync` implementation has
    /// written one.
    pub last_sync: Option<LastSyncOutcome>,
}

/// Reports status for one named Field, or every Field when `field_id` is
/// `None`.
pub fn field_status(
    notebook: &Notebook,
    field_id: Option<&str>,
) -> Result<Vec<FieldStatusReport>, AppError> {
    match field_id {
        Some(id) => Ok(vec![one_field_status(notebook, id)?]),
        None => {
            let mut reports = vec![one_field_status(notebook, SELF_FIELD)?];
            for config in list_field_configs(notebook)? {
                reports.push(status_from_config(notebook, config)?);
            }
            Ok(reports)
        }
    }
}

fn one_field_status(notebook: &Notebook, id: &str) -> Result<FieldStatusReport, AppError> {
    if id == SELF_FIELD {
        return Ok(FieldStatusReport {
            id: id.to_owned(),
            built_in: true,
            enabled: true,
            cursor_present: false,
            manifest_present: false,
            last_sync: None,
        });
    }
    let config = read_field_config(notebook, id)?
        .ok_or_else(|| AppError::FieldNotConfigured { id: id.to_owned() })?;
    status_from_config(notebook, config)
}

fn status_from_config(
    notebook: &Notebook,
    config: FieldConfig,
) -> Result<FieldStatusReport, AppError> {
    let last_sync = read_last_sync_outcome(notebook, &config.id)?;
    Ok(FieldStatusReport {
        cursor_present: cursor_exists(notebook, &config.id),
        manifest_present: config.manifest.is_some(),
        id: config.id,
        built_in: false,
        enabled: config.enabled,
        last_sync,
    })
}

/// Removes one external Field's configuration and operational sync state.
///
/// This never deletes Notes or artifacts: they remain in the notebook,
/// attributable to their original producer, and every other Field's
/// configuration and Notes are untouched. `self` cannot be removed.
pub fn remove_field(notebook: &Notebook, id: &str) -> Result<(), AppError> {
    if id == SELF_FIELD {
        return Err(AppError::CannotConfigureSelf);
    }
    if !remove_field_config(notebook, id)? {
        return Err(AppError::FieldNotConfigured { id: id.to_owned() });
    }
    remove_sync_state(notebook, id)?;
    Ok(())
}

/// Whether recording a manifest changed the stored snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestOutcome {
    /// No manifest was stored yet; this one became the snapshot.
    FirstSnapshot,
    /// A manifest was already stored and this one matches it exactly.
    Unchanged,
    /// A manifest was already stored; this one agrees with it (for example,
    /// it adds a new declared property) and replaces it as the snapshot.
    Updated,
}

/// Decodes and validates `manifest_json` against the A2 schema, without
/// storing it or comparing it against anything.
fn decode_manifest(manifest_json: &serde_json::Value) -> Result<Manifest, AppError> {
    let manifest: Manifest = serde_json::from_value(manifest_json.clone()).map_err(|error| {
        AppError::InvalidManifest {
            message: error.to_string(),
        }
    })?;
    manifest
        .validate()
        .map_err(|error| AppError::InvalidManifest {
            message: error.to_string(),
        })?;
    Ok(manifest)
}

/// Checks whether `candidate` agrees with `stored`'s snapshot, per A2
/// section 4: adding a declared property is allowed; changing or removing
/// one, or changing `cursor_format_version`, requires a migration.
///
/// This is the pure check a later `sync` phase runs after every `describe`,
/// before deciding whether to proceed or to refuse with a migration message.
pub fn check_manifest_agreement(
    stored: &serde_json::Value,
    candidate: &serde_json::Value,
) -> Result<(), AppError> {
    let stored = decode_manifest(stored)?;
    let candidate = decode_manifest(candidate)?;
    ManifestSnapshot::of(&stored)
        .check_against(&ManifestSnapshot::of(&candidate))
        .map_err(|error| AppError::ManifestMigrationRequired {
            detail: error.to_string(),
        })
}

/// Records a freshly reported manifest as `id`'s snapshot.
///
/// A later `sync` phase calls this after every `describe` run. `0.1.1` does
/// not run Fields itself, so nothing here spawns a process; this validates
/// and persists whatever manifest value it is given, which lets both this
/// crate's own tests and the future `sync` implementation exercise the same
/// migration enforcement without a schema change.
///
/// When a snapshot is already stored, the candidate must agree with it (see
/// [`check_manifest_agreement`]) or the call is refused with
/// [`AppError::ManifestMigrationRequired`] and the stored snapshot is left
/// exactly as it was.
pub fn record_manifest(
    notebook: &Notebook,
    id: &str,
    manifest_json: serde_json::Value,
) -> Result<ManifestOutcome, AppError> {
    let candidate = decode_manifest(&manifest_json)?;
    let mut config = read_field_config(notebook, id)?
        .ok_or_else(|| AppError::FieldNotConfigured { id: id.to_owned() })?;

    let outcome = match &config.manifest {
        None => ManifestOutcome::FirstSnapshot,
        Some(stored_json) => {
            let stored = decode_manifest(stored_json)?;
            let stored_snapshot = ManifestSnapshot::of(&stored);
            let candidate_snapshot = ManifestSnapshot::of(&candidate);
            stored_snapshot
                .check_against(&candidate_snapshot)
                .map_err(|error| AppError::ManifestMigrationRequired {
                    detail: error.to_string(),
                })?;
            if stored_snapshot == candidate_snapshot {
                ManifestOutcome::Unchanged
            } else {
                ManifestOutcome::Updated
            }
        }
    };

    config.manifest = Some(manifest_json);
    write_field_config(notebook, &config)?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_test_support::TempDir;
    use serde_json::json;

    fn notebook(label: &str) -> (TempDir, Notebook) {
        let temp = TempDir::new(label).unwrap_or_else(|error| {
            panic!("could not create a temporary directory: {error}");
        });
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))
            .unwrap_or_else(|error| panic!("could not create a notebook: {error}"));
        (temp, notebook)
    }

    fn sample_manifest(
        cursor_format_version: u16,
        declared_properties: serde_json::Value,
    ) -> serde_json::Value {
        json!({
            "v": 1, "type": "manifest", "run_id": "1a4c9f2e-0000-4000-8000-000000000001",
            "protocol_version": 1, "protocol_revision": 0, "supported_protocol_versions": [1],
            "driver": "local-reference", "driver_version": "0.1.1", "field_stem": "local",
            "declared_properties": declared_properties,
            "capabilities": [{"object_kind": "file", "note_type": "file", "emits_artifacts": true,
                "emits_identity_anchors": false, "description": "d"}],
            "source_key": {"scope_rule": "local_root_id", "scope_rule_version": 1,
                "scope_shape": "s", "scope_depends_on_field_label": false,
                "identity_shape": "i", "identity_includes_object_kind": true,
                "source_version_ordering": "unsupported", "stable_across_instances": true},
            "auth": {"kind": "none", "credential_profile_required": false,
                "protected_channel_required": false, "refresh_owner": "not_applicable",
                "writes_to_source": false},
            "collection": {"incremental": true, "cursor_format_version": cursor_format_version,
                "supported_modes": ["incremental"], "window_supported": false,
                "refetch": "unsupported",
                "deletion": {"tombstones": "unsupported", "snapshot": "unsupported"}}
        })
    }

    #[test]
    fn validate_field_id_rejects_self_and_unregistered_stems() {
        assert!(validate_field_id("local", "work").is_ok());
        assert!(matches!(
            validate_field_id("self", "work"),
            Err(AppError::InvalidFieldId { .. })
        ));
        assert!(matches!(
            validate_field_id("github", "joe"),
            Err(AppError::InvalidFieldId { .. })
        ));
    }

    #[test]
    fn add_list_status_remove_round_trip() -> Result<(), AppError> {
        let (_temp, notebook) = notebook("fields-app-roundtrip");
        let field_id = validate_field_id("local", "work")?;
        let mut config = BTreeMap::new();
        config.insert("path".to_owned(), "/tmp/reference".to_owned());
        let added = add_field(
            &notebook,
            &field_id,
            PathBuf::from("/usr/local/bin/fieldnotes-field-local"),
            config,
            true,
        )?;
        assert_eq!(added.id, "local_work");

        let listed = list_fields(&notebook)?;
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "self");
        assert!(listed[0].built_in);
        assert_eq!(listed[1].id, "local_work");
        assert!(!listed[1].built_in);

        let statuses = field_status(&notebook, None)?;
        assert_eq!(statuses.len(), 2);
        let local_status = &statuses[1];
        assert!(local_status.enabled);
        assert!(!local_status.cursor_present);
        assert!(!local_status.manifest_present);
        assert_eq!(local_status.last_sync, None);

        remove_field(&notebook, "local_work")?;
        assert_eq!(list_fields(&notebook)?.len(), 1);
        assert!(matches!(
            field_status(&notebook, Some("local_work")),
            Err(AppError::FieldNotConfigured { .. })
        ));
        Ok(())
    }

    #[test]
    fn adding_self_or_a_duplicate_id_is_refused() -> Result<(), AppError> {
        let (_temp, notebook) = notebook("fields-app-refused");
        assert!(matches!(
            add_field(
                &notebook,
                &FieldId::parse("self", FieldStemRegistry::v1())
                    .unwrap_or_else(|error| panic!("`self` must parse: {error}")),
                PathBuf::from("/bin/x"),
                BTreeMap::new(),
                true,
            ),
            Err(AppError::CannotConfigureSelf)
        ));

        let field_id = validate_field_id("local", "work")?;
        add_field(
            &notebook,
            &field_id,
            PathBuf::from("/bin/local-field"),
            BTreeMap::new(),
            true,
        )?;
        assert!(matches!(
            add_field(
                &notebook,
                &field_id,
                PathBuf::from("/bin/local-field-2"),
                BTreeMap::new(),
                true,
            ),
            Err(AppError::FieldAlreadyConfigured { .. })
        ));
        Ok(())
    }

    #[test]
    fn removing_self_is_refused_and_removing_an_unknown_field_is_reported() {
        let (_temp, notebook) = notebook("fields-app-remove-guard");
        assert!(matches!(
            remove_field(&notebook, "self"),
            Err(AppError::CannotConfigureSelf)
        ));
        assert!(matches!(
            remove_field(&notebook, "local_ghost"),
            Err(AppError::FieldNotConfigured { .. })
        ));
    }

    #[test]
    fn remove_preserves_other_fields_configuration() -> Result<(), AppError> {
        let (_temp, notebook) = notebook("fields-app-remove-others");
        add_field(
            &notebook,
            &validate_field_id("local", "work")?,
            PathBuf::from("/bin/local-field"),
            BTreeMap::new(),
            true,
        )?;
        add_field(
            &notebook,
            &validate_field_id("teams", "wxs")?,
            PathBuf::from("/bin/teams-field"),
            BTreeMap::new(),
            true,
        )?;
        remove_field(&notebook, "local_work")?;
        let remaining: Vec<String> = list_fields(&notebook)?
            .into_iter()
            .map(|summary| summary.id)
            .collect();
        assert_eq!(remaining, vec!["self".to_owned(), "teams_wxs".to_owned()]);
        Ok(())
    }

    #[test]
    fn a_first_manifest_snapshot_is_recorded() -> Result<(), AppError> {
        let (_temp, notebook) = notebook("fields-app-manifest-first");
        let field_id = validate_field_id("local", "work")?;
        add_field(
            &notebook,
            &field_id,
            PathBuf::from("/bin/local-field"),
            BTreeMap::new(),
            true,
        )?;
        let manifest = sample_manifest(1, json!([]));
        let outcome = record_manifest(&notebook, "local_work", manifest)?;
        assert_eq!(outcome, ManifestOutcome::FirstSnapshot);
        let status = field_status(&notebook, Some("local_work"))?;
        assert!(status[0].manifest_present);
        Ok(())
    }

    #[test]
    fn an_added_declared_property_agrees_and_updates() -> Result<(), AppError> {
        let (_temp, notebook) = notebook("fields-app-manifest-agree");
        let field_id = validate_field_id("local", "work")?;
        add_field(
            &notebook,
            &field_id,
            PathBuf::from("/bin/local-field"),
            BTreeMap::new(),
            true,
        )?;
        record_manifest(&notebook, "local_work", sample_manifest(1, json!([])))?;

        let widened = sample_manifest(
            1,
            json!([{"name": "local_media_type", "value_type": "text", "cardinality": "scalar",
                "description": "media type"}]),
        );
        let outcome = record_manifest(&notebook, "local_work", widened.clone())?;
        assert_eq!(outcome, ManifestOutcome::Updated);

        // Recording the exact same manifest again is a no-op agreement.
        let outcome = record_manifest(&notebook, "local_work", widened)?;
        assert_eq!(outcome, ManifestOutcome::Unchanged);
        Ok(())
    }

    #[test]
    fn a_changed_cursor_format_version_requires_migration_and_keeps_the_old_snapshot()
    -> Result<(), AppError> {
        let (_temp, notebook) = notebook("fields-app-manifest-cursor");
        let field_id = validate_field_id("local", "work")?;
        add_field(
            &notebook,
            &field_id,
            PathBuf::from("/bin/local-field"),
            BTreeMap::new(),
            true,
        )?;
        let original = sample_manifest(1, json!([]));
        record_manifest(&notebook, "local_work", original.clone())?;

        let changed = sample_manifest(2, json!([]));
        assert!(matches!(
            record_manifest(&notebook, "local_work", changed.clone()),
            Err(AppError::ManifestMigrationRequired { .. })
        ));
        assert!(matches!(
            check_manifest_agreement(&original, &changed),
            Err(AppError::ManifestMigrationRequired { .. })
        ));

        // The rejected candidate must not have replaced the stored snapshot.
        let config = read_field_config(&notebook, "local_work")?
            .unwrap_or_else(|| panic!("Field must still be configured"));
        assert_eq!(config.manifest, Some(original));
        Ok(())
    }

    #[test]
    fn a_changed_declared_property_type_requires_migration() -> Result<(), AppError> {
        let (_temp, notebook) = notebook("fields-app-manifest-type-change");
        let field_id = validate_field_id("local", "work")?;
        add_field(
            &notebook,
            &field_id,
            PathBuf::from("/bin/local-field"),
            BTreeMap::new(),
            true,
        )?;
        let text_typed = sample_manifest(
            1,
            json!([{"name": "local_size", "value_type": "text", "cardinality": "scalar",
                "description": "size"}]),
        );
        record_manifest(&notebook, "local_work", text_typed)?;

        let number_typed = sample_manifest(
            1,
            json!([{"name": "local_size", "value_type": "number", "cardinality": "scalar",
                "description": "size"}]),
        );
        assert!(matches!(
            record_manifest(&notebook, "local_work", number_typed),
            Err(AppError::ManifestMigrationRequired { .. })
        ));
        Ok(())
    }
}
