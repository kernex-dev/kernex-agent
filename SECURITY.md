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
| 0.6.x   | Yes       |
| < 0.6   | No        |

## Security Model

What follows describes enforced behavior, not aspirations. Platform limits
and deliberate trade-offs are stated below so operators do not assume
stronger guarantees than the implementation provides.

### Tool execution is sandboxed, fail-closed

kx builds every provider with OS-level sandbox enforcement REQUIRED: on a
host that cannot apply Seatbelt (macOS) or Landlock (Linux 5.13+), agent
tool subprocesses are refused rather than silently run unsandboxed.
Inside the sandbox (enforced by the kernex crates):

- Subprocesses start from a cleared environment plus a minimal base
  allowlist; provider API keys never reach them implicitly.
- Writes inside `$HOME` are denied outside the workspace/data, temp, and
  toolchain cache dirs; credential stores (`~/.ssh`, `~/.aws`,
  `~/.gnupg`, and friends) are read-denied.
- Network egress is denied by default; a tool gets it only by declaring
  `network = true`. Full socket coverage on macOS; TCP-only on Linux
  6.7+ (older kernels cannot restrict the network and the gap is logged
  at spawn time). UDP and non-TCP sockets are never restricted on Linux.

### Skills

- **Ref pinning** — `kx skills add owner/repo@<sha|tag>` installs that
  exact ref; the manifest records the requested ref and the resolved
  commit SHA alongside the content SHA-256.
- **Integrity** — SHA-256 verified on install; `kx skills verify` detects
  post-install tampering. Install/remove/verify operations are logged to
  the audit log.
- **Enforced runtime permissions** — the `[permissions]` a skill declares
  in its frontmatter are enforced by the runtime, not advisory: the
  command allow-list is checked at load time and re-checked by the tool
  executor immediately before every spawn; only declared environment
  variable names pass through to the tool subprocess (dynamic-linker
  names are refused even when declared); network access requires the
  declared opt-in.
- **Configurator write safety** — installs refuse to write through
  symlinked targets, and backups archive symlinks as links, so a planted
  link can neither redirect writes nor smuggle foreign content into a
  restore.

### Release integrity

Standalone binaries ship with a `SHA256SUMS` file and per-artifact
sigstore attestations (SLSA v1 provenance, signed via GitHub's OIDC
identity). Verify before running:

```
shasum -a 256 -c SHA256SUMS
gh attestation verify kx-<version>-<target>.tar.gz --repo kernex-dev/kernex-agent
```

The Docker image carries the same provenance pattern (SLSA + SBOM,
verifiable with `cosign verify-attestation`).

### Credentials

API keys are read from environment variables only; kx never writes them
to disk.

## Deployment Warnings (read before exposing kx serve)

- **`kx serve` has no TLS and no rate limiting.** It speaks plain HTTP
  and serves requests as fast as they arrive. Run it behind a reverse
  proxy (Caddy, nginx) that terminates TLS and enforces rate limits, and
  always set `--auth-token`. Never expose a bare `kx serve` port to an
  untrusted network.
- **Job results are stored in plaintext at rest** (`jobs.db` under the kx
  data directory). Anything an agent reads or produces during a serve job
  lands there unencrypted; rely on full-disk encryption and filesystem
  permissions for at-rest protection, and scrub the data directory when
  decommissioning a host. This is an accepted, documented trade-off: an
  encrypted store would not protect against an attacker who can already
  read the same user's files, which is outside kx's threat model.

## Threat Model Caveats

These limitations are deliberate trade-offs, documented so operators do
not assume stronger guarantees than the implementation provides.

### Skills SHA-256 manifest is integrity, not authenticity

The hashes recorded by `kx skills add` are TOFU (Trust On First Use).
They detect post-install tampering and, combined with a commit-SHA pin,
they make the install reproducible and auditable. They do **not** prove
that the entity who published the skill is who they claim to be; there is
no publisher signature verification today. If you install a skill from a
compromised upstream, the manifest will faithfully record the compromised
content.

Guidance: pin installs to a commit SHA, only install skills from sources
you have evaluated, and re-run `kx skills verify` after upgrades.

### The kx hook layer is observability, not the permission gate

kx's `HookRunner` logs tool calls (with `--verbose`) and allows them; it
is not where permissions are enforced. Enforcement lives below it: the
skill loader's validation chain, the executor's pre-spawn allow-list
re-check, and the OS sandbox. A skill cannot bypass those layers by the
hook saying yes.

### Builtin skills are auto-trusted

Builtins shipped inside the binary (installed by `kx init`) are stamped
`TrustLevel::Trusted` automatically. They are reviewed before each
release and bundled at build time, so they do not present a runtime
fetch attack surface, but they also do not require operator
confirmation. If you do not want builtins on a host, either skip
`kx init` for that project or run `kx skills remove <name>` after init.

### Filesystem permissions are enforced on Unix only

`kx serve` chmods its SQLite job database to `0o600` and its data
directory to `0o700` at startup so other local accounts cannot read
queued messages, provider responses, or webhook payloads, and the skills
manifest applies the same `0o600` discipline. This hardening is
`#[cfg(unix)]` only; on Windows the equivalent ACL restrictions are not
applied today, and the files inherit whatever default ACL the parent
directory grants.

Guidance: if you run `kx serve` on Windows, treat the data directory
(`%LOCALAPPDATA%\kernex\projects\serve\`) as needing manual ACL
hardening, or run the daemon under a dedicated low-privilege user whose
profile is not readable by others.

## Best Practices

- Keep your AI provider's CLI or SDK updated
- Don't run kx as root
- Review `.kx.toml` before using third-party project configs
- Review skill permissions before installing community skills (`kx skills
  add` shows granted/denied permissions before writing anything)
- Pin skills to a commit SHA for reproducible installs
- Use `kx skills verify` after installing skills from untrusted sources
