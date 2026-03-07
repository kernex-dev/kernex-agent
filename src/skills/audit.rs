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
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let z = days as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
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
