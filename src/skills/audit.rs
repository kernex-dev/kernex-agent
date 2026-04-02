use std::io::Write as _;
use std::path::Path;

use super::types::TrustLevel;

pub enum AuditEvent<'a> {
    Installed {
        name: &'a str,
        source: &'a str,
        sha256: &'a str,
        trust: &'a TrustLevel,
    },
    Removed {
        name: &'a str,
    },
    Verified {
        name: &'a str,
        result: &'a str,
    },
    Loaded {
        name: &'a str,
        trust: &'a TrustLevel,
    },
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

fn current_timestamp() -> String {
    crate::utils::iso_timestamp()
}

fn format_event(event: &AuditEvent<'_>) -> String {
    let ts = current_timestamp();
    match event {
        AuditEvent::Installed {
            name,
            source,
            sha256,
            trust,
        } => {
            format!(
                r#"{{"timestamp":"{}","event":"installed","name":"{}","source":"{}","sha256":"{}","trust":"{}"}}"#,
                escape_json_string(&ts),
                escape_json_string(name),
                escape_json_string(source),
                escape_json_string(sha256),
                trust,
            )
        }
        AuditEvent::Removed { name } => {
            format!(
                r#"{{"timestamp":"{}","event":"removed","name":"{}"}}"#,
                escape_json_string(&ts),
                escape_json_string(name),
            )
        }
        AuditEvent::Verified { name, result } => {
            format!(
                r#"{{"timestamp":"{}","event":"verified","name":"{}","result":"{}"}}"#,
                escape_json_string(&ts),
                escape_json_string(name),
                escape_json_string(result),
            )
        }
        AuditEvent::Loaded { name, trust } => {
            format!(
                r#"{{"timestamp":"{}","event":"loaded","name":"{}","trust":"{}"}}"#,
                escape_json_string(&ts),
                escape_json_string(name),
                trust,
            )
        }
    }
}

pub fn log_event(data_dir: &Path, event: &AuditEvent<'_>) {
    let log_path = data_dir.join("skills-audit.log");
    let line = format!("{}\n", format_event(event));

    let file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&log_path);

    match file {
        Ok(mut f) => {
            if let Err(e) = f.write_all(line.as_bytes()) {
                eprintln!("warning: failed to write audit log: {e}");
            }
        }
        Err(e) => {
            eprintln!("warning: failed to open audit log: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_json_string_plain() {
        assert_eq!(escape_json_string("hello"), "hello");
    }

    #[test]
    fn escape_json_string_quotes() {
        assert_eq!(escape_json_string(r#"say "hello""#), r#"say \"hello\""#);
    }

    #[test]
    fn escape_json_string_backslash() {
        assert_eq!(escape_json_string(r"path\to\file"), r"path\\to\\file");
    }

    #[test]
    fn escape_json_string_newlines() {
        assert_eq!(escape_json_string("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_json_string("line1\rline2"), "line1\\rline2");
    }

    #[test]
    fn escape_json_string_tab() {
        assert_eq!(escape_json_string("col1\tcol2"), "col1\\tcol2");
    }

    #[test]
    fn escape_json_string_mixed() {
        assert_eq!(
            escape_json_string("\"hello\"\n\tworld\\"),
            "\\\"hello\\\"\\n\\tworld\\\\"
        );
    }

    #[test]
    fn format_event_installed() {
        let event = AuditEvent::Installed {
            name: "my-skill",
            source: "acme/my-skill",
            sha256: "abc123",
            trust: &TrustLevel::Sandboxed,
        };
        let json = format_event(&event);
        assert!(json.contains(r#""event":"installed""#));
        assert!(json.contains(r#""name":"my-skill""#));
        assert!(json.contains(r#""source":"acme/my-skill""#));
        assert!(json.contains(r#""sha256":"abc123""#));
        assert!(json.contains(r#""trust":"sandboxed""#));
        assert!(json.contains(r#""timestamp":""#));
    }

    #[test]
    fn format_event_removed() {
        let event = AuditEvent::Removed { name: "old-skill" };
        let json = format_event(&event);
        assert!(json.contains(r#""event":"removed""#));
        assert!(json.contains(r#""name":"old-skill""#));
    }

    #[test]
    fn format_event_verified() {
        let event = AuditEvent::Verified {
            name: "test-skill",
            result: "ok",
        };
        let json = format_event(&event);
        assert!(json.contains(r#""event":"verified""#));
        assert!(json.contains(r#""name":"test-skill""#));
        assert!(json.contains(r#""result":"ok""#));
    }

    #[test]
    fn format_event_loaded() {
        let event = AuditEvent::Loaded {
            name: "active-skill",
            trust: &TrustLevel::Trusted,
        };
        let json = format_event(&event);
        assert!(json.contains(r#""event":"loaded""#));
        assert!(json.contains(r#""name":"active-skill""#));
        assert!(json.contains(r#""trust":"trusted""#));
    }

    #[test]
    fn format_event_escapes_special_chars() {
        let event = AuditEvent::Installed {
            name: "skill-with-\"quotes\"",
            source: "path\\with\\backslash",
            sha256: "hash",
            trust: &TrustLevel::Standard,
        };
        let json = format_event(&event);
        assert!(json.contains(r#"skill-with-\"quotes\""#));
        assert!(json.contains(r#"path\\with\\backslash"#));
    }

    #[test]
    fn log_event_creates_file() {
        let tmp = std::env::temp_dir().join("__kx_audit_log__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let event = AuditEvent::Installed {
            name: "test",
            source: "acme/test",
            sha256: "abc",
            trust: &TrustLevel::Sandboxed,
        };
        log_event(&tmp, &event);

        let log_path = tmp.join("skills-audit.log");
        assert!(log_path.exists());

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("installed"));
        assert!(content.contains("test"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn log_event_appends() {
        let tmp = std::env::temp_dir().join("__kx_audit_append__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        log_event(
            &tmp,
            &AuditEvent::Installed {
                name: "first",
                source: "a/b",
                sha256: "x",
                trust: &TrustLevel::Sandboxed,
            },
        );
        log_event(&tmp, &AuditEvent::Removed { name: "second" });

        let content = std::fs::read_to_string(tmp.join("skills-audit.log")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("first"));
        assert!(lines[1].contains("second"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
