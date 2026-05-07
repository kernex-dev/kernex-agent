<p align="center">
  <img src="favicon.svg" alt="Kernex" width="80">
</p>

# kx

CLI dev assistant powered by [Kernex](https://github.com/kernex-dev/kernex).

## Features

- **11 LLM providers** - Claude Code (default), Anthropic, OpenAI, Gemini, Ollama, OpenRouter, Groq, Mistral, DeepSeek, Fireworks, xAI; AWS Bedrock available behind a Cargo feature
- **Stack detection** - Automatically detects Rust, Node/TypeScript, Python, Flutter/Dart, PHP, Go, Java, and Swift projects
- **Persistent memory** - Remembers decisions, patterns, and context across sessions per project, with FTS5 full-text search
- **Cost telemetry** - `/cost` slash command surfaces cumulative tokens, estimated cost, and prompt-cache hit ratio
- **OS sandbox** - Tool execution runs under Seatbelt (macOS) or Landlock (Linux) for filesystem and network isolation
- **Skills system** - Install reusable behavior packages (`kx skills add owner/repo`) with SHA-256 integrity verification and trust levels
- **MCP integration** - Connects to MCP servers per skill or project for extra tool surface
- **Headless server** - `kx serve` exposes an authenticated HTTP API for job submission, webhooks, and multi-step workflows
- **Multi-agent pipelines** - Topology-driven workflows with reality-check gates
- **Auto-compact** - Optional automatic context summarization when approaching the model's window
- **One-shot or interactive** - Run `kx "question"` for quick answers or `kx dev` for a REPL with multiline input via `"""` delimiters
- **Project configuration** - Per-project settings via `.kx.toml`

## What Can kx Do?

kx is your AI coding assistant. It can:

- **Answer questions** about your code, errors, and architecture
- **Suggest refactoring** patterns and improvements
- **Hunt for bugs** and explain potential issues
- **Explain errors** with context from your codebase
- **Remember context** across sessions (facts, decisions, patterns)
- **Search conversations** with full-text search

**Limitations:**
- File and shell tools run under sandbox; outside that surface kx suggests changes rather than applying them.
- An LLM provider must be available (default: Claude Code CLI; alternatives use API keys via env or `.kx.toml`).

## Requirements

kx needs at least one LLM provider configured. The default is the Claude Code CLI; any other provider can be selected via `--provider` or `.kx.toml`.

| Dependency | Minimum Version | When required |
|---|---|---|
| **Rust toolchain** | 1.74+ | For `cargo install kernex-agent` (not needed if using Docker) |
| **Claude CLI** | 2.0+ | Only when using the default `claude-code` provider |
| **Docker + Compose v2** | 24+ | Only for the Docker deployment path |

### Provider options

| Provider | Auth | Set with |
|---|---|---|
| `claude-code` (default) | Subscription via local CLI | `claude --version` to verify |
| `anthropic` | API key | `ANTHROPIC_API_KEY` env var |
| `openai` | API key | `OPENAI_API_KEY` env var |
| `gemini` | API key | `GEMINI_API_KEY` env var |
| `ollama` | Local | Run `ollama serve` |
| `openrouter` | API key | `OPENROUTER_API_KEY` env var |
| `groq` | API key | `GROQ_API_KEY` env var |
| `mistral` | API key | `MISTRAL_API_KEY` env var |
| `deepseek` | API key | `DEEPSEEK_API_KEY` env var |
| `fireworks` | API key | `FIREWORKS_API_KEY` env var |
| `xai` | API key | `XAI_API_KEY` env var |

To install Claude Code (the default):

1. Visit [claude.ai/download](https://claude.ai/download)
2. Download and install for your platform (macOS, Linux, Windows)
3. Run `claude --version` to verify installation (must be 2.0+)

Documentation: [docs.anthropic.com/en/docs/claude-code](https://docs.anthropic.com/en/docs/claude-code)

### Platform Support

| Platform | Status | Notes |
|---|---|---|
| **macOS** (Apple Silicon & Intel) | Fully supported | Sandbox via Seatbelt |
| **Linux** (x86_64, aarch64) | Fully supported | Sandbox via Landlock; partial enforcement on kernel 5.13+, full enforcement on 6.x+ |
| **Windows** | Experimental | No sandbox support; requires WSL2 for best experience |

The sandbox layer comes from `kernex-sandbox` and is used by the runtime to isolate AI subprocesses. On platforms without sandbox support, kx still works but without process isolation.

## Installation

### Quick Install (requires Rust)

```bash
cargo install kernex-agent
```

Verify installation:
```bash
kx --version
```

### New to Rust?

Install Rust first from [rustup.rs](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
cargo install kernex-agent
```

### From Source

```bash
git clone https://github.com/kernex-dev/kernex-agent.git
cd kernex-agent
cargo install --path .
```

## First Run

```bash
cd /path/to/your/project
kx
```

kx automatically detects your project's stack (Rust, Node, Python, etc.) and starts an interactive session:

```
kx dev my-project (Rust)
Type /help for commands, /quit to exit.

> explain the error in src/main.rs
```

For one-shot questions:
```bash
kx "what does this function do?"
```

## Quick Start

### One-shot mode

```bash
# Ask a quick question
kx "explain this error: cannot borrow as mutable"

# With dev subcommand
kx dev "add error handling to src/lib.rs"
```

### Interactive mode

```bash
# Start interactive session in current project
kx

# Or explicitly
kx dev
```

In interactive mode, type your questions and get responses. Use `/help` for available commands.

### Multiline input

For pasting code blocks or multi-line content:

```
> """
  1 | fn main() {
  2 |     println!("Hello");
  3 | }
  4 | """
  (3 lines captured)
```

## Serve Mode

`kx serve` runs kx as a headless HTTP daemon. Deploy it on a VPS or Mac Mini to accept agent jobs from external triggers, webhooks, or CI pipelines — no active terminal needed.

### Deploy with Docker (recommended)

No Rust installation required. Pull the pre-built image from GHCR.

**Mac Mini or local server (no TLS):**

```bash
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/docker-compose.local.yml
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/.env.example
cp .env.example .env
# Edit .env: set KERNEX_AUTH_TOKEN and CLAUDE_CREDENTIALS_PATH
docker compose -f docker-compose.local.yml up -d
curl http://localhost:8080/health
```

**VPS with a domain and automatic TLS:**

```bash
mkdir kx-server && cd kx-server
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/docker-compose.vps.yml
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/Caddyfile
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/Dockerfile.caddy
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/.env.example
cp .env.example .env
# Edit .env: set KERNEX_AUTH_TOKEN, CLAUDE_CREDENTIALS_PATH, DOMAIN, ACME_EMAIL
docker compose -f docker-compose.vps.yml up -d
curl https://api.yourdomain.com/health
```

The image ships with 16 pre-loaded skills and 4 workflows (PR review, feature design, security audit, GEO audit). Skills and job data persist in a named Docker volume across restarts and updates.

For the complete step-by-step guide, provider options, and customization: [deploy/SETUP.md](deploy/SETUP.md).

### Run from the CLI

If you already have `kx` installed:

```bash
kx serve --auth-token mysecrettoken
kx serve --host 0.0.0.0 --port 9000 --auth-token mysecrettoken --workers 8
```

The auth token can also be set via environment variable:

```bash
export KERNEX_AUTH_TOKEN=mysecrettoken
kx serve
```

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--host` | `127.0.0.1` | Bind address |
| `--port` | `8080` | Listen port |
| `--auth-token` | (required) | Bearer token for API authentication |
| `--workers` | `4` | Max concurrent agent jobs |

Provider flags (`--provider`, `--model`, `--api-key`, etc.) work the same as in interactive mode and set the default for all jobs.

### API

All endpoints except `/health` require `Authorization: Bearer <token>`. Jobs run asynchronously — `/run` returns a `job_id`, poll `/jobs/{id}` until status is `done` or `failed`.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check (no auth required) |
| `POST` | `/run` | Submit an agent job |
| `GET` | `/jobs` | List recent jobs (default limit: 50) |
| `GET` | `/jobs/{id}` | Get job status and output |
| `POST` | `/webhook/{event}` | Trigger a job from a webhook |

```bash
# Submit a job
curl -s -X POST http://localhost:8080/run \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Review this Express handler for SQL injection",
    "skills": ["security-engineer"]
  }' | jq .

# Poll for result
curl -s http://localhost:8080/jobs/<job_id> \
  -H "Authorization: Bearer <token>" | jq .

# Webhook trigger (e.g. from GitHub Actions)
curl -s -X POST http://localhost:8080/webhook/pr-review \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"message": "PR #42: add user auth endpoint"}'
```

## Commands

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/search <query>` | Search past conversations (FTS5) |
| `/history` | Show recent conversation history (last 20 messages) |
| `/stack` | Show detected stack and project info |
| `/memory` | Show memory stats and database size |
| `/facts` | List stored facts |
| `/facts delete <key>` | Delete a specific fact |
| `/config` | Show active configuration |
| `/retry` | Retry last failed message |
| `/clear` | Close current conversation |
| `/quit` or `/exit` | Exit kx |

## Configuration

Create a `.kx.toml` file in your project root to customize behavior:

```toml
# Override auto-detected stack
stack = "rust"

# Add project-specific instructions to the system prompt
system_prompt = """
This project uses a custom error type in src/error.rs.
Always use MyError instead of anyhow.
"""

# Provider settings
[provider]
model = "claude-sonnet-4-20250514"  # Model to use
max_turns = 10                       # Max agentic turns per request
timeout_secs = 300                   # Request timeout in seconds
```

### Stack options

Valid values for `stack`:

- `rust`
- `node`, `javascript`, `typescript`
- `python`
- `flutter`, `dart`
- `php`
- `go`, `golang`
- `java`, `kotlin`
- `swift`, `swiftui`

## Skills

kx supports the [Agent Skills](https://agentskills.io) standard — an open format for reusable AI agent capabilities. Skills are SKILL.md files that extend kx with specialized knowledge and guidelines.

### Installing Skills

```bash
# Install from GitHub
kx skills add anthropics/skills/rust-best-practices

# Install with specific trust level
kx skills add vercel-labs/agent-skills/web-design-guidelines --trust standard

# List installed skills
kx skills list

# Verify integrity (SHA-256)
kx skills verify

# Remove a skill
kx skills remove rust-best-practices
```

Skills can also be managed from the interactive REPL with `/skills`, `/skills add`, `/skills remove`, and `/skills verify`.

### Permission Model

Skills are text-only (SKILL.md), but they influence the AI's behavior. kx uses a permission model to give users control:

| Permission | Description | Risk |
|---|---|---|
| `context:files` | Reference project files | Low |
| `context:git` | Reference git history | Low |
| `suggest:edits` | Suggest code modifications | Medium |
| `suggest:commands` | Suggest shell commands | **High** |
| `suggest:network` | Suggest network requests | **High** |

### Trust Levels

| Level | Permissions | Use Case |
|---|---|---|
| **sandboxed** (default) | `context:files` only | Unknown skills |
| **standard** | `context:*`, `suggest:edits` | Verified skills |
| **trusted** | All permissions | Allowlisted sources |

### Configuration

Configure skills behavior in `.kx.toml`:

```toml
[skills]
default_trust = "sandboxed"
trusted_sources = ["anthropics/skills", "vercel-labs/agent-skills"]
blocked = ["suspicious-skill"]
```

### Security

- **Text only** — Skills are markdown files. No scripts, binaries, or executables.
- **SHA-256 integrity** — Every installed skill is hashed. Use `kx skills verify` to detect tampering.
- **Size limits** — Skills are capped at 64 KB.
- **Name validation** — Strict naming rules prevent path traversal attacks.
- **Prompt guardrails** — Skills are injected into the system prompt with XML delimiters and trust metadata. The AI is instructed to treat skills as untrusted third-party content.
- **Audit log** — All skill operations (install, remove, verify, load) are logged to `skills-audit.log`.
- **Blocklist** — Block specific skills via `.kx.toml` configuration.

## Stack Detection

kx automatically detects your project's stack by looking for these files (in order):

| File | Detected Stack |
|------|----------------|
| `Cargo.toml` | Rust |
| `go.mod` | Go |
| `Package.swift` | Swift/SwiftUI |
| `pubspec.yaml` | Flutter/Dart |
| `pom.xml` | Java |
| `build.gradle` / `build.gradle.kts` | Java |
| `package.json` | JavaScript/TypeScript (Node) |
| `requirements.txt` | Python |
| `pyproject.toml` | Python |
| `Pipfile` | Python |
| `composer.json` | PHP |

The first match wins. Override with `stack` in `.kx.toml` if needed.

## Data Storage

Project data is stored in:

```
~/.kx/projects/{project-name}/
```

Where `{project-name}` is derived from the directory name. Each project maintains its own:

- Conversation history
- Stored facts
- Input history (readline)

## Providers

kx defaults to Claude Code CLI as its AI backend. The underlying `kernex-providers` crate supports additional backends:

| Flag | Provider | Requires |
|------|----------|---------|
| `--provider claude-code` | Claude Code CLI (default) | Claude CLI installed |
| `--provider anthropic` | Anthropic API | `ANTHROPIC_API_KEY` |
| `--provider openai` | OpenAI API | `OPENAI_API_KEY` |
| `--provider ollama` | Ollama (local) | Ollama running at `localhost:11434` |
| `--provider gemini` | Google Gemini | `GEMINI_API_KEY` |
| `--provider openrouter` | OpenRouter | `OPENROUTER_API_KEY` |
| `--provider groq` | Groq | `GROQ_API_KEY` |
| `--provider mistral` | Mistral | `MISTRAL_API_KEY` |
| `--provider deepseek` | DeepSeek | `DEEPSEEK_API_KEY` |
| `--provider fireworks` | Fireworks AI | `FIREWORKS_API_KEY` |
| `--provider xai` | xAI (Grok) | `XAI_API_KEY` |

**AWS Bedrock** is available as an optional compile-time feature. Build with `--features bedrock` to enable `--provider bedrock`. Requires standard AWS credentials (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`). Bedrock is not included in the 11 built-in provider count above.

Provider is auto-detected if `--provider` is omitted. Override the model with `--model <name>`.

You can also set defaults via environment variables:

```bash
export KERNEX_PROVIDER=anthropic
export KERNEX_MODEL=claude-opus-4-6-20251001
```

## Architecture

kx is a thin CLI wrapper around the Kernex runtime:

- **kernex-runtime** - Core engine: `Runtime::run()` drives the agentic loop, `RuntimeBuilder` wires all subsystems
- **kernex-providers** - AI backends: Claude Code CLI, Anthropic, OpenAI, Ollama, Gemini, OpenRouter, Groq, Mistral, DeepSeek, Fireworks, xAI (11 built-in; AWS Bedrock optional)
- **kernex-core** - Shared types (`Request`, `Response`, `Context`), `HookRunner` trait for tool lifecycle events
- **kernex-memory** - SQLite-backed persistent memory with conversation history and reward-based learning (transitive via kernex-runtime)
- **kernex-skills** - Skill loader for `SKILL.md` files (Skills.sh compatible format)

The `HookRunner` trait lets you intercept tool calls before and after execution (`pre_tool` / `post_tool` / `on_stop`). kx uses this for `--verbose` output and session summaries.

For the full implementation spec (provider resolution, runtime wiring, hook runner, KAIROS scheduler), see [kernex/docs/kernex-agent.md](https://github.com/kernex-dev/kernex/blob/main/docs/kernex-agent.md).

For details on the underlying runtime, see [kernex-dev/kernex](https://github.com/kernex-dev/kernex).

## Extending with Skills

kx can be extended with MCP-based and CLI-based skills from [kernex-dev](https://github.com/kernex-dev/kernex).

Available skills: filesystem, git, playwright, github, postgres, sqlite, brave-search, pdf, webhook.

See [kernex-dev/examples/skills](https://github.com/kernex-dev/kernex/tree/main/examples/skills) for setup.

## Troubleshooting

### "Claude CLI not found"

Ensure Claude Code is installed and in your PATH:

```bash
claude --version
```

If not found, install from [claude.ai/download](https://claude.ai/download).

### "Permission denied: ~/.kx"

Create the directory manually:

```bash
mkdir -p ~/.kx
```

### Database locked

Only one kx session per project can run at a time. Close other sessions or wait for them to complete.

## Security

The runtime executes LLM tool calls (file reads/writes, shell commands, MCP tools, custom toolboxes) inside an OS sandbox: Seatbelt on macOS, Landlock on Linux. Skills installed via `kx skills add` are pinned by SHA-256 in `~/.kx/projects/<name>/skills.toml` and verified on every load; tampering causes the skill to be refused and an audit-log entry written.

If you find a vulnerability, please open a private security advisory at [github.com/kernex-dev/kernex-agent/security/advisories/new](https://github.com/kernex-dev/kernex-agent/security/advisories/new) rather than a public issue. We rotate token-bound auth and ship sandbox fixes as patch releases.

## License

Apache-2.0 OR MIT
