#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integration test for ref-pinned skill installs.
//!
//! Runs a REAL `kx skills add owner/repo/path@<sha>` against a pinned,
//! immutable public commit of this very repository and asserts the exact
//! ref was installed: the manifest records the requested ref and resolved
//! commit, and the installed SKILL.md bytes hash to the known content of
//! the file AT THAT COMMIT (not whatever the default branch currently has).
//!
//! Needs network access to raw.githubusercontent.com (a given on CI, which
//! is where this gate matters). The test skips with a notice only when the
//! network is clearly unavailable (connect/DNS failure on a preflight);
//! any other failure is a real assertion failure.

use kernex_agent::skills::cli_handler::add_skill;
use kernex_agent::skills::manifest::SkillsManifest;
use kernex_agent::skills::permissions::PermissionPolicy;

/// Immutable public commit of kernex-dev/kernex-agent where
/// `builtins/skill-factory/SKILL.md` exists with known content.
const PINNED_SHA: &str = "868b34b3ee9f773d1b6ad57740db951039f3e53c";
/// sha256 of `builtins/skill-factory/SKILL.md` at exactly that commit.
const PINNED_CONTENT_SHA256: &str =
    "be33ca7f46a7073fe8690023cd51da39f35dd2638312a6b368050df568160714";

fn network_available() -> bool {
    std::net::TcpStream::connect_timeout(
        &"raw.githubusercontent.com:443"
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .unwrap_or_else(|| "0.0.0.0:443".parse().unwrap()),
        std::time::Duration::from_secs(5),
    )
    .is_ok()
}

use std::net::ToSocketAddrs;

#[tokio::test]
async fn pinned_install_records_and_delivers_exact_ref() {
    if !network_available() {
        eprintln!("skipping skills_ref_pinning: no network to raw.githubusercontent.com");
        return;
    }

    let data_dir = tempfile::tempdir().expect("temp data dir");
    let source = format!("kernex-dev/kernex-agent/builtins/skill-factory@{PINNED_SHA}");

    add_skill(
        data_dir.path(),
        &source,
        "sandboxed",
        &PermissionPolicy::default(),
        false,
    )
    .await
    .expect("pinned install failed");

    // 1. The manifest records the pin: requested ref and resolved commit
    //    are exactly the pinned SHA.
    let manifest = SkillsManifest::load(data_dir.path());
    let installed = manifest
        .find("skill-factory")
        .expect("installed skill missing from manifest");
    assert_eq!(installed.requested_ref.as_deref(), Some(PINNED_SHA));
    assert_eq!(installed.resolved_commit.as_deref(), Some(PINNED_SHA));

    // 2. The exact ref was delivered: the installed bytes hash to the
    //    file's content at that commit, regardless of what the default
    //    branch has moved to since.
    assert_eq!(
        installed.sha256, PINNED_CONTENT_SHA256,
        "installed content does not match the pinned commit's bytes"
    );
}
