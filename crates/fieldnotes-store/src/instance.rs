//! Reading and writing `.fieldnotes/instance.yaml`.
//!
//! Serialization and validation belong to the format crate; this module only
//! makes the approved bytes durable and reads them back.

use fieldnotes_format::instance::{InstanceMetadata, instance_yaml_string, parse_instance_yaml};

use crate::atomic;
use crate::error::StoreError;
use crate::layout::{INSTANCE_FILE, Notebook};

/// Writes instance metadata atomically.
///
/// The bytes are produced by the format crate's canonical serializer and
/// re-parsed before they are installed, so a malformed instance file cannot be
/// created even by a caller holding a hand-built [`InstanceMetadata`].
pub fn write_instance(notebook: &Notebook, metadata: &InstanceMetadata) -> Result<(), StoreError> {
    let text = instance_yaml_string(metadata);
    let path = notebook.instance_path();
    parse_instance_yaml(text.as_bytes()).map_err(|source| StoreError::Invalid {
        path: path.clone(),
        source,
    })?;
    atomic::write_atomic(&notebook.private_dir(), INSTANCE_FILE, text.as_bytes())?;
    Ok(())
}

/// Reads and validates instance metadata.
pub fn read_instance(notebook: &Notebook) -> Result<InstanceMetadata, StoreError> {
    let path = notebook.instance_path();
    let bytes = std::fs::read(&path)
        .map_err(|error| StoreError::io("read instance metadata", &path, error))?;
    parse_instance_yaml(&bytes).map_err(|source| StoreError::Invalid { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_domain::{Datetime, RecordId};
    use fieldnotes_test_support::TempDir;

    #[test]
    fn round_trips_instance_metadata() -> Result<(), StoreError> {
        let temp = TempDir::new("instance")
            .map_err(|error| StoreError::io("create temporary directory", ".", error))?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        let instance_id =
            RecordId::parse("fn_01a02837-2de0-7a2b-8c41-f2481851192a").map_err(|_| {
                StoreError::NotANotebook {
                    start: temp.path().to_path_buf(),
                }
            })?;
        let created_at =
            Datetime::parse("2026-08-22T08:45:00+02:00").map_err(|_| StoreError::NotANotebook {
                start: temp.path().to_path_buf(),
            })?;
        let metadata = InstanceMetadata {
            instance_id,
            created_at,
            name: Some("workstation".to_owned()),
        };
        write_instance(&notebook, &metadata)?;
        assert_eq!(read_instance(&notebook)?, metadata);
        Ok(())
    }
}
