# Spec: kx mem CLI promotion

> **Reference:** [proposal.md](proposal.md), [design.md](design.md).
> Behavioral scenarios use Given / When / Then. Every subcommand is
> covered for happy path plus exit-code matrix.

## Cross-cutting behavior

These rules apply to every `kx mem *` subcommand below. A scenario in a
subcommand section that contradicts a cross-cutting rule is a
specification bug; the cross-cutting rule wins.

### CC-1 Auto-JSON when stdout is not a TTY

- **Given** any `kx mem *` subcommand
- **When** stdout is not a terminal (piped or redirected)
- **Then** the command emits JSON on stdout, with `Content-Type` shape
  defined per command, and never emits ANSI color codes.

### CC-2 `--json` forces JSON

- **Given** any `kx mem *` subcommand
- **When** the `--json` flag is set, regardless of TTY state
- **Then** stdout receives the same JSON shape as CC-1.

### CC-3 `--compact` projects to high-gravity fields

- **Given** a JSON-emitting invocation
- **When** `--compact` is set
- **Then** every record in the response retains only `id`, `type`,
  `title`, `updated_at`, and `score` (where applicable). All other
  fields are omitted (not nulled).

### CC-4 `--select fld1,fld2` projects arbitrary fields

- **Given** a JSON-emitting invocation
- **When** `--select <comma-list>` is set
- **Then** every record retains only the named fields. Unknown field
  names produce exit 2 with a `Try:` hint listing the valid fields for
  that command.

### CC-5 Empty list responses are `[]`

- **Given** a list-returning command (`search`, `history`, `facts list`)
- **When** zero results match
- **Then** stdout is `[]` (a single empty JSON array). Never `null`,
  never absent, never the string `"no results"`.

### CC-6 Errors emit structured stderr when JSON is forced

- **Given** any failing invocation with auto-JSON or `--json` active
- **When** the command exits non-zero
- **Then** stderr receives a single JSON object on one line:
  ```json
  {"error":{"code":<exit-code>,"message":"<one-line>","hint":"<try-line>"}}
  ```
  and stdout is empty.

### CC-7 Exit-code taxonomy

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | Usage error (unknown flag, malformed argument) |
| 3 | Not found (id, key, or required record absent) |
| 4 | Authorization or sandbox refusal |
| 5 | Runtime (IO failure, schema mismatch, JSON serialization) |
| 7 | Transient / retryable (SQLite `database is locked` / `SQLITE_BUSY`, sqlx pool timeout, or future provider rate-limit / capacity exhaustion). Scripts may retry. |

### CC-8 Help text contract

- **Given** any `kx mem *` subcommand
- **When** invoked with `--help`
- **Then** the help output contains:
  - Synopsis line.
  - Flags section (one flag per line, with type and default).
  - At least one usage example.
  - Exit codes section.
  - A `Try:` line at the end with one runnable example.

### CC-9 Soft-deleted rows are invisible by default

- **Given** an observation or fact with `deleted_at IS NOT NULL`
- **When** any read path (`search`, `get`, `history`, `stats`,
  `facts list`, `facts get`) is invoked without an explicit override
- **Then** the deleted row is excluded from results.

> No `--include-deleted` flag ships in this change; the spec only
> guarantees the read filter exists. Recovery tooling is a follow-up.

### CC-10 REPL parity

- **Given** a REPL slash command exists today
  (`/search`, `/history`, `/memory`, `/facts`, `/facts delete <key>`)
- **When** the slash command is invoked from the REPL after this
  change lands
- **Then** the same handler that powers the CLI subcommand produces the
  result, rendered for a human reader (table or prose). No data
  divergence; the only difference is presentation. The parity harness
  asserts byte-equivalence on the underlying record set.

---

## kx mem search

### S-search-1 Happy path returns JSON when piped

- **Given** at least one observation with title `"Fixed N+1 query"`
- **When** the operator runs `kx mem search "N+1" | cat`
- **Then** stdout is a JSON array of observation records, each
  containing at minimum `id`, `type`, `title`, `updated_at`, `score`.
- **And** exit is `0`.

### S-search-2 Limit caps result count

- **Given** ten matching observations
- **When** the operator runs `kx mem search foo --limit 3`
- **Then** the response contains exactly 3 records, ordered by `score`
  descending, then `updated_at` descending.
- **And** exit is `0`.

### S-search-3 `--since` filters by recency

- **Given** observations dated 60 and 10 days ago, both matching the
  query
- **When** the operator runs `kx mem search foo --since 30d`
- **Then** only the 10-day-old record is returned.
- **And** exit is `0`.

### S-search-4 `--type` narrows the taxonomy

