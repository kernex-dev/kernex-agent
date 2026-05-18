//! Stage 3 REVIEW — print plan, prompt unless `--yes`.
//!
//! Behavior lands in §7 (E-review-1..5).

use crate::install::audit::AuditWriter;

use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

pub async fn run(
    _opts: &InstallOptions,
    _plan: &InstallPlan,
    _audit: &AuditWriter,
) -> Result<(), InstallError> {
    unimplemented!("stage_review::run — lands in §7 of the SDD")
}
