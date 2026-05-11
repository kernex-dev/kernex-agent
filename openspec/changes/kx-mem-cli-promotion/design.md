# Design: kx mem CLI promotion

> **Reference:** [proposal.md](proposal.md), [spec.md](spec.md).
> This document captures architecture decisions (ADRs), the source-tree
> delta, and the public types referenced by the spec.

---

## Architecture overview

Three layers, one store:

```
                        ┌─────────────────────────────┐
                        │   kx (binary, this repo)    │
                        │                             │
                        │  src/cli.rs                 │  ←  clap subcommands
                        │  src/mem/                   │  ←  new module
                        │   ├─ mod.rs                 │
                        │   ├─ cli.rs    handlers     │
                        │   ├─ render.rs json + table │
                        │   └─ types.rs SaveEntry…    │
                        │  src/commands.rs            │  ←  REPL slash commands
                        │                             │     now thin callers
                        └──────────────┬──────────────┘
                                       │  trait calls
                                       ▼
                        ┌─────────────────────────────┐
                        │  kernex-memory (crates.io)  │
                        │  v0.6.1                     │
                        │                             │
                        │  store::Store               │
                        │  trait MemoryStore          │
                        │  schema: deleted_at on facts│
                        │  SaveEntry { what,why,…  }  │
                        └─────────────────────────────┘
                                       │
                                       ▼
                        ~/.kx/projects/<name>/memory.db
                        SQLite + FTS5
```

The CLI module renders; the REPL slash commands render differently;
both call the same trait. The trait lives in `kernex-memory` so a
future HTTP surface (`kx serve /memory/*`) or MCP shim can mount it
without re-implementing storage logic.

---

## Source-tree delta

```
kernex-agent/                            (this repo)
├── src/
│   ├── cli.rs                           [edit] add `Mem(MemArgs)` subcommand
│   ├── commands.rs                      [edit] slash commands delegate to mem::cli
│   └── mem/                             [new] memory subcommand surface
│       ├── mod.rs                       [new] dispatcher; re-exports
│       ├── cli.rs                       [new] subcommand handlers
│       ├── render.rs                    [new] auto-JSON, --compact, --select
│       ├── types.rs                     [new] CLI-side wrappers if needed
│       └── errors.rs                    [new] structured CLI errors + Try: hints
└── tests/
    ├── mem_cli.rs                       [new] integration tests over CLI surface
    └── mem_repl_parity.rs               [new] REPL parity harness
```

The `kernex-memory = "0.6.1"` direct dep was added as a prereq chore
in a separate commit. The trait surface (`MemoryStore` with 18
methods, `SaveEntry` with structured fields, soft-delete on facts) is
already shipped on crates.io. This change is purely the agent-side
surface work.

---

## ADR-001: One trait, three surfaces, one store

### Decision

All memory access converges on the `MemoryStore` trait defined in
`kernex-memory`. This change ships two consumers (REPL, CLI). Future
work adds HTTP (`kx serve /memory/*`) and MCP (`kx mcp`). All four
surfaces depend on the same trait; none re-implement storage logic.

### Rationale

- Single source of truth. A behavior change (e.g. soft-delete filter)
  lives in one place.
- Surface-specific code (rendering, auth, transport) stays in the
  surface module, not in the store.
- Lets us add additive capabilities (insight commands, conflict
  detection, decay scoring) as trait methods later without touching
  the existing surfaces.

### Consequence

Every new memory capability lands at the trait level first, then the
surfaces opt in. We never solve a memory problem inside the CLI module
that should live in the store.

---

## ADR-002: SaveEntry is a struct with named fields, not a content blob

### Decision

