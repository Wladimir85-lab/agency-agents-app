# Build Contract — Agency Agents

> The canonical interface spec. Every agent/teammate building a module codes against
> THIS. Types are the wire format between Rust (`serde`) and the Svelte frontend (`src/lib/types.ts`).
> Keep Rust `#[serde(rename_all = "camelCase")]` so TS sees camelCase.

## A. Core types (Rust `types.rs` ↔ TS `types.ts`)

```rust
// An agent as parsed from the corpus.
struct Agent {
    slug: String,            // filename without .md, e.g. "frontend-developer"
    name: String,            // frontmatter `name`
    description: String,     // frontmatter `description`
    category: String,        // parent dir, e.g. "engineering"
    emoji: Option<String>,   // frontmatter `emoji`
    color: Option<String>,   // frontmatter `color` (named or hex)
    vibe: Option<String>,    // frontmatter `vibe`
    body: String,            // markdown body (persona) — lazy/optional in list views
}

struct CorpusEntry {         // one row of corpus-index.json
    slug: String, name: String, category: String,
    emoji: Option<String>, color: Option<String>, vibe: Option<String>,
    description: String,
    source_hash: String,        // sha256 of full canonical .md
    frontmatter_hash: String,   // sha256 of frontmatter block
    body_hash: String,          // sha256 of body
}

struct CorpusMeta { version: String, commit: Option<String>, fetched_at: String, count: u32 }

enum Tool { ClaudeCode, Copilot, Cursor, GeminiCli, Codex, Opencode,
            Windsurf, Aider, Qwen, Openclaw, Antigravity }

enum Scope { User, Project }

struct InstallRecord {       // one row of installs.json (the ledger)
    slug: String, tool: Tool, scope: Scope,
    project_path: Option<String>,
    dest: String,
    source_hash: String, rendered_hash: String,
    installed_at: String, corpus_version: String,
}

enum InstallState { Current, Outdated, Modified, Removed, Foreign }

struct InstalledAgent {      // reconciled view-model for Library
    slug: String, name: String, tool: Tool, scope: Scope,
    project_path: Option<String>, dest: String,
    state: InstallState,
    update_kind: Option<UpdateKind>,   // Some(Cosmetic|Substantive) when Outdated
}

enum UpdateKind { Cosmetic, Substantive }   // body_hash unchanged => Cosmetic

struct ToolInfo {            // for the Tools section
    tool: Tool, label: String, detected: bool, scope: Scope,
    user_dest: Option<String>, installed_count: u32,
}
```

## B. Install-target matrix (authoritative — source: agency-agents `scripts/install.sh`)

| Tool | enum | scope | dest | format/notes |
|---|---|---|---|---|
| claude-code | ClaudeCode | user | `~/.claude/agents/<slug>.md` | md, frontmatter as-is |
| copilot | Copilot | user | `~/.github/agents/<slug>.md` **and** `~/.copilot/agents/<slug>.md` | md (dual write) |
| gemini-cli | GeminiCli | user | `~/.gemini/agents/<slug>.md` | md |
| codex | Codex | user | `~/.codex/agents/<slug>.toml` | **TOML** (convert frontmatter→toml) |
| qwen | Qwen | user/project | `~/.qwen/agents/` or `.qwen/agents/<slug>.md` | md |
| openclaw | Openclaw | user | `~/.openclaw/agency-agents/<slug>.md` | md |
| antigravity | Antigravity | user | `~/.gemini/antigravity/skills/<slug>/` | skill dir |
| cursor | Cursor | project | `.cursor/rules/<slug>.mdc` | **.mdc** (color→hex) |
| opencode | Opencode | project | `.opencode/agents/<slug>.md` | md (color→hex) |
| windsurf | Windsurf | project | `.windsurfrules` (append/managed block) | rules text |
| aider | Aider | project | `CONVENTIONS.md` | conventions text |

> Per-tool exact conversion rules live in agency-agents `scripts/convert.sh` — port them
> verbatim into `render/<tool>.rs`. Each `render` MUST be deterministic & idempotent.

