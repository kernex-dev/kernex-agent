# kx serve — Setup Guide

Deploy `kx serve` as a headless AI agent API on your own machine or VPS.
No build step required — pull the pre-built image from GHCR.

---

## What You Get

- A running HTTP API that accepts agent jobs
- 13 pre-loaded skills (developer, security, architecture, GEO, UX)
- 4 pre-loaded workflows (PR review, feature design, security audit, GEO audit)
- Bearer token auth, TLS (VPS path), and structured JSON responses

---

## Prerequisites

- Docker + Docker Compose v2
- A Claude subscription (for `claude-code` provider)
  OR an API key from Anthropic, OpenAI, Groq, Mistral, DeepSeek, Fireworks, or xAI
- For VPS path: a domain with an A record pointing to your server

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
    "input": "Review this Express handler for SQL injection: app.get(\"/user\", (req, res) => { db.query(\"SELECT * FROM users WHERE id = \" + req.query.id) })",
    "skills": ["security-engineer"]
  }' | jq .
```

### Run a workflow

```bash
curl -s -X POST https://api.yourdomain.com/run \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "input": "Add user authentication with JWT to our Express API",
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
