//! Stage 1 DETECT — probe the target agent without writing any file.
//!
//! Behavior per E-detect-1..6.

use chrono::Utc;
use kernex_adapter_core::Detection;
use serde_json::json;

use crate::adapters::default_registry;
use crate::install::audit::{AuditEvent, AuditWriter, EventError, EventStatus, Stage};

use super::{InstallError, InstallOptions};

pub async fn run(opts: &InstallOptions, audit: &AuditWriter) -> Result<Detection, InstallError> {
    let started = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.detect.start".to_string(),
            stage: Stage::Detect,
            status: EventStatus::Success,
            started_at: started,
            ended_at: None,
            duration_ms: None,
            payload: json!({"agent": &opts.agent}),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    let adapter = default_registry()
        .lookup(&opts.agent)
        .ok_or_else(|| InstallError::UnknownAgent(opts.agent.clone()))?;

    let result = adapter.detect().await;
    let ended = Utc::now();
    let duration_ms = (ended - started).num_milliseconds().max(0) as u64;

    match result {
        Ok(detection) => {
            audit
                .emit(AuditEvent {
                    event: "stage.detect.end".to_string(),
                    stage: Stage::Detect,
                    status: EventStatus::Success,
                    started_at: started,
                    ended_at: Some(ended),
                    duration_ms: Some(duration_ms),
                    payload: serde_json::to_value(&detection).unwrap_or(serde_json::Value::Null),
                    errors: vec![],
                })
                .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
            Ok(detection)
        }
        Err(err) => {
            let message = err.to_string();
            audit
                .emit(AuditEvent {
                    event: "stage.detect.error".to_string(),
                    stage: Stage::Detect,
                    status: EventStatus::Failure,
                    started_at: started,
                    ended_at: Some(ended),
                    duration_ms: Some(duration_ms),
                    payload: serde_json::Value::Null,
                    errors: vec![EventError {
                        code: "adapter_detect_failed".to_string(),
                        message: message.clone(),
                        transient: false,
                    }],
                })
                .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
            Err(InstallError::Permanent(format!("detect failed: {message}")))
        }
    }
}
