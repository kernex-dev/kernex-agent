use std::io::Read as _;
use std::path::Path;

use colored::Colorize;

use super::audit::{log_event, AuditEvent};
use super::manifest::{compute_sha256, skill_dir, verify_skill, SkillsManifest, VerifyResult};
use super::parser::{parse_skill_md, parse_source, validate_skill_name, validate_skill_size};
use super::permissions::{resolve_permissions, PermissionPolicy};
use super::types::{InstalledSkill, TrustLevel};

pub async fn list_skills(data_dir: &Path) {
    let manifest = SkillsManifest::load(data_dir);
    let skills = manifest.list();

    if skills.is_empty() {
        println!("{}", "  No skills installed.\n".dimmed());
        println!(
            "  Install a skill: {} <owner/repo>\n",
            "kx skills add".cyan()
        );
        return;
    }

    let count = skills.len();
    println!("\n  {}\n", "Active skills".bold());

    for skill in skills {
        let trust_badge = match skill.trust {
            TrustLevel::Sandboxed => "sandboxed".yellow(),
            TrustLevel::Standard => "standard".blue(),
            TrustLevel::Trusted => "trusted".green(),
        };

        println!("  {} [{}]", skill.name.bold(), trust_badge);
        println!("    {} {}", "Source:".dimmed(), skill.source);
        println!(
            "    {} {}",
            "Granted:".dimmed(),
            skill
                .granted_permissions
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        if !skill.denied_permissions.is_empty() {
            println!(
                "    {} {}",
                "Denied:".dimmed(),
                skill
                    .denied_permissions
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
    }

    println!(
        "  ({count} skill{} active)\n",
        if count == 1 { "" } else { "s" }
    );
}

pub async fn add_skill(
    data_dir: &Path,
    source: &str,
    trust_str: &str,
    policy: &PermissionPolicy,
    force: bool,
) -> Result<(), String> {
    let trust = match trust_str.to_lowercase().as_str() {
        "sandboxed" => TrustLevel::Sandboxed,
        "standard" => TrustLevel::Standard,
        "trusted" => TrustLevel::Trusted,
        other => {
            return Err(format!(
                "invalid trust level: {other} (use sandboxed, standard, or trusted)"
            ))
        }
    };

    let skill_source = parse_source(source).map_err(|e| e.to_string())?;
    let url = skill_source.raw_url();

    println!("  {} {}", "Fetching:".dimmed(), url);

    // ureq's AgentBuilder::timeout sets the *connect* timeout in 2.x. A
    // hostile/slow server that completes the handshake but stalls the body
    // would otherwise block this thread indefinitely. timeout_read covers
    // the body. spawn_blocking moves the whole synchronous fetch off the
    // tokio worker so it can't stall other tasks even on a slow link.
    let url_owned = url.clone();
    let body = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(5))
            .timeout_read(std::time::Duration::from_secs(15))
            .timeout_write(std::time::Duration::from_secs(5))
            .build();
        let response = agent.get(&url_owned).call().map_err(|e| {
            let msg = e.to_string();
            if msg.contains("timed out") || msg.contains("timeout") {
                "failed to fetch skill: request timed out. Check your connection or try again."
                    .to_string()
            } else {
                format!("failed to fetch skill: {e}")
            }
        })?;

        if response.status() != 200 {
            return Err(format!(
                "failed to fetch skill: HTTP {} - {}",
                response.status(),
                url_owned
            ));
        }

        let mut body = Vec::new();
        response
            .into_reader()
            .take(super::types::MAX_SKILL_SIZE + 1)
            .read_to_end(&mut body)
            .map_err(|e| format!("failed to read response: {e}"))?;
        Ok(body)
    })
    .await
    .map_err(|e| format!("skill fetch task panicked: {e}"))??;

    validate_skill_size(body.len() as u64).map_err(|e| e.to_string())?;

    let content_str =
        String::from_utf8(body.clone()).map_err(|_| "skill file is not valid UTF-8".to_string())?;

    let skill_manifest = parse_skill_md(&content_str).map_err(|e| e.to_string())?;

    if policy.is_blocked(&skill_manifest.name) {
        return Err(format!(
            "skill '{}' is blocked by project config",
            skill_manifest.name
        ));
    }

    let resolved = resolve_permissions(
        &skill_manifest.requested_permissions,
        source,
        policy,
        &skill_manifest.name,
    );

    println!("\n  {} {}", "Skill:".dimmed(), skill_manifest.name.bold());
    println!("  {} {}", "Source:".dimmed(), skill_source.display_source());
    println!(
        "  {} {}",
        "Description:".dimmed(),
        skill_manifest.description
    );
    println!("  {} {trust}", "Trust level:".dimmed());
    println!(
        "  {} {}",
        "Granted:".dimmed(),
        resolved
            .granted
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !resolved.denied.is_empty() {
        println!(
            "  {} {}",
            "Denied:".dimmed(),
            resolved
                .denied
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Refuse to silently shadow an existing skill from a different source.
    // Without this guard, `kx skills add hostile/repo` whose SKILL.md
    // declared `name = "senior-developer"` would replace the trusted
    // builtin's manifest entry (and overwrite its SHA-256), so a later
    // verify would report OK against the attacker's content.
    let new_source = skill_source.display_source();
    let mut manifest = SkillsManifest::load(data_dir);
    if let Some(existing) = manifest.find(&skill_manifest.name) {
        if existing.source != new_source && !force {
            return Err(format!(
                "skill '{}' is already installed from a different source ({}). \
                 Pass --force to replace it.",
                skill_manifest.name, existing.source
            ));
        }
    }

    let skill_path = skill_dir(data_dir).join(&skill_manifest.name);
    std::fs::create_dir_all(&skill_path)
        .map_err(|e| format!("failed to create skill directory: {e}"))?;

    let file_path = skill_path.join("SKILL.md");
    std::fs::write(&file_path, &body).map_err(|e| format!("failed to write skill file: {e}"))?;

    let sha256 = compute_sha256(&body);

    let installed = InstalledSkill {
        name: skill_manifest.name.clone(),
        source: new_source,
        sha256,
        size_bytes: body.len() as u64,
        installed_at: current_timestamp(),
        trust,
        granted_permissions: resolved.granted,
        denied_permissions: resolved.denied,
    };

    log_event(
        data_dir,
        &AuditEvent::Installed {
            name: &installed.name,
            source: &installed.source,
            sha256: &installed.sha256,
            trust: &installed.trust,
        },
    );

    manifest.add(installed);
    manifest
        .save(data_dir)
        .map_err(|e| format!("failed to save manifest: {e}"))?;

    println!(
        "\n  {} {} installed successfully.\n",
        "OK".green().bold(),
        skill_manifest.name.bold()
    );

    Ok(())
}

pub async fn remove_skill(data_dir: &Path, name: &str) -> Result<(), String> {
    let mut manifest = SkillsManifest::load(data_dir);

    if manifest.find(name).is_none() {
        return Err(format!("skill not found: {name}"));
    }

    let skill_path = skill_dir(data_dir).join(name);
    if skill_path.exists() {
        std::fs::remove_dir_all(&skill_path)
            .map_err(|e| format!("failed to remove skill directory: {e}"))?;
    }

    manifest.remove(name);
    manifest
        .save(data_dir)
        .map_err(|e| format!("failed to save manifest: {e}"))?;

    println!("\n  {} {} removed.\n", "OK".green().bold(), name.bold());

    log_event(data_dir, &AuditEvent::Removed { name });

    Ok(())
}

pub async fn verify_skills(data_dir: &Path) {
    // Verify is the integrity-check entry point; if the manifest itself is
    // corrupted, refuse rather than reporting "no skills installed".
    let manifest = match SkillsManifest::load_strict(data_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {e}", "error:".red().bold());
            eprintln!("  skills.toml is the integrity root; cannot proceed with verify.");
            return;
        }
    };
    let skills = manifest.list();

    if skills.is_empty() {
        println!("{}", "  No skills installed.\n".dimmed());
        return;
    }

    println!("\n  {}\n", "Verifying skills".bold());

    let mut ok_count = 0;
    let mut warn_count = 0;

    for skill in skills {
        match verify_skill(data_dir, skill) {
            VerifyResult::Ok => {
                println!("  {} {} (SHA256 OK)", "OK".green(), skill.name);
                log_event(
                    data_dir,
                    &AuditEvent::Verified {
                        name: &skill.name,
                        result: "ok",
                    },
                );
                ok_count += 1;
            }
            VerifyResult::Modified { expected, actual } => {
                let exp_short = truncate_hash(&expected);
                let act_short = truncate_hash(&actual);
                println!(
                    "  {} {} (modified!)",
                    "FAIL".red().bold(),
                    skill.name.bold()
                );
                println!("    {} {exp_short}", "Expected:".dimmed());
                println!("    {} {act_short}", "Actual:".dimmed());
                log_event(
                    data_dir,
                    &AuditEvent::Verified {
                        name: &skill.name,
                        result: "modified",
                    },
                );
                warn_count += 1;
            }
            VerifyResult::Missing => {
                println!(
                    "  {} {} (file missing!)",
                    "FAIL".red().bold(),
                    skill.name.bold()
                );
                log_event(
                    data_dir,
                    &AuditEvent::Verified {
                        name: &skill.name,
                        result: "missing",
                    },
                );
                warn_count += 1;
            }
        }
    }

    println!();

    if warn_count == 0 {
        println!(
            "  {} All {ok_count} skill(s) verified.\n",
            "OK".green().bold()
        );
    } else {
        println!(
            "  {} {warn_count} skill(s) have issues.\n",
            "WARN".yellow().bold()
        );
    }
}

pub fn lint_skill_dir(path: &std::path::Path) -> bool {
    let skill_path = path.join("SKILL.md");

    let content = match std::fs::read_to_string(&skill_path) {
        Ok(c) => c,
        Err(_) => {
            println!(
                "{} SKILL.md not found in {}",
                "ERROR".red().bold(),
                path.display()
            );
            return false;
        }
    };

    let (frontmatter, body) = match skill_frontmatter_and_body(&content) {
        Some(pair) => pair,
        None => {
            println!(
                "{} missing or malformed frontmatter (expected --- delimiters)",
                "ERROR".red().bold()
            );
            return false;
        }
    };

    let name = extract_fm_value(frontmatter, "name").unwrap_or_default();
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Required fields
    if extract_fm_value(frontmatter, "name").is_none() {
        errors.push("missing required field: name".to_string());
    }
    if extract_fm_value(frontmatter, "description").is_none() {
        errors.push("missing required field: description".to_string());
    }

    // Recommended fields
    if extract_fm_value(frontmatter, "version").is_none() {
        warnings.push("missing recommended field: version".to_string());
    }
    if extract_fm_value(frontmatter, "author").is_none() {
        warnings.push("missing recommended field: author".to_string());
    }

    // Name validation (only if present)
    if !name.is_empty() {
        if let Err(e) = validate_skill_name(&name) {
            errors.push(format!("invalid name: {e}"));
        }
        if name.contains("--") {
            errors.push(format!(
                "invalid name '{name}': consecutive hyphens not allowed"
            ));
        }
    }

    // Required section
    if !has_section(body, &["Workflow"]) {
        errors.push("missing required section: ## Workflow".to_string());
    }

    // Recommended sections
    if !has_section(body, &["Examples"]) {
        warnings.push("missing recommended section: ## Examples".to_string());
    }
    if !has_section(body, &["Output Format"]) {
        warnings.push("missing recommended section: ## Output Format".to_string());
    }

    // Anti-pattern detection
    let body_lower = body.to_lowercase();
    for pattern in &["ask the user", "ask for clarification", "prompt the user"] {
        let count = body_lower.matches(pattern).count();
        if count > 0 {
            warnings.push(format!("anti-pattern ({count}x): \"{pattern}\""));
        }
    }

    let display_name = if name.is_empty() { "unknown" } else { &name };
    print_lint_results(display_name, &errors, &warnings);

    errors.is_empty()
}

fn skill_frontmatter_and_body(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_start();
    let rest = trimmed.strip_prefix("---")?;
    let closing_idx = rest.find("\n---")?;
    let fm = &rest[..closing_idx];
    let after = &rest[closing_idx + 4..]; // skip "\n---"
    let body = after.strip_prefix('\n').unwrap_or(after);
    Some((fm, body))
}

fn extract_fm_value(frontmatter: &str, key: &str) -> Option<String> {
    let yaml_prefix = format!("{key}:");
    let toml_prefix = format!("{key} =");
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(yaml_prefix.as_str()) {
            let val = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !val.is_empty() {
                return Some(val);
            }
        } else if let Some(rest) = trimmed.strip_prefix(toml_prefix.as_str()) {
            let val = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

fn has_section(body: &str, keywords: &[&str]) -> bool {
    body.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("##") && keywords.iter().any(|kw| trimmed.contains(kw))
    })
}

fn print_lint_results(name: &str, errors: &[String], warnings: &[String]) {
    println!("\n  {} {}\n", "Linting skill:".dimmed(), name.bold());

    for err in errors {
        println!("  {} {}", "ERROR".red().bold(), err);
    }
    for warn in warnings {
        println!("  {} {}", "WARN".yellow().bold(), warn);
    }

    if errors.is_empty() && warnings.is_empty() {
        println!("  {} No issues found.", "OK".green().bold());
    } else if errors.is_empty() {
        println!(
            "\n  {} {} warning(s), 0 errors.",
            "OK".green().bold(),
            warnings.len()
        );
    } else {
        println!(
            "\n  {} {} error(s), {} warning(s).",
            "FAIL".red().bold(),
            errors.len(),
            warnings.len()
        );
    }
    println!();
}

fn truncate_hash(hash: &str) -> &str {
    if hash.len() >= 16 {
        &hash[..16]
    } else {
        hash
    }
}

fn current_timestamp() -> String {
    crate::utils::iso_timestamp()
}
