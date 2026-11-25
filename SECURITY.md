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
| 0.7.x   | Yes       |
| < 0.7   | No        |

## Security Model

What follows describes enforced behavior, not aspirations. Platform limits
and deliberate trade-offs are stated below so operators do not assume
stronger guarantees than the implementation provides.

### Tool subprocesses are sandboxed, fail-closed

kx builds every provider with OS-level sandbox enforcement REQUIRED: on a
host that cannot apply Seatbelt (macOS) or Landlock (Linux 6.2+, Landlock
ABI 3), agent tool subprocesses are refused rather than silently run
unsandboxed.

This covers shell commands, toolboxes, MCP servers declared by skills, and
the `claude` and `opencode` CLIs. Inside the sandbox (enforced by the
kernex crates):

- Subprocesses start from a cleared environment plus a minimal base
  allowlist; provider API keys never reach them implicitly. The one
  exception is the claude-code CLI, which is given its login token
  (`CLAUDE_CODE_OAUTH_TOKEN`, when set), and the commands it runs inherit
  it.
- Writes are allowed only under the project directory, the temp
  directories and the toolchain cache directories. The kx data directory
  (`~/.kx/projects/<name>`, with its memory database, skills and the
  Claude CLI's login) and `~/.kx/config.toml` cannot be written.
- Credential stores (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.npmrc`,
  `~/.git-credentials`, agent CLI state such as `~/.claude`, and friends),
  shell startup files, the memory database, `~/.kx/config.toml` and every
  other project's data directory under `~/.kx/projects/` (its memory,
  skills and job history) are read-denied and write-denied. The list of
  other projects is read when the provider is built and grows by one path
  per project.
- Network egress is denied by default for shell commands, toolboxes and
  MCP servers; a tool gets it only by declaring `network = true`. The
  agent CLIs are the exception (see below). Full socket coverage on
  macOS; TCP-only on Linux 6.7+ (older kernels cannot restrict the network
  and the gap is logged at spawn time). UDP and non-TCP sockets are never
  restricted on Linux.
- An MCP server starts only when its skill lists the server's command in
  `permissions.commands`, and has no network unless the skill declares
  `permissions.network` (or `network = true` on the server entry).
- The `claude` and `opencode` CLIs always have the network: they are the
  provider. claude-code also gets the macOS keychain for its login. With
  no tool list (`kx dev`, one-shot runs, pipelines, `kx serve --tools
  full`) each CLI runs its tools without asking, inside this sandbox; a
  tool list never runs with the permission checks off. Every command the
  CLI runs shares its sandbox, so it has the network and, for
  claude-code, the keychain and the login token. With either CLI as the
  provider (claude-code is the default), a steered session can send what
  it can read to any host, whatever `web_fetch` says.

### File tools and `web_fetch`

The `read`, `write`, `edit`, `grep` and `glob` tools run inside the kx
process, so the runtime checks every path they are given: writes only
under the project directory, reads only under the project directory, the
data directory's skills, and directories you add with `--read-path` or
`read_paths` in `~/.kx/config.toml`. The deny list above applies inside
those roots too.

`web_fetch` is off by default and not offered to the model. The setting
governs kx's own tool; it does not take the network away from the
`claude` and `opencode` CLIs (see above). Turn it on with
`--web-fetch <host>` or `web_fetch = [...]` in `~/.kx/config.toml` for a
host list, or with `all` for any public host. Private and loopback addresses are
refused, and every redirect hop is checked like the first URL. The
claude-code and opencode CLIs get their own fetch and search tools only
under `all`, because they cannot be held to a host list.

An agent that reads untrusted text can still be steered within those
limits; with `web_fetch` on, it can send what it can read to an allowed
host, and with an agent CLI as the provider, to any host. kx has no per-call approval prompt yet.

### Memory

`--no-memory` neither loads from nor writes to the project's memory
database, and `/clear` in such a session writes nothing. The REPL then
also skips its input history file and does not run scheduled tasks.
`kx serve` jobs
load nothing from it, and a job that does not name a channel stores nothing
in it either.

### Project files do not choose the endpoint

`provider.name` and `provider.base_url` in a project's `.kx.toml` are
ignored with a warning. A repository you cloned cannot decide which
provider kx talks to or where your API key is sent; only `--provider`,
`--base-url` and their environment variables can.

Nor can it widen what the tools reach. `web_fetch` and `read_paths` come
only from `~/.kx/config.toml` and the command line. A project's `.kx.toml`
may set `web_fetch = "none"`, which only narrows; any other `web_fetch` or
`read_paths` there is ignored with a warning.

### Skills

- **Ref pinning** — `kx skills add owner/repo@<sha|tag>` installs that
  exact ref; the manifest records the requested ref and the resolved
  commit SHA alongside the content SHA-256.
- **Integrity** — SHA-256 verified on install; `kx skills verify` detects
  post-install tampering. Install/remove/verify operations are logged to
  the audit log.
- **MCP servers need declared commands.** The runtime starts a skill's
  MCP server only when the skill lists the server's command in
  `permissions.commands`, and gives it the network only when the skill
  declares `permissions.network`. A server that downloads itself
  (`npx -y ...`) needs both.
- **Names and descriptions are plain text.** kx refuses to start while an
  installed skill's name or description, as the runtime reads it, holds a
  control character (a newline, or an escape that decodes to one), since
  the runtime lists both in the prompt.
- **Commands need explicit trust** — a skill that declares an MCP server
  or a toolbox is refused by `kx skills add` unless installed with
  `--trust trusted`, which prints the command lines first. At startup kx
  asks the runtime what each installed skill would launch and refuses to
  run if such a skill is below `trusted`, missing from the manifest,
  changed since install, or carries an `mcp.json` / `toolbox.json`
  sidecar.
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

The claude-code provider runs the Claude CLI with its own config
directory, `cli/claude` inside the project's kx data directory, never the
user's `~/.claude`. A tool call cannot add a hook or an MCP server that the
user's next interactive Claude session would run outside the sandbox. The
cost is a separate login: `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_API_KEY`,
or a one-time `CLAUDE_CONFIG_DIR=<that directory> claude` login.

## Deployment Warnings (read before exposing kx serve)

- **`kx serve` has no TLS and no rate limiting.** It speaks plain HTTP
  and serves requests as fast as they arrive. Run it behind a reverse
  proxy (Caddy, nginx) that terminates TLS and enforces rate limits, and
  always set `--auth-token`. Never expose a bare `kx serve` port to an
  untrusted network.
- **Jobs run with read-only tools unless you start the server with
  `--tools full`.** A job reads text nobody vetted (a webhook payload, a
  file under review, a fetched page), and that text can try to steer the
  model. Under `read-only` a job can read and search files and nothing
  else: no shell, no file writes, no `web_fetch`, no MCP or toolbox calls,
  so a steered job cannot change the host or send data anywhere but its
  own output. Under `full` it can do all of that with whatever the process
  can reach, including the provider credentials. Use `full` only where you
  control the input of every job, and keep the bearer token as secret as
  those credentials. Two limits: with the `claude-code` provider the
  restriction is the allowlist the `claude` CLI is started with, and MCP
  servers declared by a skill you installed as `trusted` stay available
  to it; the `opencode` provider has no allowlist and is refused under
  `read-only`. A read-only job can still read any file under the server's
  working directory, its `--read-path` directories and the project's
  skills, outside the blocked credential paths, and return it to the
  caller. `web_fetch` stays off under `read-only` whatever `--web-fetch`
  says.
- **Jobs do not see each other's conversation.** Each job starts with an
  empty history. A `/run` request that names a `channel` opts into a
  conversation shared by every request on that channel; webhook jobs
  never share one.
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
hook saying yes. The one exception is `kx serve`, whose hook refuses
tool calls outside the server's tool profile (see Deployment Warnings).

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
