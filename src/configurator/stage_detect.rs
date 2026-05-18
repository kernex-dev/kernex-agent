//! Stage 1 DETECT — probe the target agent without writing any file.
//!
//! Behavior lands in §5 (E-detect-1..6). The scaffold here defines the
//! typed `Detection` output that RESOLVE consumes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{InstallError, InstallOptions};

/// Output of DETECT consumed by RESOLVE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    /// True iff the agent CLI (`claude`, `cursor`, etc.) is on `$PATH`.
    pub installed: bool,
    /// `$HOME/.claude/` for the Claude adapter; analogous for others.
    pub config_root: Option<PathBuf>,
    /// Parsed from `claude --version` when `installed`.
    pub version: Option<String>,
}

pub async fn run(_opts: &InstallOptions) -> Result<Detection, InstallError> {
    unimplemented!("stage_detect::run — lands in §5 of the SDD")
}
