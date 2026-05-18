//! Stage 4 BACKUP — snapshot every path APPLY will touch.
//!
//! Behavior lands in §8 (E-backup-1..6). The scaffold here defines the
//! `BackupReceipt` output that APPLY consumes and rollback restores from.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

/// Result of a successful BACKUP. APPLY threads this through and rollback
/// consults it on failure to restore overwritten files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupReceipt {
    /// Path to the gzipped tarball under `~/.kx/backups/`.
    pub tarball_path: PathBuf,
    /// Absolute paths contained in the tarball.
    pub files: Vec<PathBuf>,
    /// Size of the tarball on disk.
    pub bytes: u64,
}

pub async fn run(
    _opts: &InstallOptions,
    _plan: &InstallPlan,
) -> Result<BackupReceipt, InstallError> {
    unimplemented!("stage_backup::run — lands in §8 of the SDD")
}
