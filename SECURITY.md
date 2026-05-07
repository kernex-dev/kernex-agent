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
