//! Stage 2 RESOLVE — combine user options + DETECT into a typed plan.
//!
//! Behavior lands in §6 (E-resolve-1..5). The scaffold here defines the
//! typed `InstallPlan` output that BACKUP and APPLY both consume.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::stage_detect::Detection;
use super::{InstallError, InstallOptions};

/// Output of RESOLVE consumed by BACKUP and APPLY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    /// Logical agent identifier (e.g. "claude-code"). Mapped to an
    /// `AdapterId` at registry lookup time.
    pub agent: String,
    /// Components from the resolved preset (e.g. `["claude-md", "mcp-json"]`).
    pub components: Vec<String>,
    /// Per-component absolute target paths under `$HOME`.
    pub target_paths: Vec<(String, PathBuf)>,
}

pub async fn run(
    _opts: &InstallOptions,
    _detection: &Detection,
) -> Result<InstallPlan, InstallError> {
    unimplemented!("stage_resolve::run — lands in §6 of the SDD")
}
