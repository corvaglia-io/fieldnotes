//! `init`: create a notebook and its instance identity.

use std::path::{Path, PathBuf};

use fieldnotes_domain::{Clock, RandomSource, RecordKind};
use fieldnotes_format::instance::InstanceMetadata;
use fieldnotes_store::{InitState, Notebook, read_instance, write_instance};

use crate::error::AppError;
use crate::kernel::Kernel;

/// What `init` did.
#[derive(Debug, Clone, PartialEq)]
pub struct InitOutcome {
    /// The notebook root.
    pub root: PathBuf,
    /// The notebook's instance identity.
    pub instance: InstanceMetadata,
    /// Whether this call created the notebook or found an existing one.
    pub state: InitState,
}

/// Creates a notebook at `root`, or adopts an already-initialized one.
///
/// Re-running `init` on a valid notebook is idempotent and never rewrites the
/// instance identity, because that identity is immutable.
pub fn init<C: Clock, R: RandomSource>(
    kernel: &mut Kernel<C, R>,
    root: &Path,
    name: Option<&str>,
) -> Result<InitOutcome, AppError> {
    let (notebook, state) = Notebook::create(root)?;
    let instance = match state {
        InitState::AlreadyInitialized => read_instance(&notebook)?,
        InitState::Created => {
            let (instance_id, created_at) = kernel.new_record(RecordKind::Instance)?;
            let metadata = InstanceMetadata {
                instance_id,
                created_at,
                name: name.map(str::to_owned),
            };
            write_instance(&notebook, &metadata)?;
            metadata
        }
    };
    Ok(InitOutcome {
        root: notebook.root().to_path_buf(),
        instance,
        state,
    })
}
