//! Unit tests for the install audit writer (§2).
//!
//! Covers E-audit-1..6 plus the secret redactor.

#![cfg(feature = "agent-claude")]

use chrono::{TimeZone, Utc};
use kernex_agent::install::audit::{redact_payload, AuditEvent, AuditWriter, EventStatus, Stage};
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn fixed_event() -> AuditEvent {
    AuditEvent {
        event: "stage.detect.start".to_string(),
        stage: Stage::Detect,
        status: EventStatus::Success,
        started_at: Utc.with_ymd_and_hms(2026, 5, 18, 10, 23, 45).unwrap(),
        ended_at: None,
        duration_ms: None,
        payload: json!({"agent": "claude-code"}),
        errors: vec![],
    }
}

#[test]
fn emits_one_line_per_event_with_trailing_newline() {
    let tmp = TempDir::new().unwrap();
    let writer = AuditWriter::new(tmp.path()).unwrap();
    writer.emit(fixed_event()).unwrap();
    writer.emit(fixed_event()).unwrap();
    let contents = fs::read_to_string(writer.path()).unwrap();
    let lines: Vec<&str> = contents.split_inclusive('\n').collect();
    assert_eq!(lines.len(), 2, "expected exactly two terminated lines");
    for line in &lines {
        assert!(line.ends_with('\n'));
        let body = line.trim_end_matches('\n');
        // Every line must round-trip through serde_json.
        let _: serde_json::Value = serde_json::from_str(body).unwrap();
    }
}

#[test]
fn flushes_after_each_event() {
    let tmp = TempDir::new().unwrap();
    let writer = AuditWriter::new(tmp.path()).unwrap();
    writer.emit(fixed_event()).unwrap();
    // The file is observable BEFORE the writer is dropped because every
    // emit() flushes the inner BufWriter (E-audit-4). Drop is not required.
    let contents = fs::read_to_string(writer.path()).unwrap();
    assert!(
        contents.contains("\"event\":\"stage.detect.start\""),
        "post-emit read found: {contents:?}"
    );
}

#[test]
fn creates_audit_dir_if_missing() {
    let tmp = TempDir::new().unwrap();
    // Nothing under tmp/.kx yet.
    assert!(!tmp.path().join(".kx").exists());
    let writer = AuditWriter::new(tmp.path()).unwrap();
    let audit_dir = tmp.path().join(".kx").join("audit");
    assert!(
        audit_dir.exists(),
        "audit dir should be created on writer init"
    );
    assert!(writer.path().starts_with(&audit_dir));
}

#[test]
fn collision_suffix_resolves_when_same_second_install() {
    let tmp = TempDir::new().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 18, 10, 23, 45).unwrap();
    let a = AuditWriter::new_with_now(tmp.path(), now).unwrap();
    let b = AuditWriter::new_with_now(tmp.path(), now).unwrap();
    let c = AuditWriter::new_with_now(tmp.path(), now).unwrap();
    // All three opened in the same second; filenames must be distinct.
    assert_ne!(a.path(), b.path());
    assert_ne!(b.path(), c.path());
    assert_ne!(a.path(), c.path());
    // Primary uses no suffix; siblings carry -1, -2.
    let a_name = a.path().file_name().unwrap().to_str().unwrap();
    let b_name = b.path().file_name().unwrap().to_str().unwrap();
    let c_name = c.path().file_name().unwrap().to_str().unwrap();
    assert_eq!(a_name, "install-2026-05-18T10-23-45Z.jsonl");
    assert_eq!(b_name, "install-2026-05-18T10-23-45Z-1.jsonl");
    assert_eq!(c_name, "install-2026-05-18T10-23-45Z-2.jsonl");
}

#[test]
fn redacts_secret_keys_in_payload() {
    let payload = json!({
        "agent": "claude-code",
        "api_key": "sk-real-secret",
        "nested": {
            "TOKEN": "deadbeef",
            "password": "hunter2",
            "ok_field": "visible"
        },
        "list": [
            {"secret_value": "hidden"},
            {"safe": "shown"}
        ]
    });
    let out = redact_payload(payload);
    assert_eq!(out["agent"], json!("claude-code"));
    assert_eq!(out["api_key"], json!("<redacted>"));
    assert_eq!(out["nested"]["TOKEN"], json!("<redacted>"));
    assert_eq!(out["nested"]["password"], json!("<redacted>"));
    assert_eq!(out["nested"]["ok_field"], json!("visible"));
    assert_eq!(out["list"][0]["secret_value"], json!("<redacted>"));
    assert_eq!(out["list"][1]["safe"], json!("shown"));
}

#[test]
fn redacts_auth_header_style_keys() {
    // The original substring list missed common auth-header naming; these
    // must all redact.
    let payload = json!({
        "Authorization": "Bearer abc123",
        "bearer_value": "abc123",
        "aws_credentials": "AKIA...",
        "x-api-key": "sk-123",
        "apiKey": "sk-456",
        "plain": "visible"
    });
    let out = redact_payload(payload);
    assert_eq!(out["Authorization"], json!("<redacted>"));
    assert_eq!(out["bearer_value"], json!("<redacted>"));
    assert_eq!(out["aws_credentials"], json!("<redacted>"));
    assert_eq!(out["x-api-key"], json!("<redacted>"));
    assert_eq!(out["apiKey"], json!("<redacted>"));
    assert_eq!(out["plain"], json!("visible"));
}
