<p align="center">
  <img src="favicon.svg" alt="Kernex" width="80">
</p>

# kx

CLI dev assistant powered by [Kernex](https://github.com/kernex-dev/kernex).

## Features

- **12 LLM providers** - Claude Code (default), OpenCode, Anthropic, OpenAI, Gemini, Ollama, OpenRouter, Groq, Mistral, DeepSeek, Fireworks, xAI; AWS Bedrock available behind a Cargo feature
- **Stack detection** - Automatically detects Rust, Node/TypeScript, Python, Flutter/Dart, PHP, Go, Java, and Swift projects
- **Persistent memory** - Remembers decisions, patterns, and context across sessions per project, with FTS5 full-text search
- **Cost telemetry** - `/cost` slash command surfaces cumulative tokens, estimated cost, and prompt-cache hit ratio
- **OS sandbox** - Shell commands and other tool subprocesses run under Seatbelt (macOS) or Landlock (Linux) for filesystem and network isolation; see [Security](#security) for what runs outside it
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
- An LLM provider must be available (default: Claude Code CLI; alternatives use API keys from environment variables).

## Requirements

kx needs at least one LLM provider configured. The default is the Claude Code CLI; any other provider can be selected via `--provider` or `KERNEX_PROVIDER`.

| Dependency | Minimum Version | When required |
|---|---|---|
| **Rust toolchain** | 1.89+ | For `cargo install kernex-agent` (not needed if using Docker) |
| **Claude CLI** | 2.0+ | Only when using the default `claude-code` provider |
| **Docker + Compose v2** | 24+ | Only for the Docker deployment path |

### Provider options

| Provider | Auth | Set with |
|---|---|---|
| `claude-code` (default) | Subscription via local CLI, with its own login (see below) | `claude --version` to verify |
| `opencode` | The API key variable of the model's provider (none for local Ollama) | `--provider opencode --cli-tools all` |
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

#### Logging in the claude-code provider

kx runs the Claude CLI inside its sandbox with a config directory of its
own, `~/.kx/projects/<project>/cli/claude`, so the login you use in a
normal terminal (`~/.claude`) does not carry over. Give kx one of:

- `CLAUDE_CODE_OAUTH_TOKEN`: a long-lived token for your subscription.
  Create it once with `claude setup-token` and export it. Works for every
  project.
- `ANTHROPIC_API_KEY`: bills the API instead of the subscription.
- A one-time login per project:
  `CLAUDE_CONFIG_DIR=~/.kx/projects/<project>/cli/claude claude`, then
  `/login`. `kx doctor` prints the exact directory for the current project.

When the CLI reports that it is not logged in, kx prints these options.

#### How the agent CLIs run their tools

The `claude-code` and `opencode` providers are agent CLIs that run their own
tools. kx starts claude-code with a tool list: by default `Read`, `Glob`,
`Grep`, `Edit`, `Write`, `NotebookEdit` and `TodoWrite`, plus `WebFetch`
and `WebSearch` under `web_fetch = "all"`. The CLI may use those and no
others, and never skips its permission checks. The list has no shell and
names no MCP server. Under `kx serve --tools read-only` the list is
`Read`, `Grep` and `Glob`.

The CLI's sandbox always has the network, because the CLI needs it to
reach its provider. claude-code also gets the macOS keychain and, when you
set it, `CLAUDE_CODE_OAUTH_TOKEN` in its environment. A shell command the
CLI runs shares all of that: it can reach any host and read the login
token. The default list keeps the shell out for that reason.

To give the CLI more, set `cli_tools` in `~/.kx/config.toml` or pass
`--cli-tools` (see [Tool access](#tool-access)). With `all` (and under
`kx serve --tools full`) the CLI runs every tool without asking you, inside
the OS sandbox. A list such as `["Read", "Edit", "Bash(cargo test *)"]`
offers and pre-approves those entries; a `Bash` entry gives the commands
it matches the network and the login token. Each entry is one of the
CLI's tool names, optionally with one pattern in parentheses at its end,
or an MCP rule (`mcp__server__*`). kx refuses a list that is empty or
names an unknown tool or a malformed rule. While `web_fetch` is not
`"all"` it leaves `WebFetch` and `WebSearch` out with a warning, and
refuses the run when the list names nothing else. opencode has no tool
list, so kx runs it only with `all`.

### Agent adapters

`kx install --agent <name> --preset <preset>` wires kernex into a coding assistant's config so it can call kernex's MCP server and share the kernex voice template. The default build ships Claude Code only; the others are opt-in feature flags so the default binary stays under 15 MiB on macOS aarch64.

| Adapter        | Feature flag        | Default? | Docs                                                     |
|----------------|---------------------|----------|----------------------------------------------------------|
| `claude-code`  | `agent-claude`      | yes      | (built-in; see top of this README)                       |
| `codex`        | `agent-codex`       | no       | [docs/adapters/codex.md](docs/adapters/codex.md)         |

Use `cargo install kernex-agent --features agent-codex` (or any combination) to opt into adapters beyond Claude Code.

### Platform Support

| Platform | Status | Notes |
|---|---|---|
| **macOS** (Apple Silicon & Intel) | Fully supported | Sandbox via Seatbelt |
| **Linux** (x86_64, aarch64) | Fully supported | Sandbox via Landlock; tool subprocesses need kernel 6.2+ (Landlock ABI 3), network rules need 6.7+ |
| **Windows** | Experimental | No sandbox support; requires WSL2 for best experience |

The sandbox layer comes from `kernex-sandbox` and is used by the runtime to isolate AI subprocesses. On platforms without sandbox support, kx refuses to spawn agent tool subprocesses instead of running them unsandboxed.

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

### Sessions without memory

`kx --no-memory` neither loads from nor writes to the project's memory:
nothing from earlier sessions reaches the prompt, and nothing from this one
is stored. Since the conversation itself lives in memory, each message is
answered without the earlier ones, and `/clear` has nothing to close. The
REPL also leaves the input history (`history.txt`) alone and does not run
scheduled tasks, which are kept in the same database.

## Serve Mode

`kx serve` runs kx as a headless HTTP daemon. Deploy it on a VPS or Mac Mini to accept agent jobs from external triggers, webhooks, or CI pipelines — no active terminal needed.

### Deploy with Docker (recommended)

No Rust installation required. Pull the pre-built image from GHCR.

**Mac Mini or local server (no TLS):**

```bash
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/docker-compose.local.yml
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/.env.example
cp .env.example .env
# Edit .env: set KERNEX_AUTH_TOKEN and CLAUDE_CODE_OAUTH_TOKEN (from claude setup-token)
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
# Edit .env: set KERNEX_AUTH_TOKEN, CLAUDE_CODE_OAUTH_TOKEN, DOMAIN, ACME_EMAIL
docker compose -f docker-compose.vps.yml up -d
curl https://api.yourdomain.com/health
```

With `KERNEX_PROVIDER=claude-code`, put `CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`) or `ANTHROPIC_API_KEY` in `.env`; the compose files pass `.env` to the container. They no longer mount `~/.claude/.credentials.json`, which kx would not use (see [Logging in the claude-code provider](#logging-in-the-claude-code-provider)).

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
| `--tools` | `read-only` | Tools a job may use: `read-only` (read and search files) or `full` (shell, file writes, `web_fetch`, MCP). Also `KX_SERVE_TOOLS` |
| `--auth-token` | (required) | Bearer token for API authentication |
| `--workers` | `4` | Max concurrent agent jobs |

Provider flags (`--provider`, `--model`, `--api-key`, etc.) work the same as in interactive mode and set the default for all jobs. So do `--web-fetch` and `--read-path` (see [Tool access](#tool-access)), with two differences: serve reads `~/.kx/config.toml` but no project `.kx.toml`, and `web_fetch` stays off under `--tools read-only`. `--cli-tools` and `cli_tools` do not apply: a job's tools come from `--tools`, and serve refuses `--cli-tools`.

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
model = "claude-sonnet-4-6"          # Model to use
max_turns = 10                       # Max agentic turns per request
timeout_secs = 300                   # Request timeout in seconds
```

### Tool access

The agent's tools work inside the project directory. Three settings
widen that on purpose:

| Setting | Flag | Config key | Default |
|---|---|---|---|
| Hosts the `web_fetch` tool may reach | `--web-fetch <host>` (repeatable or comma-separated; `all` for any public host, `none` to turn it off; env `KX_WEB_FETCH`) | `web_fetch = ["docs.rs", "*.crates.io"]` or `web_fetch = "all"` | off |
| Extra directories the file tools may read | `--read-path <dir>` (repeatable) | `read_paths = ["/opt/shared-docs", "~/notes"]` | none |
| Tools the `claude-code` and `opencode` CLIs may use | `--cli-tools <tool>` (comma-separated, commas inside parentheses kept; `all` or `default`; env `KX_CLI_TOOLS`) | `cli_tools = ["Read", "Edit", "Bash(cargo test *)"]` or `cli_tools = "all"` | read, search and edit files; no shell |

These keys belong in `~/.kx/config.toml`, which applies to every project and
which no repository can change. For `web_fetch` and `cli_tools` the flag
replaces the config value; `read_paths` from the config and the flags add
up. Relative paths are
taken from your home directory in the config and from the current directory
on the command line.

A project's `.kx.toml` cannot widen any of them, because that file
arrives with the repository. The one key it may set is `web_fetch = "none"`,
which turns the tool off for that project even when your config turns it
on. Any other `web_fetch` or `read_paths` in `.kx.toml` is ignored with a
warning.

The `claude-code` and `opencode` providers bring their own WebFetch and
WebSearch tools, which cannot be held to a host list: they get them only
with `--web-fetch all` (or `web_fetch = "all"`). When `cli_tools` gives
them a shell, it has the network whatever `web_fetch` says (see
[How the agent CLIs run their tools](#how-the-agent-clis-run-their-tools)).

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
kx skills add anthropics/skills/skills/frontend-design

# Install with specific trust level
kx skills add vercel-labs/agent-skills/skills/web-design-guidelines --trust standard

# List installed skills
kx skills list

# Verify integrity (SHA-256)
kx skills verify

# Remove a skill
kx skills remove frontend-design
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
- **Prompt guardrails.** The system prompt lists each installed skill with its trust level and denied permissions, and tells the AI to treat skills as untrusted third-party content. The AI reads a skill's `SKILL.md` with its read tool when it needs it, so 21 builtins cost about 400 tokens per turn instead of about 27,000. Where the model has no read tool, the full instructions go into the prompt instead.
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
- Input history (readline; not read or written under `--no-memory`)
- The Claude CLI's login and settings for this project (`cli/claude`)

User-wide settings live in `~/.kx/config.toml` (see [Tool access](#tool-access)).

## Providers

kx defaults to Claude Code CLI as its AI backend. The underlying `kernex-providers` crate supports additional backends:

| Flag | Provider | Requires |
|------|----------|---------|
| `--provider claude-code` | Claude Code CLI (default) | Claude CLI installed |
| `--provider opencode` | OpenCode CLI | OpenCode CLI installed and configured |
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

`claude-code` runs with a tool list that has no shell unless you widen it, and `opencode` runs only with `--cli-tools all`; see [How the agent CLIs run their tools](#how-the-agent-clis-run-their-tools).

**AWS Bedrock** is available as an optional compile-time feature. Build with `--features bedrock` to enable `--provider bedrock`. Requires standard AWS credentials (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`). Bedrock is not included in the 12 built-in providers above.

Without `--provider` or `KERNEX_PROVIDER`, kx uses `claude-code`. Override the model with `--model <name>`.

You can also set defaults via environment variables:

```bash
export KERNEX_PROVIDER=anthropic
export KERNEX_MODEL=claude-opus-4-8
```

## Architecture

kx is a thin CLI wrapper around the Kernex runtime:

- **kernex-runtime** - Core engine: `Runtime::run()` drives the agentic loop, `RuntimeBuilder` wires all subsystems
- **kernex-providers** - AI backends: Claude Code CLI, OpenCode CLI, Anthropic, OpenAI, Ollama, Gemini, OpenRouter, Groq, Mistral, DeepSeek, Fireworks, xAI (12 built-in; AWS Bedrock optional)
- **kernex-core** - Shared types (`Request`, `Response`, `Context`), `HookRunner` trait for tool lifecycle events
- **kernex-memory** - SQLite-backed persistent memory with conversation history and reward-based learning (transitive via kernex-runtime)
- **kernex-skills** - Skill loader for `SKILL.md` files (Skills.sh compatible format)

The `HookRunner` trait lets you intercept tool calls before and after execution (`pre_tool` / `post_tool` / `on_stop`). kx uses this for `--verbose` output and session summaries.

Provider resolution, runtime wiring and the hook runner live in [`src/runtime_glue.rs`](src/runtime_glue.rs); the scheduler is in [`src/scheduler.rs`](src/scheduler.rs).

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

### "Invalid API key" or "Not logged in" with claude-code

The CLI is installed but has no login in kx's config directory. See
[Logging in the claude-code provider](#logging-in-the-claude-code-provider);
`kx doctor` shows which login kx will use.

### "Permission denied: ~/.kx"

Create the directory manually:

```bash
mkdir -p ~/.kx
```

### Database locked

Only one kx session per project can run at a time. Close other sessions or wait for them to complete.

## Security

Tool subprocesses (shell commands, custom toolboxes, MCP servers, the `claude` and `opencode` CLIs) run inside an OS sandbox: Seatbelt on macOS, Landlock on Linux. They can write only in the project directory and the temp and toolchain cache directories, cannot read credential stores (`~/.ssh`, `~/.aws`, `~/.npmrc`, `~/.claude` and similar), shell startup files, the memory database, `~/.kx/config.toml` or any other project's data directory under `~/.kx/projects/`. Shell commands, toolboxes and MCP servers have no network unless a tool declares it. The `claude` and `opencode` CLIs always have the network, and so does every command they run; claude-code also gets the macOS keychain and its login token. By default claude-code is given no shell for that reason (see [How the agent CLIs run their tools](#how-the-agent-clis-run-their-tools)).

The `read`, `write`, `edit`, `grep` and `glob` tools run inside the kx process and are held to the same places: writes in the project directory, reads in the project directory, the installed skills and any `--read-path` / `read_paths` directory (set in `~/.kx/config.toml`; a project's `.kx.toml` cannot add one). `web_fetch` is off unless you turn it on (see [Tool access](#tool-access)); that setting covers kx's own tool, not the network the agent CLIs already have.

A session that reads untrusted content (a web page, a file from someone else) can still be steered within those limits. With `web_fetch` on, it can send what it can read to an allowed host. With an agent CLI that has a shell (`cli_tools = "all"`, a `Bash` entry, or opencode) it can send it anywhere, whatever `web_fetch` says.

Skills installed via `kx skills add` are pinned by SHA-256 in `~/.kx/projects/<name>/skills.toml`. A skill may declare an MCP server or a toolbox only when installed with `--trust trusted`, and kx refuses to start while a skill that declares one is untrusted, unlisted, or changed since install. The runtime starts a skill's MCP server only when the skill lists the server's command in `permissions.commands`, and gives it the network only when the skill declares `permissions.network`; a server started with `npx -y` needs both.

A project's `.kx.toml` cannot choose the provider or its endpoint: `provider.name` and `provider.base_url` in that file are ignored with a warning. Set them with `--provider` / `KERNEX_PROVIDER` and `--base-url` / `KERNEX_BASE_URL`.

If you find a vulnerability, please open a private security advisory at [github.com/kernex-dev/kernex-agent/security/advisories/new](https://github.com/kernex-dev/kernex-agent/security/advisories/new) rather than a public issue. We rotate token-bound auth and ship sandbox fixes as patch releases.

## License

Apache-2.0 OR MIT