- **Given** observations of types `bugfix` and `decision`, both matching
- **When** the operator runs `kx mem search foo --type bugfix`
- **Then** only the `bugfix` record is returned.

### S-search-5 Unknown type is a usage error

- **Given** any store state
- **When** the operator runs `kx mem search foo --type bogus`
- **Then** exit is `2` and stderr includes the list of valid types.

### S-search-6 No matches returns empty array

- **Given** zero matching observations
- **When** the operator runs `kx mem search nonsense | cat`
- **Then** stdout is `[]` and exit is `0` (not 3; "no results" is not
  "not found").

---

## kx mem get

> **Id shape:** message ids are UUIDs (string), not integers. The
> samples below use short placeholders for readability; the real
> surface accepts any string argument and pushes it straight into
> `MemoryStore::get_message_by_id` (added in `kernex-memory 0.7.0`).

### S-get-1 Happy path returns full record

- **Given** an observation with id `MSG-A`
- **When** the operator runs `kx mem get MSG-A --json`
- **Then** stdout is a single JSON object including `id`, `type`,
  `title`, all save-body fields (`what`, `why`, `where`, `learned`),
  `created_at`, `updated_at`.
- **And** exit is `0`.

### S-get-2 Missing id is exit 3

- **When** the operator runs `kx mem get MSG-NOPE`
- **Given** id `MSG-NOPE` does not exist
- **Then** exit is `3`.
- **And** stderr (in JSON mode) contains a `Try:` hint pointing to
  `kx mem search`.

### S-get-3 Soft-deleted record returns 3

- **Given** an observation with id `MSG-A` and `deleted_at` set
- **When** the operator runs `kx mem get MSG-A`
- **Then** exit is `3` (deleted is invisible per CC-9).

---

## kx mem history

### S-history-1 Default returns last N for current project

- **Given** the cwd resolves to project `foo` with 50 observations
- **When** the operator runs `kx mem history`
- **Then** the response is the most recent 20 observations for project
  `foo`, ordered by `updated_at` descending.

### S-history-2 `--last` overrides the default count

- **When** the operator runs `kx mem history --last 5`
- **Then** the response contains exactly 5 records.

### S-history-3 `--project` overrides cwd detection

- **Given** project `bar` exists at `~/.kx/projects/bar/`
- **When** the operator runs `kx mem history --project bar`
- **Then** records returned belong to project `bar`.

### S-history-4 Unknown project is exit 3

- **When** the operator runs `kx mem history --project nope`
- **Given** `~/.kx/projects/nope/` does not exist
- **Then** exit is `3`.

---

## kx mem stats

### S-stats-1 Returns counts and last-write timestamp

- **Given** project `foo` with 12 observations and 4 facts
- **When** the operator runs `kx mem stats --json`
- **Then** stdout is a JSON object with at least
  `{ "project": "foo", "observations": 12, "facts": 4,
     "last_write_at": "<iso8601>" }`.

### S-stats-2 Empty project returns zero counts

- **Given** project `empty` with no observations or facts
- **When** the operator runs `kx mem stats --project empty --json`
- **Then** stdout shows zero counts; `last_write_at` is `null`. Exit is
  `0` (not 3; an empty project is still a valid project).

---

## kx mem facts list

### S-facts-list-1 Returns all facts for current project

- **Given** project `foo` with facts `auth-pattern`, `db-driver`
- **When** the operator runs `kx mem facts list --json`
- **Then** stdout is a JSON array of `{key, value, updated_at}` records.

### S-facts-list-2 Empty project returns empty array

- **Given** project `empty` with zero facts
- **When** the operator runs `kx mem facts list --json`
- **Then** stdout is `[]` and exit is `0`.

---

## kx mem facts get

### S-facts-get-1 Returns the single fact

- **Given** fact `auth-pattern` with value `"OIDC + PKCE"`
- **When** the operator runs `kx mem facts get auth-pattern --json`
- **Then** stdout is `{ "key": "auth-pattern", "value": "OIDC + PKCE",
  "updated_at": "<iso8601>" }`.

### S-facts-get-2 Missing key is exit 3 with hint

- **When** the operator runs `kx mem facts get bogus`
- **Then** exit is `3`.
- **And** stderr (when JSON forced) includes a hint suggesting
  `kx mem facts list`.

---

## kx mem facts add

### S-facts-add-1 Inline value writes a new fact

- **When** the operator runs
  `kx mem facts add auth-pattern "OIDC + PKCE"`
- **Then** the fact is persisted with that value, exit is `0`, and
  stdout (when piped) is the saved record.

### S-facts-add-2 `--stdin` reads value from pipe

