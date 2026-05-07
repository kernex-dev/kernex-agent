# Security Policy

## Reporting a Vulnerability

kx is a thin CLI wrapper around [kernex-runtime](https://github.com/kernex-dev/kernex). For security vulnerabilities:

**Email:** security@kernex.dev

**Response SLA:**
- Acknowledgment: 48 hours
- Initial assessment: 7 days
- Fix timeline: Based on severity

## Supported Versions

Only the most recent minor release line receives security fixes. Older
lines are end-of-life as soon as a new minor ships. Upgrade is the
recommended remediation; backports are not provided.

| Version | Supported |
|---------|-----------|
| 0.4.x   | Yes       |
| < 0.4   | No        |

## Security Model

kx inherits security from kernex-runtime:

1. **OS-level sandbox** — Seatbelt (macOS), Landlock (Linux)
2. **Code-level enforcement** — Path validation via SandboxProfile
3. **No credential storage** — API keys are read from environment variables only; never written to disk by kx
4. **Skills integrity** — SHA-256 hash verified on install; `kx skills verify` detects post-install tampering
5. **Skills audit log** — all install/remove/verify operations logged to `~/.kernex/audit.log`

## Best Practices

- Keep your AI provider's CLI or SDK updated
- Don't run kx as root
- Review `.kx.toml` before using third-party project configs
- Review skill permissions before installing community skills (`kx skills add` shows granted/denied permissions before writing anything)
- Use `kx skills verify` after installing skills from untrusted sources

## Threat Model Caveats

These limitations are deliberate trade-offs. They are documented here so
operators do not assume stronger guarantees than the implementation
provides.

### Skills SHA-256 manifest is integrity, not authenticity

The hashes recorded by `kx skills add` are TOFU (Trust On First Use).
They detect post-install tampering of files on disk and they detect a
network-tampered initial download if the hash you compare against came
from a separate trusted channel. They do **not** prove that the entity
who published the skill is who they claim to be. There is no signature
verification today. If you install a skill from a compromised upstream,
the manifest will faithfully record the compromised content.

Guidance: only install skills from sources you have evaluated, prefer
sources that publish their own out-of-band hashes, and re-run `kx skills
verify` after upgrades.

### Skill permission model is advisory

`SKILL.md` frontmatter declares the tools, paths, and commands a skill
expects to use. The kx runtime's `HookRunner` currently logs (when
`--verbose` is on) and otherwise allows every tool call. Path-traversal
and shell-metacharacter blocks happen earlier in `kernex-skills` and at
the OS sandbox layer (Seatbelt/Landlock), so a skill cannot escape the
sandbox just because the hook said yes. But the per-tool allow-list in
`SKILL.md` is documentation, not a runtime gate.

Guidance: treat `SKILL.md` permissions as contract-with-the-author, not
runtime enforcement. Trust skills accordingly.

### Builtin skills are auto-trusted

Builtins shipped inside the binary (installed by `kx init`) are stamped
`TrustLevel::Trusted` automatically. They are reviewed before each
release and bundled at build time, so they do not present a runtime
fetch attack surface, but they also do not require operator
confirmation. If you do not want builtins on a host, either skip
`kx init` for that project or run `kx skills remove <name>` after init.
