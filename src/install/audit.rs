//! JSONL append-only audit log writer per ADR-003 / E-audit-1..6.
//!
//! Each install opens a fresh `~/.kx/audit/install-<UTC-ISO8601>.jsonl`.
//! Every stage emits at least one event through `AuditWriter::emit`; one
//! JSON object per line, flushed after every write so a process crash
//! leaves a consistent prefix. Append-only: rollback writes its own events
//! without modifying prior entries.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

const REDACTED: &str = "<redacted>";
const SECRET_KEY_SUBSTRINGS: [&str; 9] = [
    "token",
    "secret",
    "password",
    "api_key",
    "api-key",
    "apikey",
    "authorization",
    "bearer",
    "credential",
];
const COLLISION_LIMIT: u32 = 1000;

/// Audit writer error surface.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("failed to create audit directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "could not pick a non-colliding filename under '{dir}' after {COLLISION_LIMIT} attempts"
    )]
    CollisionExhausted { dir: PathBuf },
    #[error("failed to open audit file '{path}': {source}")]
    OpenFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize audit event: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to write audit event: {0}")]
    Write(#[source] std::io::Error),
}

/// One canonical event in the install audit log per ADR-003.
///
/// Free-form `event` field (e.g. `"stage.detect.start"`, `"install.summary"`)
/// pairs with a typed `stage` enum so consumers can filter without parsing
/// the event name. `payload` is `serde_json::Value` so each stage can
/// attach its own typed output; the writer redacts secret keys before
/// serializing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event: String,
    pub stage: Stage,
    pub status: EventStatus,
    #[serde(serialize_with = "serialize_rfc3339_millis")]
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[serde(serialize_with = "serialize_rfc3339_millis_opt")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<EventError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Detect,
    Resolve,
    Review,
    Backup,
    Apply,
    Verify,
    Report,
    Rollback,
    Install,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Success,
    Failure,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventError {
    pub code: String,
    pub message: String,
    pub transient: bool,
}

/// The append-only JSONL writer.
///
/// One per install run. The `Mutex<BufWriter<File>>` lets multiple stages
/// share the writer through an `Arc` while keeping append ordering. Each
/// `emit` call serializes, writes the line, flushes.
pub struct AuditWriter {
    path: PathBuf,
    file: Mutex<BufWriter<File>>,
}

impl AuditWriter {
    /// Open a fresh audit log under `<home>/.kx/audit/`.
    ///
    /// The filename is `install-<UTC-ISO8601-seconds>.jsonl`. If a file at
    /// that path already exists (two installs starting in the same second),
    /// the writer falls back to `install-<ts>-1.jsonl`, `-2.jsonl`, etc.,
    /// up to 1000 before erroring out (E-audit-2).
    pub fn new(home: &Path) -> Result<Self, AuditError> {
        Self::new_with_now(home, Utc::now())
    }

    /// Same as `new` but with an injected timestamp for tests.
    pub fn new_with_now(home: &Path, now: DateTime<Utc>) -> Result<Self, AuditError> {
        let dir = home.join(".kx").join("audit");
        std::fs::create_dir_all(&dir).map_err(|source| AuditError::CreateDir {
            path: dir.clone(),
            source,
        })?;

        let stamp = now.format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let path = next_non_colliding_path(&dir, &stamp)?;
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|source| AuditError::OpenFile {
                path: path.clone(),
                source,
            })?;

        Ok(Self {
            path,
            file: Mutex::new(BufWriter::new(file)),
        })
    }

    /// Absolute path of the audit log this writer owns.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serialize, append, flush. Redacts secret keys in `payload` first.
    pub fn emit(&self, mut event: AuditEvent) -> Result<(), AuditError> {
        event.payload = redact_payload(event.payload);
        let line = serde_json::to_string(&event)?;
        let mut guard = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .write_all(line.as_bytes())
            .map_err(AuditError::Write)?;
        guard.write_all(b"\n").map_err(AuditError::Write)?;
        guard.flush().map_err(AuditError::Write)?;
        Ok(())
    }
}

fn next_non_colliding_path(dir: &Path, stamp: &str) -> Result<PathBuf, AuditError> {
    let primary = dir.join(format!("install-{stamp}.jsonl"));
    if !primary.exists() {
        return Ok(primary);
    }
    for suffix in 1..=COLLISION_LIMIT {
        let candidate = dir.join(format!("install-{stamp}-{suffix}.jsonl"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AuditError::CollisionExhausted {
        dir: dir.to_path_buf(),
    })
}

/// Replace values whose key matches a secret pattern (case-insensitive)
/// with the sentinel `"<redacted>"`. Walks nested maps and arrays.
pub fn redact_payload(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, inner) in map {
                let lower = key.to_ascii_lowercase();
                if SECRET_KEY_SUBSTRINGS.iter().any(|pat| lower.contains(pat)) {
                    out.insert(key, serde_json::Value::String(REDACTED.to_string()));
                } else {
                    out.insert(key, redact_payload(inner));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(redact_payload).collect())
        }
        other => other,
    }
}

fn serialize_rfc3339_millis<S>(ts: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_str(&ts.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn serialize_rfc3339_millis_opt<S>(ts: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match ts {
        Some(ts) => s.serialize_str(&ts.to_rfc3339_opts(SecondsFormat::Millis, true)),
        None => s.serialize_none(),
    }
}
