# Security Policy

## Reporting a Vulnerability

kx is a thin CLI wrapper around [kernex-runtime](https://github.com/kernex-dev/kernex). For security vulnerabilities:

**Email:** security@kernex.dev

**Response SLA:**
- Acknowledgment: 48 hours
- Initial assessment: 7 days
- Fix timeline: Based on severity

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Security Model

kx inherits security from kernex-runtime:

1. **OS-level sandbox** — Seatbelt (macOS), Landlock (Linux)
2. **Code-level enforcement** — Path validation via SandboxProfile
3. **No credential storage** — Uses Claude CLI's auth, no local secrets

## Best Practices

- Keep Claude CLI updated
- Don't run kx as root
- Review `.kx.toml` before using third-party configs