## C. Tauri commands (the invoke surface — register in `lib.rs`)

```
// corpus
corpus_status() -> CorpusMeta
corpus_refresh() -> CorpusMeta            // fetch tarball, reindex (streaming)
corpus_list(category?: String) -> Vec<Agent>      // list view (body omitted)
corpus_get(slug: String) -> Agent                 // full incl. body
corpus_categories() -> Vec<Category>

// install / reconcile
install_agent(slug, tool, project_path?) -> InstallRecord   // streaming
uninstall_agent(slug, tool, project_path?) -> ()
installs_reconcile() -> Vec<InstalledAgent>       // the 5-state scan (our brew list)
installs_for_agent(slug) -> Vec<InstalledAgent>
adopt_agent(slug, tool, project_path?) -> InstallRecord
update_agent(slug, tool, project_path?) -> InstallRecord    // outdated -> current

// tools / projects
tools_list() -> Vec<ToolInfo>
projects_list() -> Vec<ProjectInfo>
projects_add(path: String) -> ProjectInfo

// loadouts (Agentfile)
loadout_export(path?) -> String           // serialize current installs
loadout_import(file: String) -> Vec<InstallRecord>   // restore (streaming)

// trending / quality / github  (Phase 4–5; mirror brew-browser equivalents)
trending_fetch(window) -> ...             // New & Updated / Popular from git history
quality_scan_all() / quality_scan_one(slug)        // opt-in lint+originality
github_repo_stats() / github_star() / github_status() / github_create_issue(...)

// inherited verbatim from brew-browser: settings_*, update_*, app_version, cancel_job, open_in_finder
```

## D. Frontend stores (Svelte 5 runes, `src/lib/stores/*.svelte.ts`)

Replace brew stores with: `corpus`, `library` (reconciled installs), `tools`,
`projects`, `loadouts`, `trending`, `quality`, plus inherited `settings`, `activity`,
`github`, `ui`, `toast`, `updater`. Keep the `$state`/`$derived` rune patterns from
brew-browser's stores verbatim.

## E. Determinism rules (non-negotiable)

1. Renderers are pure functions of `(Agent, Tool)` — no timestamps, no random, stable key order.
2. Hash = SHA-256 lowercase hex of UTF-8 bytes.
3. Frontmatter parsing tolerates the agency-agents format (YAML between `---` fences).
4. Never write into an agent file we'd classify `Modified` without explicit user confirm.

## F. IntentOS Runtime v0.1 contract

### Command surface

```text
runtime_providers() -> RuntimeProvider[]
runtime_start(StartRunRequest, Channel<RunEvent>) -> RunSummary
runtime_cancel(run_id: UUID) -> ()
runtime_get(run_id: UUID) -> RunSummary
runtime_list(project_path?: String) -> RunSummary[]
```

`StartRunRequest` requires a non-empty intent, registered provider `codexCli`,
canonical non-root project directory, runbook id, capability id, exactly five
non-empty stage labels and exactly five non-empty catalog agent slugs.

### State contract

- Run: `queued | running | succeeded | failed | cancelled`.
- Stage: `pending | running | passed | failed | cancelled`.
- Stage ids are fixed: `project-management`, `ux-architecture`, `development`,
  `qa`, `reality-check`.
- Rust is the source of truth. The Svelte store is only a live projection.
- Evidence is persisted under app data `state/runs/<uuid>.json` by atomic write.

### Gate contract

The Codex process must exit successfully and the last explicit marker in its output
must be `INTENTOS_GATE:PASS`. A later `INTENTOS_GATE:FAIL`, no marker, a non-zero
exit or a 30-minute timeout fails the stage. QA may trigger at most two Development
remediation passes before its third and final attempt.

### Explicitly excluded from v0.1

Human approval gates, parallel stages, multiple model providers, NeMo/Switchyard,
cost budgets, automatic commits, transactional rollback, deployment and physical
IoT certification.
