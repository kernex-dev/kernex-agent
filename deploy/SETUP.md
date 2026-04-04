# kx serve — Setup Guide

Deploy `kx serve` as a headless AI agent API on your own machine or VPS.
No build step required — pull the pre-built image from GHCR.

---

## What You Get

- A running HTTP API that accepts agent jobs
- 16 pre-loaded skills (developer, security, architecture, GEO, UX, reviewer personas)
- 4 pre-loaded workflows (PR review, feature design, security audit, GEO audit)
- Bearer token auth, TLS (VPS path), and structured JSON responses

---

## Pulling the Image from GHCR

The image is published publicly at `ghcr.io/kernex-dev/kernex-agent:latest`. No authentication
is required to pull it. If you receive a 401 or 403, check that:

1. The package visibility is set to **Public** at
   `github.com/orgs/kernex-dev/packages/container/kernex-agent/settings`
2. If your org restricts public package creation, the org owner must enable it at
   `github.com/organizations/<org>/settings/member_privileges`

---

## Prerequisites

- Docker + Docker Compose v2
- An API key from Anthropic, OpenAI, Groq, Mistral, DeepSeek, Fireworks, or xAI
  (recommended for server deployments — see security note below)
- For VPS path: a domain with an A record pointing to your server

> **Security note for server deployments:** The `claude-code` provider uses OAuth credentials
> tied to your personal Claude subscription. These credentials are stored in
> `~/.claude/.credentials.json` and are intended for interactive desktop sessions.
> Mounting them into a server container means a credential leak would expose your personal
> account. For any server or VPS deployment, use an API key provider (`anthropic`, `openai`,
> `groq`, etc.) with a scoped key that can be rotated independently.

---

## Path A: Mac Mini or Local Server (No TLS)

Best for: running kx on your home network or Mac Mini and calling it from your own tools.

### 1. Get the files

```bash
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/docker-compose.local.yml
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/.env.example
cp .env.example .env
```

### 2. Configure .env

Open `.env` and set:

```bash
# Required: strong random token for API auth
KERNEX_AUTH_TOKEN=<output of: openssl rand -hex 32>

# Required if using Claude subscription:
CLAUDE_CREDENTIALS_PATH=/Users/yourname/.claude/.credentials.json

# Required if using an API key instead:
# KERNEX_PROVIDER=anthropic
# ANTHROPIC_API_KEY=sk-ant-...
```

Leave `DOMAIN` and `ACME_EMAIL` blank — not used for local.

### 3. Authenticate Claude (skip if using API key)

```bash
# Install Claude CLI if you haven't already
npm install -g @anthropic-ai/claude-code

# Log in once — saves credentials to ~/.claude/.credentials.json
claude auth login
```

### 4. Start

```bash
docker compose -f docker-compose.local.yml up -d
```

On first start, the container bootstraps the default skills and workflows into
the `kx_data` volume. Subsequent restarts skip this and use the persisted state.

### 5. Verify

```bash
curl http://localhost:8080/health
# {"status":"ok","queued":0,"running":0,"done":0,"failed":0}
```

---

## Path B: VPS with Domain and TLS

Best for: running kx as a persistent API endpoint accessible from anywhere.

> **Note for VPS servers already running Traefik (or another reverse proxy):**
> This compose file includes Caddy, which binds to ports 80 and 443. If those ports are
> already in use, the container will fail to start. Use `docker-compose.traefik.yml` instead,
> which binds kx to a local port only and expects Traefik to route traffic to it.

### 1. Point DNS

Add an A record: `api.yourdomain.com` → your VPS IP.
Wait for propagation before continuing.

### 2. Get the files

```bash
mkdir kx-server && cd kx-server

curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/docker-compose.vps.yml
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/Caddyfile
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/Dockerfile.caddy
curl -O https://raw.githubusercontent.com/kernex-dev/kernex-agent/main/deploy/.env.example
cp .env.example .env
```

### 3. Configure .env

```bash
KERNEX_AUTH_TOKEN=<output of: openssl rand -hex 32>
CLAUDE_CREDENTIALS_PATH=/home/deploy/.claude/.credentials.json
DOMAIN=api.yourdomain.com
ACME_EMAIL=admin@yourdomain.com
```

### 4. Authenticate Claude on the VPS (skip if using API key)

```bash
# On the VPS:
npm install -g @anthropic-ai/claude-code
claude auth login
# Credentials saved to ~/.claude/.credentials.json
```

### 5. Open firewall ports

```bash
ufw allow 80
ufw allow 443
```

### 6. Start

```bash
docker compose -f docker-compose.vps.yml up -d
```

Caddy obtains a TLS certificate automatically on first start.

