//! Stage 3 REVIEW — print plan, prompt unless `--yes` or `--dry-run`.
//!
//! Behavior per E-review-1..5.

use std::io::{BufRead, IsTerminal};

use chrono::Utc;
use serde_json::json;

use crate::install::audit::{AuditEvent, AuditWriter, EventStatus, Stage};

use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

pub async fn run(
    opts: &InstallOptions,
    plan: &InstallPlan,
    audit: &AuditWriter,
) -> Result<(), InstallError> {
    let started = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.review.start".to_string(),
            stage: Stage::Review,
            status: EventStatus::Success,
            started_at: started,
            ended_at: None,
            duration_ms: None,
            payload: json!({"agent": &plan.agent, "components": &plan.components}),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    print_plan(plan);

    let outcome = decide(opts, &mut std::io::stdin().lock())?;

    let ended = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.review.end".to_string(),
            stage: Stage::Review,
            status: match outcome {
                ReviewOutcome::Continue { skipped_prompt: _ } => EventStatus::Success,
            },
            started_at: started,
            ended_at: Some(ended),
            duration_ms: Some((ended - started).num_milliseconds().max(0) as u64),
            payload: json!({
                "decision": match outcome {
                    ReviewOutcome::Continue { skipped_prompt: true } => "skipped_prompt",
                    ReviewOutcome::Continue { skipped_prompt: false } => "user_confirmed",
                }
            }),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    Ok(())
}

#[derive(Debug)]
enum ReviewOutcome {
    Continue { skipped_prompt: bool },
}

fn print_plan(plan: &InstallPlan) {
    println!("\nInstall plan:");
    println!("  agent: {}", plan.agent);
    println!("  components:");
    for (component, path) in &plan.target_paths {
        println!("    - {component} -> {}", path.display());
    }
    println!();
}

fn decide<R: BufRead>(
    opts: &InstallOptions,
    reader: &mut R,
) -> Result<ReviewOutcome, InstallError> {
    if opts.yes || opts.dry_run {
        return Ok(ReviewOutcome::Continue {
            skipped_prompt: true,
        });
    }
    if !std::io::stdin().is_terminal() {
        return Err(InstallError::NonInteractive);
    }
    println!("Proceed? [y/N]");
    let mut buf = String::new();
    reader
        .read_line(&mut buf)
        .map_err(|e| InstallError::Permanent(format!("stdin read failed: {e}")))?;
    if buf.trim().eq_ignore_ascii_case("y") {
        Ok(ReviewOutcome::Continue {
            skipped_prompt: false,
        })
    } else {
        Err(InstallError::UserDeclined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configurator::stage_resolve::InstallPlan;
    use std::path::PathBuf;

    fn opts(yes: bool, dry: bool) -> InstallOptions {
        InstallOptions {
            agent: "claude-code".to_string(),
            preset: "solo-dev".to_string(),
            yes,
            dry_run: dry,
            verify_deep: false,
            home: PathBuf::from("/tmp/kx-review-test"),
        }
    }

    fn plan_stub() -> InstallPlan {
        InstallPlan {
            agent: "claude-code".to_string(),
            components: vec!["claude-md".into()],
            target_paths: vec![("claude-md".into(), PathBuf::from("/tmp/x/CLAUDE.md"))],
        }
    }

    #[test]
    fn decide_yes_flag_skips_prompt() {
        let mut empty: &[u8] = b"";
        let out = decide(&opts(true, false), &mut empty).unwrap();
        assert!(matches!(
            out,
            ReviewOutcome::Continue {
                skipped_prompt: true
            }
        ));
    }

    #[test]
    fn decide_dry_run_skips_prompt() {
        let mut empty: &[u8] = b"";
        let out = decide(&opts(false, true), &mut empty).unwrap();
        assert!(matches!(
            out,
            ReviewOutcome::Continue {
                skipped_prompt: true
            }
        ));
    }

    #[test]
    fn decide_non_tty_without_yes_aborts() {
        // Tests run with stdin NOT attached to a TTY; the function should
        // return NonInteractive when neither --yes nor --dry-run is set.
        let mut empty: &[u8] = b"";
        let err = decide(&opts(false, false), &mut empty).expect_err("must error");
        assert!(matches!(err, InstallError::NonInteractive));
    }

    // E-review-3 (interactive 'y' prompt) covered by manual testing;
    // mocking the TTY-attached stdin from a unit test is environment-
    // dependent. The dispatcher signature in `decide` is set up to take
    // a BufRead so a future change can extract `decide` further and test
    // both branches with a piped stream when atty/console mocks are added.

    #[test]
    fn plan_stub_is_constructible() {
        // Keeps the test-only import alive and gives a baseline that
        // ReviewOutcome::Continue can pair with a real plan structurally
        // when run() is exercised via the orchestrator.
        let plan = plan_stub();
        assert_eq!(plan.agent, "claude-code");
    }
}
