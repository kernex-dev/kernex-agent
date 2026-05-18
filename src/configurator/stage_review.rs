//! Stage 3 REVIEW — print plan, prompt unless `--yes`.
//!
//! Behavior lands in §7 (E-review-1..5).

use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

pub async fn run(_opts: &InstallOptions, _plan: &InstallPlan) -> Result<(), InstallError> {
    unimplemented!("stage_review::run — lands in §7 of the SDD")
}