### 7. Verify

```bash
curl https://api.yourdomain.com/health
# {"status":"ok","queued":0,"running":0,"done":0,"failed":0}
```

---

## Running Your First Job

Replace `<token>` with your `KERNEX_AUTH_TOKEN`.

### One-shot task

```bash
curl -s -X POST https://api.yourdomain.com/run \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Review this Express handler for SQL injection: app.get(\"/user\", (req, res) => { db.query(\"SELECT * FROM users WHERE id = \" + req.query.id) })",
    "skills": ["security-engineer"]
  }' | jq .
```

### Run a workflow

```bash
curl -s -X POST https://api.yourdomain.com/run \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Add user authentication with JWT to our Express API",
    "workflow": "feature-design"
  }' | jq .
```

Response includes a `job_id`. Poll for results:

```bash
curl -s https://api.yourdomain.com/jobs/<job_id> \
  -H "Authorization: Bearer <token>" | jq .
```

Status values: `queued` → `running` → `done` | `failed` | `flagged`

### List all jobs

```bash
curl -s https://api.yourdomain.com/jobs \
  -H "Authorization: Bearer <token>" | jq .
```

---

## Webhooks

Trigger a job from an external system (GitHub Actions, CI pipelines, etc.):

```bash
curl -s -X POST https://api.yourdomain.com/webhook/pr-review \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"message": "PR #42: add user auth endpoint"}'
```

The `Authorization: Bearer` header is always required.

To add an extra HMAC layer (recommended for automated callers), set a per-event secret in `.env`:

```bash
KERNEX_WEBHOOK_SECRET_PR_REVIEW=<strong-random-secret>
```

When set, the caller must include a valid `X-Hub-Signature-256` header (same format as GitHub webhooks).
If the env var is not set, the HMAC check is skipped and bearer auth alone applies.

---

## Available Workflows

| Workflow | What It Does |
|---|---|
| `pr-review` | Code quality + security review + readiness gate |
| `feature-design` | Architecture + implementation plan + API tests |
| `security-audit` | Vulnerability scan + hardened CI pipeline |
| `geo-audit` | GEO score + Schema.org generation + validation |

---

## Available Skills

| Skill | Role |
|---|---|
| `senior-developer` | Code review, implementation planning |
| `backend-architect` | API design, data models |
| `security-engineer` | Vulnerability audits, OWASP analysis |
| `api-tester` | Test case generation |
| `devops-automator` | CI/CD pipelines, Dockerfiles |
| `agents-orchestrator` | Multi-step task coordination |
| `reality-checker` | Validation gate — verifies claims against evidence |
| `uxui-evaluator` | UX/UI audit against 168 principles |
| `interface-auditor` | Antipattern detection, severity scoring |
| `geo-auditor` | GEO readiness scoring, AI crawler audit |
| `geo-schema-generator` | Schema.org JSON-LD generation |
| `workflow-runner` | Structured multi-skill orchestration |
| `enterprise-buyer-reviewer` | Procurement buyer evaluation |
| `developer-dx-reviewer` | Developer experience review |
| `security-skeptic-reviewer` | Security-first engineering review |
| `non-technical-stakeholder` | Plain-language business review |

---

## Workflows vs Pipelines: Steps vs Phases

kx has two distinct multi-agent execution models. They use different TOML constructs and
different CLI entry points.

### Workflows (`[[steps]]`) — HTTP API mode

Used with `kx serve`. Triggered via `POST /run` with `"workflow": "name"`.
Defined in `.toml` files under `/home/kx/.kx/workflows/`.

```toml
name = "pr-review"
description = "..."

[[steps]]
id = "code-review"
skill = "senior-developer"
input = "Review this PR: {input}"
mode = "evaluate"

[[steps]]
id = "readiness-gate"
skill = "reality-checker"
input = "Based on: {code-review.output}"
depends_on = ["code-review"]
```

Key fields: `id`, `skill`, `input`, `mode`, `depends_on` (for ordering and output chaining).
Steps can run in parallel when they share no `depends_on` relationship.

### Pipelines (`[[phases]]`) — CLI mode

Used with `kx pipeline run <topology>`. Defined in `TOPOLOGY.toml` files under
`~/.kx/projects/<project>/topologies/<name>/`.

```toml
[topology]
name = "my-eval"
description = "..."

[[phases]]
name = "scout"
agent = "scout"
phase_type = "standard"
model_tier = "complex"
max_turns = 20

[[phases]]
name = "save"
agent = "save"
phase_type = "standard"
model_tier = "fast"
max_turns = 10
```

