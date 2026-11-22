//! The compose files under `deploy/` pin the image to a release. This test keeps
//! that pin on the crate version, so a release cannot ship compose files that
//! still point at the previous image.

use std::path::Path;

const PINNED: [&str; 3] = [
    "deploy/docker-compose.local.yml",
    "deploy/docker-compose.vps.yml",
    "deploy/docker-compose.traefik.yml",
];

#[test]
fn compose_files_pin_the_crate_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // `deploy/` is not part of the published crate.
    if !root.join("deploy").is_dir() {
        return;
    }
    let want = format!(
        "image: ghcr.io/kernex-dev/kernex-agent:${{KX_VERSION:-{}}}",
        env!("CARGO_PKG_VERSION")
    );
    for file in PINNED {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        assert!(text.contains(&want), "{file} does not contain `{want}`");
        assert!(
            !text.contains("kernex-agent:latest"),
            "{file} tracks the moving `latest` tag"
        );
    }
}
