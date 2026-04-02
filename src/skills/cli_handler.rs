use std::io::Read as _;
use std::path::Path;

use colored::Colorize;

use super::audit::{log_event, AuditEvent};
use super::manifest::{
    compute_sha256, skill_dir, skill_file_path, verify_skill, SkillsManifest, VerifyResult,
};
use super::parser::{parse_skill_md, parse_source, validate_skill_size};
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

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("failed to fetch skill: {e}"))?;

    if response.status() != 200 {
        return Err(format!(
            "failed to fetch skill: HTTP {} - {}",
            response.status(),
            url
        ));
    }

    let mut body = Vec::new();
    response
        .into_reader()
        .take(super::types::MAX_SKILL_SIZE + 1)
        .read_to_end(&mut body)
        .map_err(|e| format!("failed to read response: {e}"))?;

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

    let skill_path = skill_dir(data_dir).join(&skill_manifest.name);
    std::fs::create_dir_all(&skill_path)
        .map_err(|e| format!("failed to create skill directory: {e}"))?;

    let file_path = skill_path.join("SKILL.md");
    std::fs::write(&file_path, &body).map_err(|e| format!("failed to write skill file: {e}"))?;

    let sha256 = compute_sha256(&body);

    let installed = InstalledSkill {
        name: skill_manifest.name.clone(),
        source: skill_source.display_source(),
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

    let mut manifest = SkillsManifest::load(data_dir);
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
    let manifest = SkillsManifest::load(data_dir);
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

#[allow(dead_code)]
pub fn skill_file(data_dir: &Path, name: &str) -> Option<String> {
    let path = skill_file_path(data_dir, name);
    std::fs::read_to_string(path).ok()
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
