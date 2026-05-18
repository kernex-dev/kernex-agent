//! Stage 5 APPLY — render templates and write files; auto-rollback on failure.
//!
//! Behavior lands in §9 (E-apply-1..8 plus rollback). The scaffold here
//! defines the `Receipt` shape that the rollback walk consults in reverse.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::stage_backup::BackupReceipt;
use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

/// Per-component write result. APPLY accumulates a `Vec<Receipt>` and emits
/// each as a `stage.apply.write` audit event. Rollback walks the vec in
/// reverse order: `Created` files are removed, `Overwrote` files are
/// extracted from the backup tarball.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub component: String,
    pub path: PathBuf,
    pub action: ReceiptAction,
    pub bytes_written: u64,
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptAction {
    Created,
    Overwrote,
    Skipped,
}

pub async fn run(
    _opts: &InstallOptions,
    _plan: &InstallPlan,
    _backup: &BackupReceipt,
) -> Result<Vec<Receipt>, InstallError> {
    unimplemented!("stage_apply::run — lands in §9 of the SDD")
}

pub async fn rollback(
    _backup: &BackupReceipt,
    _receipts: &[Receipt],
    _err: &InstallError,
) -> Result<(), InstallError> {
    unimplemented!("stage_apply::rollback — lands in §9 of the SDD")
}
