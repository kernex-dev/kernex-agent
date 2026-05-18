//! Stage 4 BACKUP — snapshot every path APPLY will touch.
//!
//! Behavior per E-backup-1..6.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::install::audit::{AuditEvent, AuditWriter, EventError, EventStatus, Stage};

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
    opts: &InstallOptions,
    plan: &InstallPlan,
    audit: &AuditWriter,
) -> Result<BackupReceipt, InstallError> {
    let started = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.backup.start".to_string(),
            stage: Stage::Backup,
            status: EventStatus::Success,
            started_at: started,
            ended_at: None,
            duration_ms: None,
            payload: json!({"agent": &plan.agent}),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    let result = build_backup(opts, plan, started);
    let ended = Utc::now();
    let duration_ms = (ended - started).num_milliseconds().max(0) as u64;

    match result {
        Ok(receipt) => {
            audit
                .emit(AuditEvent {
                    event: "stage.backup.end".to_string(),
                    stage: Stage::Backup,
                    status: EventStatus::Success,
                    started_at: started,
                    ended_at: Some(ended),
                    duration_ms: Some(duration_ms),
                    payload: json!({
                        "tarball_path": &receipt.tarball_path,
                        "files": &receipt.files,
                        "bytes": receipt.bytes,
                    }),
                    errors: vec![],
                })
                .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
            Ok(receipt)
        }
        Err(err) => {
            let message = err.to_string();
            audit
                .emit(AuditEvent {
                    event: "stage.backup.error".to_string(),
                    stage: Stage::Backup,
                    status: EventStatus::Failure,
                    started_at: started,
                    ended_at: Some(ended),
                    duration_ms: Some(duration_ms),
                    payload: serde_json::Value::Null,
                    errors: vec![EventError {
                        code: "backup_failed".to_string(),
                        message,
                        transient: false,
                    }],
                })
                .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
            Err(err)
        }
    }
}

fn build_backup(
    opts: &InstallOptions,
    plan: &InstallPlan,
    now: chrono::DateTime<Utc>,
) -> Result<BackupReceipt, InstallError> {
    let backups_dir = opts.home.join(".kx").join("backups");
    fs::create_dir_all(&backups_dir)?;

    let stamp = now.format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let tarball_path = backups_dir.join(format!("{stamp}-{}.tar.gz", plan.agent));

    // Sandbox probe (E-CC-2): if the kernex sandbox would block this
    // write, refuse the stage with a transient error so the orchestrator
    // can surface exit 7.
    let data_dir = opts.home.join(".kx");
    if kernex_sandbox::is_write_blocked(&tarball_path, &data_dir, None) {
        return Err(InstallError::SandboxRefused { path: tarball_path });
    }

    let file = File::create(&tarball_path)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);

    let mut included: Vec<PathBuf> = Vec::new();
    for (_, path) in &plan.target_paths {
        if path.is_file() {
            append_file(&mut builder, path)?;
            included.push(path.clone());
        }
    }

    // Finalize and close. tar::Builder::into_inner returns the GzEncoder;
    // GzEncoder::finish drops the gzip footer. Both must succeed before
    // the tarball is usable.
    let encoder = builder
        .into_inner()
        .map_err(|e| io_err_to_install(e, &tarball_path))?;
    encoder
        .finish()
        .map_err(|e| io_err_to_install(e, &tarball_path))?;

    let bytes = match fs::metadata(&tarball_path) {
        Ok(meta) => meta.len(),
        Err(_) => 0,
    };

    if bytes == 0 && included.is_empty() {
        // Empty backup (no targets exist). Unlink so the filesystem
        // doesn't accumulate zero-byte tarballs.
        let _ = fs::remove_file(&tarball_path);
    }

    Ok(BackupReceipt {
        tarball_path,
        files: included,
        bytes,
    })
}

fn append_file(
    builder: &mut tar::Builder<GzEncoder<File>>,
    path: &Path,
) -> Result<(), InstallError> {
    // Use the path as-is inside the tarball. Restore is `tar -xzf -C /`
    // so the absolute path round-trips. Strip the leading '/' for tar
    // compatibility per the tar crate's API requirements.
    let archive_path = path.strip_prefix("/").unwrap_or(path);
    builder
        .append_path_with_name(path, archive_path)
        .map_err(|e| io_err_to_install(e, path))?;
    Ok(())
}

fn io_err_to_install(err: std::io::Error, path: &Path) -> InstallError {
    InstallError::Permanent(format!("backup IO failed for '{}': {err}", path.display()))
}
