//! Smoke tests for §1 configurator scaffolding.
//!
//! These exist to prove the module compiles, types round-trip through
//! serde, and `InstallError` renders cleanly. Behavior tests for each
//! stage land in §5-§11 alongside the actual implementations.

#![cfg(feature = "agent-claude")]

use std::path::PathBuf;

use kernex_agent::configurator::{InstallError, InstallOptions};

#[test]
fn configurator_compiles_with_all_stages_unimplemented() {
    // The mere fact that this file links is the assertion. A compile-time
    // regression in the module surface fails this test by failing to build.
    let _: fn(InstallOptions) -> _ = |opts| kernex_agent::configurator::run(opts);
}

#[test]
fn install_options_round_trip_serde() {
    let opts = InstallOptions {
        agent: "claude-code".to_string(),
        preset: "solo-dev".to_string(),
        yes: true,
        dry_run: false,
        verify_deep: false,
        cwd: None,
        home: PathBuf::from("/tmp/kx-smoke"),
    };
    let json = serde_json::to_string(&opts).expect("serialize InstallOptions");
    let back: InstallOptions = serde_json::from_str(&json).expect("deserialize InstallOptions");
    assert_eq!(back.agent, "claude-code");
    assert_eq!(back.preset, "solo-dev");
    assert!(back.yes);
    assert!(!back.dry_run);
    assert_eq!(back.home, PathBuf::from("/tmp/kx-smoke"));
}

#[test]
fn install_error_implements_thiserror() {
    let err = InstallError::UnknownAgent("nope".to_string());
    let display = format!("{err}");
    assert!(display.contains("unknown agent"));
    assert!(display.contains("nope"));

    let err = InstallError::UnknownPreset("missing".to_string());
    assert!(format!("{err}").contains("missing"));

    let err = InstallError::SandboxRefused {
        path: PathBuf::from("/etc/forbidden"),
    };
    assert!(format!("{err}").contains("/etc/forbidden"));
}