Key fields: `name`, `agent`, `phase_type`, `model_tier`, `max_turns`. Phases always run
sequentially. Each phase maps to an agent defined in the same topology directory
(e.g., `scout.md`, `save.md`).

| | Workflows | Pipelines |
|---|---|---|
| Construct | `[[steps]]` | `[[phases]]` |
| Entry point | `POST /run` (kx serve) | `kx pipeline run <topology>` |
| Agent type | skills | named agents (md files) |
| Ordering | DAG via `depends_on` | sequential |
| Output chaining | `{step-id.output}` | agent reads prior output from context |

---

## Customizing Skills and Workflows

### Add or edit a skill

Skills are stored in the `kx_data` Docker volume at `/home/kx/.kx/skills/`.

To add a custom skill:

```bash
# Copy a SKILL.md into the running container
docker cp my-skill.md kx:/home/kx/.kx/skills/my-skill/SKILL.md
```

To mount a local skills directory instead:

1. Stop the container
2. Uncomment the skills bind mount in your compose file
3. Point `source:` at your local directory
4. Restart

### Add a workflow

Same pattern — copy a `.toml` file into `/home/kx/.kx/workflows/`:

```bash
docker cp my-workflow.toml kx:/home/kx/.kx/workflows/
```

---

## Updating

```bash
docker compose -f docker-compose.local.yml pull
docker compose -f docker-compose.local.yml up -d
```

The data volume persists across updates. Skills and workflows are not overwritten
on subsequent starts (the `.initialized` marker prevents re-bootstrapping).

To reset to image defaults:

```bash
docker compose -f docker-compose.local.yml down
docker volume rm kx_data
docker compose -f docker-compose.local.yml up -d
```

---

## Pipeline Save Agents and DATABASE_URL

When running `kx pipeline run <topology>`, the save phase of each pipeline can write results
directly to PostgreSQL using the `psql` CLI. This requires `DATABASE_URL` to be set in the
container's environment.

### Setting DATABASE_URL

Add it to your `.env` file or compose service env:

```bash
DATABASE_URL=postgresql://<user>:<password>@<host>:5432/<dbname>
```

If running `va-kx` on a Docker network alongside PostgreSQL (e.g., `va-postgresql`), the host
is the container name:

```bash
DATABASE_URL=postgresql://visualaudit:<password>@va-postgresql:5432/visualaudit
```

The `postgresql-client` package must be installed in the container for `psql` to be available.
The base `ghcr.io/kernex-dev/kernex-agent` image does not include it. If your save agents use
`psql "$DATABASE_URL" -c "..."`, extend the image:

```dockerfile
FROM ghcr.io/kernex-dev/kernex-agent:latest
USER root
RUN apt-get update && apt-get install -y --no-install-recommends postgresql-client \
    && rm -rf /var/lib/apt/lists/*
USER kx
# Do NOT override ENTRYPOINT or CMD — inherit from base image.
# Clearing ENTRYPOINT will cause Docker to exec the CMD args as a bare binary,
# which breaks kx serve startup.
```

### Save agent example (TOPOLOGY.toml)

```toml
[[phases]]
name = "save"
phase_type = "save"
model_tier = "fast"
max_turns = 5
system_prompt = """
You receive scored leads as JSON. For each lead, insert a row into mission_control.leads
using: psql "$DATABASE_URL" -c "INSERT INTO mission_control.leads ..."
Only use columns that exist in the schema. Store additional fields in the notes JSONB column.
"""
```

### mission_control.leads schema reference

| Column | Type | Notes |
|--------|------|-------|
| `brand` | text | Brand name |
| `company` | text | Business name |
| `contact_email` | text | Extracted email |
| `score` | integer | 0-100 lead score |
| `status` | text | `qualified`, `warm`, `cold` |
| `industry` | text | Business category |
| `source_url` | text | Origin URL |
| `notes` | jsonb | Extra fields: `website`, `city`, `state`, `scorer`, etc. |
| `created_at` | timestamptz | Auto-set |

---

## Provider Options

| Provider | `KERNEX_PROVIDER` value | Env var needed |
|---|---|---|
| Claude subscription | `claude-code` | `CLAUDE_CREDENTIALS_PATH` |
| Anthropic API | `anthropic` | `ANTHROPIC_API_KEY` |
| OpenAI | `openai` | `OPENAI_API_KEY` |
| Groq | `groq` | `GROQ_API_KEY` |
| Mistral | `mistral` | `MISTRAL_API_KEY` |
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` |
| Fireworks | `fireworks` | `FIREWORKS_API_KEY` |
| xAI | `xai` | `XAI_API_KEY` |
| Ollama (local) | `ollama` | none (runs locally) |
| Gemini | `gemini` | `GEMINI_API_KEY` |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` |