- **When** the operator runs
  `printf 'OIDC + PKCE' | kx mem facts add auth-pattern --stdin`
- **Then** the fact is persisted with `OIDC + PKCE` as value.

### S-facts-add-3 Existing key is upsert (update updated_at)

- **Given** fact `auth-pattern` already exists with value `"basic"`
- **When** the operator runs
  `kx mem facts add auth-pattern "OIDC + PKCE"`
- **Then** the value is replaced and `updated_at` advances. No new row.

### S-facts-add-4 Empty value is exit 2

- **When** the operator runs `kx mem facts add auth-pattern ""`
- **Then** exit is `2` and stderr explains that an empty value is not
  permitted (use `kx mem facts delete` to remove the fact).

---

## kx mem facts delete

### S-facts-delete-1 Soft-delete by default

- **Given** fact `auth-pattern` exists
- **When** the operator runs `kx mem facts delete auth-pattern`
- **Then** the fact is soft-deleted (`deleted_at` set), exit is `0`,
  and `kx mem facts list` no longer surfaces it.

### S-facts-delete-2 Missing key is exit 3

- **When** the operator runs `kx mem facts delete bogus`
- **Then** exit is `3`.

### S-facts-delete-3 Already-deleted key is exit 3

- **Given** fact `auth-pattern` already soft-deleted
- **When** the operator runs `kx mem facts delete auth-pattern` again
- **Then** exit is `3` (idempotent absence).

---

## kx mem save

### S-save-1 Inline structured fields

- **When** the operator runs
  ```
  kx mem save --type bugfix "Fixed N+1 query" \
      --what "added eager loading in UserList" \
      --why "lists were 12s slow on 5k users" \
      --where src/users/list.rs \
      --learned "FTS5 query rewriter cannot fix N+1; only the ORM call site can"
  ```
- **Then** a new observation is persisted with `type=bugfix`, the title
  and structured fields recorded, and stdout (piped) returns the saved
  record with its assigned `id`. Exit is `0`.

### S-save-2 `--stdin` accepts SaveEntry JSON

- **When** the operator pipes:
  ```
  echo '{"type":"decision","title":"Adopt rusqlite",
         "what":"chose rusqlite over sqlx",
         "why":"sync API matches our blocking store layer",
         "where":"Cargo.toml",
         "learned":"sqlx async did not pay off for our access pattern"}' \
    | kx mem save --stdin
  ```
- **Then** the entry is persisted with all fields populated and exit
  is `0`.

### S-save-3 Title is required

- **When** the operator runs `kx mem save --type bugfix` (no title)
- **Then** exit is `2`.
- **And** stderr names the missing positional arg.

### S-save-4 Type is required

- **When** the operator runs `kx mem save "fixed something"` (no `--type`)
- **Then** exit is `2`.

### S-save-5 Unknown type is exit 2

- **When** the operator runs `kx mem save --type bogus "x"`
- **Then** exit is `2` and stderr lists the valid types.

### S-save-6 Mixing `--stdin` and inline fields is exit 2

- **When** the operator runs
  `kx mem save --type bugfix --stdin --what "..." "title"`
- **Then** exit is `2`. The two input modes are mutually exclusive.

### S-save-7 Empty title is exit 2

- **When** the operator runs `kx mem save --type bugfix ""`
- **Then** exit is `2`.

### S-save-8 Save respects sandbox refusal

- **Given** the sandbox blocks writes to `~/.kx/projects/`
- **When** the operator runs any `kx mem save ...`
- **Then** exit is `4` and stderr explains the sandbox refusal.

---

## REPL slash-command parity

### S-repl-1 `/search` calls the search handler

- **Given** the operator is in the `kx` REPL
- **When** the operator types `/search auth bug`
- **Then** the same handler that powers `kx mem search` runs; the
  result is rendered for a human reader (formatted table or prose,
  not JSON).

### S-repl-2 `/facts delete <key>` performs soft-delete

- **When** the operator runs `/facts delete auth-pattern`
- **Then** the fact is soft-deleted, identical to
  `kx mem facts delete auth-pattern`.

### S-repl-3 `/memory` is renamed to surface stats

- **Given** legacy slash command `/memory`
- **When** invoked
- **Then** behavior is identical to `kx mem stats` rendered for a
  human reader. The legacy name continues to work for one minor
  release; help text mentions `kx mem stats` as the canonical name.

### S-repl-4 Parity harness byte-equivalence

- **Given** any store state used by the integration suite
- **When** the harness invokes each slash command and the equivalent
  `kx mem *` subcommand against the same store
- **Then** the underlying record set (post-render) is byte-equivalent.
  Render differs (table vs JSON); the data does not.