`SaveEntry` (as already shipped in `kernex-memory 0.6.1`) is a Rust
struct with first-class fields:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SaveEntry {
    pub project: ProjectId,
    pub r#type: ObservationType,
    pub title: String,
    pub what: Option<String>,
    pub why: Option<String>,
    pub r#where: Option<String>,
    pub learned: Option<String>,
}
```

`ObservationType` is a Rust enum with seven variants:

```rust
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationType {
    Bugfix,
    Decision,
    Pattern,
    Config,
    Discovery,
    Learning,
    Architecture,
}
```

### Rationale

- Named fields are searchable individually via FTS5 column scopes,
  enabling future `kx mem search "title:N+1"` queries without
  re-parsing a content blob.
- The CLI can validate field presence at the clap layer; bad input
  fails before touching the DB.
- JSON wire format is stable and self-documenting.

### Consequence

Free-text titles continue to write only `title`; the optional fields
stay `None` and the row remains valid. No migration of existing rows.

---

## ADR-003: Soft-delete is the default; hard delete is opt-in (later)

### Decision

`kernex-memory 0.6.1` already added `deleted_at TIMESTAMP NULL` to
the `facts` table via migration `017_soft_delete.sql` with the
partial index `idx_facts_active (sender_id, key) WHERE deleted_at IS
NULL`. All read paths in the trait apply the implicit
`WHERE deleted_at IS NULL` filter inside the implementation, not in
callers. `fact_soft_delete()` sets `deleted_at = now`; no row is
removed. Hard delete (`delete_fact`, `delete_facts`) stays
inherent-only on `Store` for emergency cleanup.

### Rationale

- Recovers from agent mistakes without restoring the SQLite file.
- Cheap to ship: one column on the table that needed it most, one
  `WHERE` clause per query.
- Matches production-grade behavior most persistent memory systems
  converge on.

### Consequence

`stats` counts exclude soft-deleted by default. A future
`--include-deleted` flag may be added to specific read commands; out
of scope for this change.

---

## ADR-004: Auto-JSON when stdout is not a TTY

### Decision

Every `kx mem *` subcommand checks `std::io::IsTerminal::is_terminal`
on stdout. When false, output is JSON; ANSI color codes are suppressed
unconditionally; help and error text route to stderr.

### Rationale

- Zero token overhead for shell-driven agents (no need to remember
  `--json` every call).
- Aligns with modern agent-native CLI conventions (gh, jq friendly).
- Operators in a TTY still get a human-friendly table.

### Consequence

Two render paths per command. Both consume the same handler return
type. Tests cover both modes via golden output.

---

## ADR-005: Exit-code taxonomy

### Decision

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | Usage error (unknown flag, malformed arg) |
| 3 | Not found (id, key, project absent) |
| 4 | Authorization or sandbox refusal |
| 5 | Runtime (DB locked, IO failure, schema mismatch) |
| 7 | Rate / capacity (reserved for future provider-backed commands) |

Codes are returned via `std::process::ExitCode` from `main`. Errors
are mapped to codes inside `mem::errors`.

### Rationale

- Agents self-correct without parsing error text.
- Same taxonomy carries forward to `/memory/*` HTTP status mapping in
  any future HTTP surface change.

### Consequence

Adding a new exit code in a later change is breaking. Code 6 is
deliberately reserved for "conflict" (future insight surfaces).

---

## ADR-006: JSON invariants (the wire contract)

### Decision

Every command that emits JSON satisfies:

1. List responses are JSON arrays. Empty list is `[]`, never `null`.
2. Single-record responses are JSON objects.
3. Field names use `snake_case` and match the Rust struct field name
   (via `serde` defaults).
4. Timestamps are ISO 8601 with timezone (`2026-05-09T17:42:11Z`).
5. `--compact` strips to `id`, `type`, `title`, `updated_at`, `score`.
6. `--select fld1,fld2` projects to the named fields. Unknown fields
   produce exit `2`.
7. Errors in JSON mode emit a one-line JSON object on stderr:
   ```json
   {"error":{"code":3,"message":"...","hint":"..."}}
   ```
   Stdout is empty on error.

### Rationale

- Makes the wire format the API. Agents can rely on it.
- `--compact` measured to drop ~60-80% of bytes on representative
  observation payloads.

### Consequence

Adding a non-additive change (renaming a field, removing a field) is
breaking and requires a `kernex-memory` minor bump and paired
`kernex-agent` release.

---

## ADR-007: REPL slash commands become thin callers

### Decision

`src/commands.rs` retains its slash-command parser, but each handler
is reduced to:

```rust
async fn slash_search(&self, args: &str) -> Result<()> {
    let req = mem::cli::SearchRequest::parse_from_args(args)?;
    let records = mem::cli::search(req).await?;
    self.render_for_human(&records); // table / prose
    Ok(())
}
```

The CLI subcommand handler returns the same data; only rendering
differs.

### Rationale

- One place to fix a search bug.
- REPL prose stays human-readable. CLI JSON stays agent-readable. No
  divergence.

### Consequence

A future change that touches search semantics ships once and lights
up both surfaces. The REPL parity harness asserts both renderers
consume the same record set.

---

## ADR-008: Minimal new dependencies

### Decision

This change introduces zero new top-level crate dependencies. JSON
already lives in `serde_json`. Terminal detection uses
`std::io::IsTerminal` (Rust 1.70+). Time formatting reuses `chrono`
already pulled by the runtime. The `kernex-memory = "0.6.1"` direct
dep needed for trait imports was added as a prereq chore in a
separate commit; this change only imports from it.

### Rationale

- Keeps `cargo audit` and `cargo deny check` clean.
- Avoids supply-chain decisions inside a presentation-layer change.

### Consequence

If `--since 30d` parsing demands a duration parser we do not yet have,
implement it in 30 lines locally rather than adding `humantime`.

---

## ADR-009: REPL parity test harness

### Decision

A new test file `tests/mem_repl_parity.rs` runs each slash command
and the equivalent `kx mem *` subcommand against the same store
state and asserts byte-equivalence on the underlying record set
(post-handler, pre-render). Render path differs (table vs JSON); the
data does not.

The harness uses `tempfile::TempDir` for isolated
`~/.kx/projects/_parity_<rand>/` per test, seeds a canonical fixture,
and walks the parity matrix:

| Slash command            | CLI subcommand                  |
|--------------------------|---------------------------------|
| `/search <q>`            | `kx mem search <q>`             |
| `/history`               | `kx mem history`                |
| `/memory`                | `kx mem stats`                  |
| `/facts`                 | `kx mem facts list`             |
| `/facts delete <key>`    | `kx mem facts delete <key>`     |

### Rationale

Without a harness, REPL/CLI divergence only surfaces during manual
operator use. The pre-implementation review explicitly flagged this as
a hole in the original draft. A harness makes parity an enforced
invariant.

### Consequence

Any future change that touches a memory handler must update both
surfaces atomically (single handler returns shared data) or the
parity test fails.

---

## Public types referenced by the spec

These are already shipped in `kernex-memory 0.6.1` and imported by
the agent. Reproduced here for spec clarity.

```rust
pub type ProjectId = String;
pub type MemoryId = i64;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SaveEntry { /* see ADR-002 */ }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub project: ProjectId,
    pub r#type: ObservationType,
    pub title: String,
    pub what: Option<String>,
    pub why: Option<String>,
    pub r#where: Option<String>,
    pub learned: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub score: Option<f32>,        // populated by search; None elsewhere
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Fact {
    pub key: String,
    pub value: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryStats {
    pub project: ProjectId,
    pub observations: u64,
    pub facts: u64,
    pub last_write_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchQuery {
    pub project: ProjectId,
    pub query: String,
    pub limit: usize,                       // default 10
    pub since: Option<chrono::Duration>,    // None means unbounded
    pub r#type: Option<ObservationType>,
}
```

The `MemoryStore` trait has 18 methods (6 conversation/message + 6
fact + 6 task) per the shipped surface in `kernex-memory 0.6.1`. The
subset this change consumes:

```rust
#[async_trait::async_trait]
pub trait MemoryStore: Send + Sync {
    // ...full surface in kernex-memory docs; this change uses:
    async fn store_fact(&self, project: &ProjectId, fact: Fact) -> Result<(), MemoryError>;
    async fn get_fact(&self, project: &ProjectId, key: &str) -> Result<Option<Fact>, MemoryError>;
    async fn list_facts(&self, project: &ProjectId) -> Result<Vec<Fact>, MemoryError>;
    async fn soft_delete_fact(&self, project: &ProjectId, key: &str) -> Result<bool, MemoryError>;
    async fn search_messages(&self, q: SearchQuery) -> Result<Vec<MemoryRecord>, MemoryError>;
    async fn get_memory_stats(&self, project: &ProjectId) -> Result<MemoryStats, MemoryError>;
    // ...plus history/get/save shipped in 0.6.x; see kernex-memory crate docs.
}
```

The trait reads always apply `WHERE deleted_at IS NULL` inside the
trait implementation, not the caller. `MemoryError` carries a kind
that maps onto the exit-code taxonomy.

---

## Migration `017_soft_delete.sql`

Already shipped in `kernex-memory 0.6.1`. Reproduced here for spec
clarity:

```sql
ALTER TABLE facts ADD COLUMN deleted_at TIMESTAMP NULL;

CREATE INDEX IF NOT EXISTS idx_facts_active
    ON facts(sender_id, key) WHERE deleted_at IS NULL;
```

The partial index keeps the common read path (where most rows are
not deleted) cheap. Note that the migration scope landed narrower
than the first SDD draft: only `facts` got `deleted_at`. The
`observations` table did not need a column because (a) it does not
exist by that name in the actual `kernex-memory` schema and (b) the
hard-delete path for messages stays inherent-only on `Store`.

---

## Risks restated for design review

| Risk | Mitigation in this design |
|------|--------------------------|
| Soft-delete read filter forgotten in a new query path | Filter lives inside trait impl in `kernex-memory`; callers cannot bypass it |
| Trait churn between this change and a future HTTP surface | Trait is fixed at 0.6.x; additive methods only in later minors |
| Render divergence between REPL and CLI | Single handler returns data; rendering is per-surface; REPL parity harness pins it |
| New dependency drag | ADR-008 forbids new crates this change |
| JSON shape drift | ADR-006 names the wire contract; tests pin it via golden files |
| `cargo install` users on an older `kernex-memory` | Direct pin to `0.6.1` in `Cargo.toml` guarantees the shipped trait surface |

---

## What this design intentionally does NOT decide

- The export / import / sync chunk format (separate change).
- HTTP route paths and authn for `/memory/*` (separate change).
- MCP tool naming convention (separate change).
- Entry de-duplication via stable keys, conflict detection, decay
  scoring (separate changes, will land once an insight-capabilities
  crate exists).

These decisions wait for real traffic against the wire format
shipped by this change before being designed.
