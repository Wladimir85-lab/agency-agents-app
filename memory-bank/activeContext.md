# Active Context — Agency Agents

## Cognitive-comprehension architecture for Esmeralda's conversational intent — 2026-09-05

**Read this before touching `sendTurn`/`isConversationalMessage`-adjacent code.** Three
same-day live bugs (generic-question false positive, plain "sí" not matching a `\b`-wrapped
regex, "no construyas nada todavía" trapping the chat in an infinite build-confirmation loop)
were all the same failure mode: a flat regex classifier enumerating Spanish grammar instead of
understanding it, checked in a fragile if/else order inside `Runbooks.svelte`'s `sendTurn`.
Wladimir explicitly rejected shipping a third regex patch (already built, tested, live-verified)
and issued a full mandate for genuine comprehension, plus a follow-up requiring the architecture
to stay domain-neutral (not hardwired to "software builds") without overclaiming AGI.

**What changed** (`intentosCapabilities.ts`): `ConversationalIntent` (`act: "execute" | "explain"
| "plan" | "smalltalk" | "confirm" | "cancel" | "unclear"`, `negated`, `restrictions`,
`confidence`, `reason`) is now the single comprehension output. `classifyConversationalIntent` is
hybrid — a semantic layer (`classifyConversationalIntentSemantic`, same DeepSeek/
`local_model_complete` primitive as the existing capability classifier) genuinely interprets
negation/hypothesis/confirmation/restrictions given conversation context and any pending
proposal; on failure it falls back to `classifyConversationalIntentDeterministic`, which is the
*old* regex classifier (`BUILD_INTENT_PATTERN` etc. — all six patterns, unchanged) reframed as
acts instead of ad hoc booleans, not deleted. `act: "execute"` (not "build") is the deliberately
domain-neutral verdict "authorize a real effect now" — today that only ever means the 5-stage
software pipeline, but the vocabulary itself doesn't assume that, so a future non-software action
plugs in without redesigning this layer. `PendingBuildProposal { text, restrictions }` replaces
the old plain-string `pendingBuildText`, so explicit conditions ("pero no implementes nada
todavía") survive into the eventual build intent instead of being dropped.

`Runbooks.svelte`'s `sendTurn` is now one `classifyConversationalIntent` call feeding a small,
fixed decision table — the only place authority lives, unchanged in spirit from before this
mandate, just fed a structured signal instead of raw regex booleans.

**Live-verified against the real running app** (CDP): replayed the exact reported conversation
(hypothetical team-planning opener, three refusal phrasings, the team-composition follow-up, and
the hardest adversarial phrase "Sí, pero solo explícame.") — zero confirmation-loop, zero
unwanted build, the hardest phrase correctly explained instead of asking to confirm.

**Deliberately not built**: a multi-domain tool-selection engine, real "research" capabilities, or
an action-type plugin system — none exist in the runtime today; only the comprehension vocabulary
was made domain-neutral. `vitest` 67 passed (up from 52), `npm run check` 495/0/0. Full detail:
`agentLog.md` 2026-09-05, `decisions.md`'s new ADR.

## Runtime engineering audit + JOB-level operational responsibility — 2026-09-04

**Read this before every section below** — it supersedes the "Architecture blocked" framing
of the 2026-09-03 entry (Architecture now completes; see the foreground-execution fix below)
and closes out the "what's left" items from several earlier sessions with real, tested,
committed code, not just analysis.

**Earlier the same day, before this documented session** (commits `299308a`..`68be1d8`, not
narrated here in detail — read those commits directly if you need the why): hybrid intent
routing (keyword fast path + semantic fallback via DeepSeek after Groq was tried and reverted),
`creativeTechnologyJustified` prompt tightened against false positives, an explicit provider
now allowed to run direction/architecture/development (not just qa/reality), Claude Code
preferred over Codex when both probe available, stage agents told to run blocking commands in
the foreground (`stage_prompt`'s "single non-interactive turn" paragraph — this is what
actually unblocked Architecture), and two dangling agent refs + three dormant corpus personas
repaired/connected.

**This session's mandate**: Wladimir asked for a from-first-principles audit of the runtime
against a formal "Constitución de Trabajo Profesional Asistido por IA" (not written into this
repo — it's a standing instruction to Claude across sessions) plus an operational-autonomy
addendum whose authoritative hierarchy is **LLM RESPONSE ≠ AGENT TURN ≠ STAGE ≠ TASK ≠ JOB
COMPLETED**. Real incidents motivated it: a background `npm install` left pending after a
one-shot Claude Code turn ended with no future turn to resume it; a suspicion that OpenAI had
silently replaced Claude as executor; ambiguous run states with no deliberate terminal
semantics. Methodology: Fase 0 (baseline, 455 tests clean) → Fase 1 (full pipeline map,
UI→intent→routing→executor→stages→QA→persistence, each piece classified EXISTS-WORKS / WEAK /
DISCONNECTED / MISSING, against all 10 of Wladimir's named requirements) → Fase 2 gap analysis
(P0-P3, plus a sourced comparative study of Devin/OpenHands/SWE-agent's job-completion
mechanisms — principles extracted, nothing adopted as a dependency) → Fase 3 implementation.
All 10 requirements closed this session; six real commits landed on `main`:

1. **`4791aa6` — executor↔runtime contract (P0)**: for external-CLI (Claude Code/Codex)
   stages, a claimed `INTENTOS_GATE:PASS` is now checked against a real before/after workspace-
   manifest diff (`StageCompletionOutcome`/`StageCompletionDiagnosis` in `runtime.rs`) before
   being trusted — evidence can only ever downgrade a false-positive PASS, never manufacture one
   out of a genuine FAIL. A deferred-continuation text heuristic is advisory-only (never gates on
   its own, so a legitimate long-lived background process like a dev-server preview can't cause
   a false FAIL). Real E2E against the live Claude Code CLI (wrote a real file, real PASS, real
   evidence). **Documented, not fixed**: IntentOS still cannot see grandchild processes an
   external CLI spawns internally — a Windows Job Object would close this, scoped as a real
   follow-up, not attempted this cycle (unsafe FFI, stability risk not justified yet).
2. **`cc5dbfd` — observability (P1.1)**: root-caused and closed the "did OpenAI replace Claude"
   suspicion — it never did. Groq's open-weight model is literally named `openai/gpt-oss-120b`
   (OpenAI publishes it open-weight; Groq serves it; unrelated to OpenAI's own paid API), and a
   real 429 from Groq's free-tier rate limit reads exactly like "falló por cuota." Added a
   caller-declared `purpose` tag on `local_model_complete` (the classifier now tags itself
   `capability_classifier`) and a "stage executor starting" log line in
   `run_claude_stage`/`run_codex_stage` — a classifier call and a stage-executor run can no
   longer be conflated even by someone just grepping logs.
3. **`8204421` — MODEL CLAIM vs OBJECTIVE EVIDENCE (P1.2)**: `GateMarker` (Pass/Fail/**Missing**
   — a process ending with no verdict at all is now distinguishable from an explicit FAIL) and
   `ObjectiveEvidence` (currently just `workspace_changed`, deliberately not more — build/test-
   exit-code/Playwright checks are a real, named extension point for later, not stubbed as an
   always-`None` field) persisted alongside the final diagnosis in
   `{stage}-{attempt}.completion.json`.
4. **`f90bf29` — JOB-level operational responsibility**: the JOB-level instance of the
   LLM-response≠...≠job-completed hierarchy. `JobState` (`ResultVerified` /
   `HumanDecisionRequired(reason)` / `BlockedWithEvidence(reason)` — deliberately no fourth
   "Active" variant; being active is simply `RunSummary.terminal_state == None`) on
   `RunSummary.terminal_state`. `normalize_terminal_state` (pre-existing, only did cosmetic
   stage-status cleanup before) now also reconciles a persisted `Queued`/`Running` run against
   `AppState.runtime_jobs` (the live-task table) and reclassifies it to `BlockedWithEvidence` if
   the process that was actually executing it is confirmed dead — **the concrete fix for "a run
   stays Running forever in disk state after the app that was running it crashed."**
   `AppError::CapabilityProviderUnavailable` (already existed, already documented, never
   connected to anything) now correctly classifies as `HumanDecisionRequired` instead of a
   generic failure via a new `classify_app_error`. Two real E2E tests: a crashed-job disk-
   reconciliation run (persists a `Running` run with no live task, reads it back, independently
   re-reads the raw file to prove the correction landed on disk) and a real Claude Code job
   carried all the way to a persisted `ResultVerified`.
5. **`be94bc0` — translation-principle encoded in `fabric.rs`**: a second constitutional
   addendum ("IntentOS translates human intention into verified computational results; code/
   models/tools are replaceable mechanisms, not the system's identity — not fundamentally a code
   generator") is now real, tested data in `fabric.rs`'s existing `FabricStatus.principle` string
   plus a new ordered `translation_pipeline` field, not just a comment — a test fails if
   `principle` ever again reduces to "generates code." No new capability catalog, RAG, or plugin
   system — explicitly out of scope for this addendum.
6. **`89a1ed2` — Requisito 6 closure**: `terminalState` now actually crosses the Tauri IPC
   boundary into `types.ts` and the frontend. Before this, the backend had the right semantics
   but a `HumanDecisionRequired` run rendered identically to a technically-broken one — same
   generic red "failed" text either way, which is operational autonomy without an unambiguous
   human signal, i.e. not real soberanía humana. `summarizeRunForEsmeralda` (the one function
   that writes Esmeralda's chat message) and `Runbooks.svelte`'s run-error paragraph now
   distinguish it (warning-toned, "no es una falla técnica, necesito tu decisión") without any
   new UI system — reused the existing `--color-warning` token already used elsewhere in the app.
   `vitest.config.ts` gained the `sveltekit()` plugin (first test to need Svelte-rune/`$lib`
   resolution — anticipated by that file's own prior comment). Real E2E: a Rust test builds the
   exact production `AppError::CapabilityProviderUnavailable`, persists it, and asserts the
   literal JSON shape; the frontend test parses that *exact* captured string, so backend and
   frontend are proven to agree on the wire shape, not just assumed to.

**Surprising finding worth remembering**: Esmeralda's chat "voice" is 100% deterministic
templated Spanish text (`summarizeRunForEsmeralda`) over real `RunSummary` data — there is
currently no LLM call anywhere that generates what she "says." Not a bug; just don't assume
more conversational intelligence exists there than actually does when reasoning about future
work in this area.

**Verified**: `cargo test --lib` 476 passed / 0 failed / 17 ignored (was 455/0/15 at this
session's start). `npm run check` 496 files / 0 errors. `vitest` 26 passed (was 21). Every real
E2E in this list was actually executed against the live Claude Code CLI or real disk I/O, not
only asserted as unit tests.

**Deliberately not done this session** (explicit scope decisions, not oversights — don't
"discover" these as gaps and rebuild them without checking this note first): Learning Points as
a visible feature, generalized HITL beyond Mission approval, semantic (non-exact-text) resume
matching, any NeMo/OpenHands/SWE-agent integration as a dependency, capability-catalog
expansion, new UI panels beyond the minimal Requisito 6 fix, a large Esmeralda refactor, and
`KnowledgeCandidate`/P1.3 knowledge capitalization (designed conceptually in this session's
discussion, never built).

**Same-day follow-up — `7640edd`, real chat with Esmeralda**: after the six commits above,
Wladimir pointed out the one place Esmeralda actually "talks" — the chat panel in
`Runbooks.svelte` — sent every message, no matter how trivial, straight into
`prepareProposal()` → `mission_create` → a full 5-stage `runs.start()`. Saying "hola esmeralda"
would try to classify a build capability for it. Fix, reusing the same completion primitive
already wired for the post-run report (nothing new invented):
`isConversationalMessage(text)` in `intentosCapabilities.ts` (same hybrid-router spirit as
`routeIntent` — a Spanish action-verb pattern or a real `routeIntent` keyword match means
"build"; otherwise "chat", deliberately asymmetric toward chat since a false "build" verdict
wastes a real Mission+run on a greeting) gates `Runbooks.svelte`'s `sendTurn`: a conversational
message short-circuits to `session.appendMessage` + the new `replyToEsmeraldaChat` (session.
svelte.ts, same `local_model_complete` call and never-silent-fallback discipline as
`narrateRunForEsmeralda`) and never reaches the proposal/Mission/run machinery at all. Gated on
`projectPath` already existing — a project's first-ever message keeps today's behavior.

**Live-verified in the real running app, not simulated**: relaunched `npm run tauri dev` with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, connected Playwright via
`chromium.connectOverCDP` to the actual WebView2 window, opened the real
`quiero-un-sitio-web-para-almarnaval-un-astillero-empre` (ALMARNAVAL) project's existing
conversation, and sent "Hola Esmeralda, ¿cómo estás? ¿En qué proyecto estamos trabajando?".
No new run started (composer stayed the same, no stage list appeared). Esmeralda's real,
model-generated reply, captured verbatim from the DOM: *"¡Hola! Estoy muy bien, gracias.
Estamos trabajando en el sitio web premium para ALMARNAVAL, con ese estilo elegante y
minimalista que me indicaste. ¿Hay algo que quieras comentar o afinar antes de seguir?"* —
correctly grounded in the real project name and the real style brief from the conversation
history, not invented. `vitest` 36 passed (up from 26), `npm run check` 496/0, `cargo test --lib`
unaffected (476/0/17, no Rust touched).

## Ollama backend + evidence-driven guards — Direction reliable, Architecture blocked — 2026-09-03

**Read this before the Groq section below** — it's the same session, continued, and
supersedes that section's "what's left" framing in one important way: **Direction now
passes reliably with a free, fully local model** (`qwen2.5-coder:3b` via Ollama), 5/5 live
runs. This was not true when the Groq section below was written.

**What changed**: two deterministic guards added to `local_agent.rs`'s turn loop —
(1) exact-duplicate-write detection (`is_redundant_write`), and (2) a broader
contract-satisfied circuit breaker: once `validate_stage_contract` reports nothing missing
for two consecutive turns (write or redundant read alike), IntentOS concludes the stage on
its **own** contract verification instead of waiting for the model to say
`INTENTOS_GATE:PASS` — see `CONTRACT_SATISFIED_AUTO_CONCLUDE_THRESHOLD`. Always logged
visibly in the transcript, never silent, never a relaxation of what the contract requires.
Also added `InferenceBackend::Ollama` in `local_model.rs` (`INTENTOS_INFERENCE_BACKEND=ollama`,
no network gate — same trust category as loopback).

**What's still broken, honestly**: Architecture (the next stage — read a prior artifact,
then write a new one) does **not** complete. Live evidence: the model repeats the identical
`read_file` action 6/6 times even when the observation explicitly names every missing
contract item verbatim. This is not a wording problem — the message is already maximally
specific. The model gets anchored on its first action in a multi-step stage and doesn't
pivot. Untried remaining options: a bigger local model, or redesigning Architecture's
step sequence. Stopped here deliberately, not stalled — see `agentLog.md` 2026-09-03 and
`decisions.md`'s new ADR for full detail, including a real infra finding (this machine runs
low on free RAM — 1.2 GiB free of 7.8 GiB — which caused two transient Ollama connection
failures at the Direction→Architecture transition; `OLLAMA_KEEP_ALIVE=30m` fixed it).

Rust 431/0/13, Svelte/TS 0/0/0. Committed this session per explicit instruction.

## Sovereign local executor + Groq backend — 2026-09-02

**Honesty note first**: this memory bank was last updated 2026-08-23. Everything below —
the entire sovereign local executor — was built across multiple sessions on branch
`feature/mission-v1` since then and was never logged here until now. Read `git log
feature/mission-v1` and the code itself, not just this file, for the full history; this
entry only covers what is needed to resume correctly.

**What exists now**: `src-tauri/src/local_agent.rs` (new, untracked) is a sovereign
model↔tool↔observation loop that runs `direction`/`architecture`/`development` stages
entirely inside IntentOS, with no external CLI (Codex/Claude) involved — see
`stage_uses_local_executor()` in `runtime.rs` for the dispatch. It is governed by a
deterministic **Stage Contract** (`validate_stage_contract`, `StageContract`) and a
causality-safe protocol parser (`resolve_turn_directive`) that resolves
`INTENTOS_ACTION:`/`INTENTOS_GATE:PASS|FAIL` sentinels by whichever marker appears
*first* in the raw completion text — not by which one happens to parse, and not by
trusting the model's own claims about what it did. Tools are `read_file`/`write_file`
only, path-contained to the run workspace. Per-turn prompts are rebuilt fresh every turn
via `WorkingContext`/`render_working_context` (bounded observed-reads budget:
`MAX_OBSERVED_READS_BUDGET_CHARS`), not accumulated, to avoid unbounded context growth.
Frozen budgets: `MAX_ITERATIONS = 6`, `MAX_STAGE_SECONDS = 240`, `MAX_TURN_TOKENS = 420`
— do not raise these without a live-evidence-backed reason.

**2026-09-02 delta — inference transport decoupled (Groq added as a second, non-sovereign
backend)**: `local_model.rs`'s `complete_raw` now dispatches on `InferenceBackend`
(`Loopback` — llama-server/Qwen, unchanged, still what `sovereign_ready` describes — or
`Groq`, selected by `INTENTOS_INFERENCE_BACKEND=groq`). Groq is deliberately never
"sovereign": fixed HTTPS endpoint (`https://api.groq.com/openai/v1/chat/completions`, not
configurable, so no local-looking env var can redirect traffic to the internet), gated on
every call by `crate::state::network_allowed` (paranoid mode blocks it exactly like every
other outbound feature), API key (`INTENTOS_GROQ_API_KEY`) read only into the
`Authorization` header, never logged/serialized. `GroqBackendStatus.configured` is a
static check only — `reachable`/`model_available` stay `None` until an actual call
succeeds, never assumed. `LocalCompletion.backend` added (`"loopback"`/`"groq"`); `local`
keeps its original meaning (`false` for Groq). 401/403/429 are classified distinctly,
429 surfaces `Retry-After`. Model: `openai/gpt-oss-120b` (`GROQ_DEFAULT_MODEL`).

**Live-verified via the real UI (CDP, `providerId: null`), not simulated**: with
`reasoning_effort: "low"` in the Groq request body (see below for why), a real intent's
**Direction stage passed cleanly in 2 turns** — one `write_file`, one real
`INTENTOS_GATE:PASS`, no hallucination, no repeated actions. This is the first live
completion of this loop's Direction stage that did not get stuck. Architecture started
but hit a real Groq 429 (free-tier `on_demand` rate limit: **8000 tokens/minute** for
`openai/gpt-oss-120b`) partway through its own turns.

**`reasoning_effort` history** (Groq/gpt-oss-family-specific request field, not part of
base OpenAI chat-completions, not sent on the loopback path): started at `"high"` on
request, measurably discarded — 1 of 5 trivial-prompt calls spent the entire
`MAX_TURN_TOKENS` budget on internal reasoning with zero visible output (a markerless
turn — its own stuck-turn failure mode). Lowered to `"medium"` (5/5 reliable, faster), then
to `"low"` after a live vertical run at `"medium"` hit the same 8000 TPM cap after only 6
Direction turns — every reasoning token spent counts against that per-minute budget.
Currently `"low"`.

**Known limitation, not a code bug**: the free Groq tier's 8000 TPM ceiling is too tight
for a full 3-stage vertical (Direction+Architecture+Development) in one run, even at the
cheapest `reasoning_effort` setting — confirmed by exhausting the cheapest lever available
before concluding this. Fixing it requires paying for Groq's Dev Tier; nothing left to
tune in the prompt/parameters. Left unpaid/unresolved deliberately per explicit instruction
("la idea es no gastar dinero").

**Also still open, not part of today's delta**: the loopback/Qwen path has a known,
unresolved failure mode (Direction repeating an identical `write_file` up to
`MAX_ITERATIONS` times without ever reaching PASS) that a redundant-write-detection guard
was designed for but never implemented — see `agentLog.md` 2026-09-02 for the design
sketch, still pending approval.

**Evidence gap worth knowing about**: `run_local_agentic_stage`'s turn-by-turn transcript
(`<stage>-<attempt>.stdout.log`) is only written via `atomic_write` on the loop's normal
exit path. A hard error mid-loop (e.g. this 429, or the earlier local context-overflow 400)
returns early and that file is never written — only the manifests exist. This predates
today's Groq work and would affect loopback the same way; not fixed here.

**Files touched today**: `src-tauri/src/local_model.rs` (Groq backend + tests),
`src-tauri/src/local_agent.rs` (threaded a `settings` handle through to `complete_raw` for
the paranoid-mode gate), `src-tauri/src/runtime.rs` (clones `state.settings` into the
spawned run task), `src-tauri/src/state.rs` (extracted `network_allowed` as a shared,
non-`AppState`-bound helper). Baselines after this delta: Rust 422 passed / 0 failed / 13
ignored, Svelte/TS 0 errors / 0 warnings.

## IntentOS Runtime v0.1 — CLOSED — 2026-08-23

Branch `intentos/runtime-v0.1` holds the experimental runtime extension; it remains
uncommitted pending a review/merge decision — this closure is a milestone on the branch,
not a merge to `main`. Scope is frozen: Codex CLI only, eight intent capabilities, five
sequential stages, catalog-backed teams, QA remediation capped at three attempts, local run
persistence, event streaming, cancellation and resumption. All 24 unique capability personas
resolve from the bundled corpus. Do not add providers or capabilities before reviewing the
complete diff and preserving this baseline.

**2026-08-22 15:58 snapshot** (superseded by the re-verification below, kept for history):
Svelte check 0/0, production build PASS, Rust 281 passed / 0 failed / 1 external-parity test
ignored, focused runtime 9/9, native Tauri compilation/launch reached the executable.

**Work that landed after that snapshot, now folded into this closure:** backend
(`src-tauri/src/runtime.rs`, `state.rs`, registered in `lib.rs`) and frontend
(`src/lib/stores/runs.svelte.ts`, `src/lib/components/Runbooks.svelte`) implement the full
`contracts.md` §F command surface and the `systemPatterns.md` §9 invariants. UI integration —
the Runbooks panel wired into `Sidebar.svelte` and `routes/+page.svelte` nav (⌘5), plus
IntentOS branding assets — landed 2026-08-22 22:00–22:38. A small, unrelated
language/appearance settings addition (`Settings.svelte`, `SettingsSectionAppearance.svelte`,
locale files) landed in the same window and is NOT part of Runtime v0.1.

**Final gates, 2026-08-23** — run manually by Wladimir and reported to the assistant (not
re-executed in this session; no shell access to the dev machine here): `npm run check` — 0
errors, 0 warnings. `npm run build` — PASS, only the non-blocking >500kB chunk-size warning.
`cargo test --lib` — 281 passed / 0 failed / 1 ignored. Runtime v0.1 is closed on this
evidence.

Factory evidence: TaskFlow automated gate 19/19 but browser QA remains blocked; the web
automotive MVP exists without final certification; the IoT vertical compiles and passes
backend/contract/MQTT/persistence gates but remains FAILED pending five QA findings and
fresh browser evidence. These are evidence projects, not production deployments.

**Separate, unrelated work on the same working tree (explicitly NOT part of this closure):**
a divisions/agency-organization rework — `agencyOrganization.ts`, `DivisionsLanding.svelte`,
`AgentsWorkspace.svelte`, `corpus.svelte.ts`, `categoryIcon.ts`, `presetTeams.ts` — started
2026-08-23 in the early morning, after this closure's code was already in place. It is
undocumented here and was intentionally left untouched while closing Runtime v0.1.

**State**: 🚀 **v0.2.0 SHIPPED (2026-06-23)** — `main` @ `16182e5`. First feature release since the v0.1.0
launch (the internally-tracked "0.1.1"/"0.1.2" milestones were never cut separately — they ship here), and
**auto-update is now LIVE** at [`agencyagents.app/updater.json`](https://agencyagents.app/updater.json) for
**both Mac arches** (`darwin-aarch64` + `darwin-x86_64`). Release at
[releases/tag/v0.2.0](https://github.com/msitarzewski/agency-agents-app/releases/tag/v0.2.0): **9 assets** (macOS
aarch64+x64 signed/notarized DMGs **+ updater tarballs**, Linux deb/rpm/AppImage, Windows x64/arm64). Homebrew:
`brew tap msitarzewski/agency-agents && brew install --cask agency-agents` (cask @ 0.2.0). Cross-platform CI in
`.github/workflows/` (linux-build, windows-build) fires on `v*` tags; macOS DMGs build locally via
`scripts/release.sh`. Full ship log: `agentLog.md` 2026-06-23; task doc `tasks/2026-06/260623_v0.2.0-ship.md`.

**Workflow (from 2026-06-16):** ALL changes go through a **branch → PR → merge to `main`**. No direct commits to main.

## IntentOS Navigation Charter v2 — APPROVED — 2026-08-23

Full architecture reconstruction (Hunter, IntentOS/Agency, Factory, Mission/Engagement,
Offer/Delivery) reviewed by the NCTO in two passes and approved. Persisted at
`memory-bank/intentosNavigationCharter.md` — **read that file before extending IntentOS
beyond what's built**. Key resequencing: Hunter's first narrow experiment (Legacy Hunter
candidate) now comes right after a minimal Mission/Engagement, not at the end of the
roadmap. Fase 0 (Agency + Runtime v0.1) and Fase 1 (`agencyRamosLink.ts` guard) were
already done; this persistence is Fase 2. **Fase 3 (Mission/Engagement mínimo) is next
and has NOT been started** — needs an explicit go-ahead from Wladimir before any code is
touched. This is a separate line of work from Runtime v0.1 (frozen at `404f4b1`, untouched)
and from the divisions/agency-organization rework noted above.

## Mission v1 (Agency Operating Specification) — implemented, verification pending — 2026-08-23

Following NCTO approval of four architecture-review passes plus a real-world Agency Operational
Pattern Survey, implemented the minimal Fase 3 Mission/Engagement layer: new `mission.rs`
(Mission type, persistence, 4 commands, deterministic `mission_brief()`), additive edits to
`lib.rs` (module + command registration) and `runtime.rs` (`mission_id` optional on
StartRunRequest/RunSummary; `stage_prompt()` interpolates the brief only when attached — output
is byte-identical to v0.1 otherwise, covered by a new explicit test). `types.rs`/`state.rs`
untouched. See `intentosNavigationCharter.md` for the corrected Fase 3 text and
`decisions.md`/`agentLog.md` for the full record.

**Pending**: git branch creation and `cargo test` — no terminal/device_bash access to this
machine in the session that wrote the code. Frontend (Runbooks.svelte / lib/types.ts /
lib/api.ts) not yet touched — needs a dedicated read-then-implement pass. See NEXT-SESSION.md.

## ✅ v0.2.0 — first feature release + LIVE auto-update — SHIPPED (2026-06-23, PRs #21 + #22; `main` @ 16182e5)
- **Auto-update is on.** Endpoint `agencyagents.app/updater.json` (Caddy on `umacbookpro` from `~/Sites/agency-agents/`,
  sibling vhost to the live `brew-browser.zerologic.com` manifest). **Dedicated agency signing key `ABF5AFD8`**
  (embedded pubkey in `tauri.conf.json`; private key + password in the **macOS Keychain**, services
  `agency-agents-updater-key` / `…-key-pw`; canonical key file backup at `~/.config/agency-agents-app/updater.key`).
  Live path = check → notify → one-click install; full hands-off auto-install is still deferred (the
  "Install updates automatically" toggle ships **present-but-disabled**).
- **Release build gotchas (now fixed + documented in `BUILD.md` / `release.sh` / PR #22):**
  - Updater-on macOS builds **must pass a `--config`** flag — the macos-private-api allowlist check reads only
    base `tauri.conf.json`, so the split `tauri.macos.conf.json` (`macOSPrivateApi:true`) is invisible to it
    (tauri#11142). `release.sh` now always passes `--config '{"app":{"macOSPrivateApi":true}}'`. (Every old
    `SKIP_UPDATER` build worked only because it happened to pass a `--config`.)
  - **Intel cross-compile needs the rustup toolchain** — Homebrew's `rust` is host-only (`can't find crate for
    core`). Build with `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`.
  - **Store the updater Keychain key via `$(cat …)`**, never a manual paste — a trailing newline corrupts it and
    signing fails with `incorrect updater private key password: Invalid input`.
- **Asset naming uses underscores** (`Agency_Agents_0.2.0_*`), NOT v0.1.0's auto-sanitized dots — the live updater
  manifest and the brew cask url both depend on this.
- **Post-ship UX (PR #23):** the Agents pane now has a **"Needs attention" filter** (= Outdated ∪ Modified ∪
  Missing). The install-state lens moved into nav (`ui.agentsLens`) so the Dashboard "N need attention" stat +
  health-donut segments **deep-link** to a **flat all-divisions** filtered list (`showDivisions` gates on
  `lens === "all"`); cross-launch lens persistence dropped (would hijack the landing).
- Decisions: `decisions.md` 2026-06-22 (host + dedicated key) and 2026-06-23 (build mechanics). Gates: cargo 264/0,
  svelte-check 0, signed+notarized, CI green.

## ✅ v0.1.2 — Tool registry + Osaurus + Playbook + Projects dashboard — SHIPPED (2026-06-21, PRs #18 + #19; `main` @ 1df932c)
**Tool knowledge is now a single source of truth.** It consolidated to the single canonical `tools.json` the
upstream `agency-agents` repo OWNS (twin of `divisions.json`, CI-guarded by its no-jq `check-tools.sh`); both the
Rust backend (`registry.rs`, `include_str!`) and the frontend (`toolRegistry.ts`) read it. **The Rust `Tool` enum
is GONE** — a tool is a string id; `label`/`detect`/`version`/`dests`/`scope` are registry lookups; `render()`
dispatches on the JSON `format` key. Frontend deleted ACCENTS/ICONS_SVG/SHORT/hardcoded SUPPORTED_TOOLS.
**Adding a tool = editing one JSON file** (+ a Rust formatter only for a brand-new output format).
- **All 13 tools** modeled (incl. Kimi, Osaurus); Tools panel shows installable + recognized-only (dimmed). Real
  brand logos (Lobe Icons, MIT) under `assets/tools/`, letter fallback otherwise.
- **Installability derived, not stored:** `installable(tool) = format ∈ IMPLEMENTED_FORMATS` ({identity,
  codex-toml, gemini-md, qwen-md, cursor-mdc, opencode-md, skill-md}). aa owns upstream truth; renderer coverage
  is app-side + self-maintaining (ship a renderer → add its format → those tools light up).
- **Osaurus wired** via a `skill-md` format (Agent-Skills `SKILL.md`, `slugPrefix:"agency-"`), byte-identical to
  upstream `convert_osaurus` — contributed UPSTREAM first (catalog owns transforms), mirrored here (parity test).
  Verified live: catalog agents run as native Osaurus skills.
- **Playbook** (in-app practices + copyable starter prompts + per-team/division examples; title-bar 📖 + ⌘K) +
  `docs/USING-AGENTS.md`. **Teams & Projects master/detail** via the system back arrow
  (`ui.projectsSelected`/`teamsSelected`); **division overview** + deploy.
- **Dashboard**: two-ring Global-vs-Projects install **sunburst** (`InstallSunburst.svelte`) so totals reconcile;
  cross-tool coverage **merged** with catalog-by-division (linked hover); uniform donuts, equal-height cards.

Task doc: [tasks/2026-06/260621_tool-registry-12-tools-osaurus.md](tasks/2026-06/260621_tool-registry-12-tools-osaurus.md).
Green: cargo 264/0, svelte-check 0, build clean. Upstream `agency-agents` (same machine): Osaurus transformer +
`tools.json` + `check-tools.sh` + `check-tools.yml` landed (aa PRs #605/#606).

## ✅ v0.1.1 IA arc — SHIPPED (2026-06-17 → 06-20, PRs #15 + #16, + the deploy-browser PR)
The whole "how people think about agents" reorganization landed:
- **Divisions landing** — the Agents tab opens on divisions; select-mode → bulk deploy.
- **Install-state lens** — filter the agent list by deployment state (In sync / Outdated / Untracked / Missing /
  Not installed), counts scoped to the division.
- **Teams** (renamed from Loadouts) — "Your team" (current installs, division-grouped) + "Team presets"
  (app-bundled `presetTeams.ts` + your saved teams, `teams.svelte.ts`).
- **how × where engine** (backend, PR #15) — tools are dual-scope; `render::dests()` scope-aware;
  `supports_user()`/`supports_project()`; install scope derived from the chosen project. Verified tool-path
  matrix (June 2026) in the PR. Cursor is project-only; Windsurf/Aider/Antigravity/openclaw deferred.
- **Projects pillar** (4th nav section, ⌘4; Activity → ⌘5) — `projects.svelte.ts` store (registered roots in
  localStorage ∪ the live ledger), `Projects.svelte` panel with rosters.
- **One `InstallModal`** — the destinations × tools GRID (rows = Global + each project + "Add project…",
  columns = detected tools, cells = tri-state toggles). Reused by agent detail, divisions, Teams, Projects.
  Replaced `DeployModal` + the inline switch-matrix. Agent detail: "Install…" in the title, pills on their own row.
- **Two-pane `DeployBrowser.svelte`** (Projects "Deploy…") — System-Settings master/detail: left =
  searchable list of EVERY granularity (agents · divisions · teams incl. saved · current roster); right =
  per-project per-tool install.

**Four-pillar model (drives IA copy):** Agents = *who* · Tools = *how* · Teams = *which* · Projects = *where*.

## 🔵 Backlog (next — full list in `docs/PLAN.md` post-0.2.0 punch list)
- **Opt-in automatic install** — the v0.2.0 "Install updates automatically" toggle is inert; wire it to a real
  off-by-default background download → verify → install (live updater is check → notify → one-click install today).
- **Refresh `tools.json` from the catalog clone** (vs. the bundled baseline) — like the corpus + `divisions.json`;
  cleaner now that aa #605 prunes stale convert output.
- **Foreign-sweep for nested skill dirs** (`…/<dir>/SKILL.md`) so CLI-installed Osaurus/Antigravity skills are
  detected (app-installed ones already are).
- **Antigravity wiring** once upstream makes its skill deterministic (drops the non-deterministic `date_added`).
- **"Auto Updates" subscription** for bulk installs — installing all of a division/team into a project/tool
  offers to auto-deploy newly-added catalog agents.
- **Copilot `.md` → `.agent.md`** (needs reconcile `file_stem` double-extension handling).
- Optionally tighten backend `detect()` to require the tool **binary**, not just a lingering config dir.
- Bonus: a "scaffold AGENT-ZERO into a project" action (every assistant honors repo-root `AGENTS.md`).

**Dev-harness note:** to screenshot-verify the Svelte frontend in a browser (the native Tauri window can't be
auto-driven), a `?shim=1` Tauri-IPC shim is temporarily injected into `src/app.html` then reverted — it never
ships. The shim can't open a real native folder dialog (returns a fixture path), so "Add project…" looks broken
in the shim though it works natively.

**Last updated**: 2026-06-23

## ✅ Pre-release polish (2026-06-15) — committed + pushed on `release-planning`
- **brew vestige cleanup**: error-type rename (`BrewError*`→`AppError*`), removed dead `catalogAutoRefresh`
  setting, removed the dead error codes (`brew_*`, `job_not_found`, `canceled`, `feature_disabled`,
  `vulns_not_installed`), and **deleted the brew-era Python pipeline** (`tools/{catalog,categorize,enrich,
  pipeline,trending-collector}` — they fetched Homebrew formulae, NOT used by AA; the catalog comes from
  `corpus/mod.rs`).
- **Activity Journal** (replaces the inherited, permanently-empty brew streaming "Activity"): pivoted
  `activity.svelte.ts` to a `JournalEntry` store (localStorage), `install.svelte.ts` logs every
  install/uninstall/update/track/bulk + default-target switch, `ActivityHistory.svelte` rewritten as a
  day-grouped clearable journal. Deleted `ActivityDrawer.svelte` + `AppStreamEvent`/`ActivityJob` types.
  Built via a Workflow (planner→builder→Code-Reviewer+UX-Architect team→fix loop); UX nits hand-polished.
- **Tools pane lens**: defaults to **Installed** (detected/in-use) tools; toggle `Installed · Not installed
  · All` (top row beside rescan, no count chips). `ToolsView.svelte`. Bar = **catalog coverage**
  (green installed / gray rest), not sync-state.
- **Agents workspace streamlined**: removed the filter lens (per-row install dots already show count);
  Division dropdown moved onto the search row as the first element (neutral form styling); detail pane
  hidden when no agent is selected (list goes full-width).
- **Cold `cargo test` tauri-gate fix**: `.cargo/config.toml` feeds `TAURI_CONFIG` so bare cargo (tests/CI)
  passes the `macos-private-api` allowlist gate (Tauri CLI overrides it for real builds). `macos-private-api`
  enabled in `Cargo.toml`. Verified `tauri dev` still launches clean.
- **Cross-platform creds FIXED + VM-validated**: GitHub token now persists to the OS-native vault per
  platform (Keychain / Credential Manager / Secret Service) via per-target `keyring` features; also moved
  `macos-private-api` to `[target.macos]` only (was wrongly in base deps → broke the Linux gate). Built +
  tested on Ubuntu (258/0 + deb/rpm/appimage) and Windows x64 via `phase-c.sh` VM matrix.
- **Dead-code/brew pass**: removed dead `agentsFilter` lens plumbing; scrubbed ALL brew comment mentions
  (grep → none); zero cargo dead_code warnings.
- **UX**: adaptive Uninstall/Delete wording by ownership; OS-style click-outside menu dismiss; Tools detail
  closes when the lens hides the tool; CoverageMatrix shades by **coverage-%** (not raw size).
- **Terminology**: user-facing **Category → Division** (catalog repo's term); internal `category` field kept.
- **Dashboard viz DONE**: replaced the cross-tool matrix with **CoverageDonuts** (one donut per tool,
  sliced by division, shared legend, linked hover); established a curated **division color scheme** as catalog
  metadata (PR github.com/msitarzewski/agency-agents/pull/592 = `divisions.json`) read via `corpus.colorOf`;
  Dashboard "Coverage by tool" click now selects the tool (`ui.openTools`). **`CatalogByDivision.svelte`** (NEW)
  replaces the orange bar-list: ONE proportional bar (segment per division, brand-colored), labels across FOUR
  lanes (2 top, 2 bottom) tied to segments by **non-crossing Z-elbow leaders** (rank-staggered rails +
  phase-shifted bottom columns), plus CoverageDonuts-style **linked hover** (dim others). Division **icons
  tinted** with their color in the `Division ▾` dropdown + persona pill (added `corpus.iconOf`); `categoryIcon.ts`
  gained `Map`+`Workflow` so gis/integrations stop falling back to "?". See `agentLog.md` 2026-06-15 (later 4).
- **Green throughout**: svelte-check 0 errors, cargo 258/0 (macOS + Linux), config validation all-pass.

## ✅ Phase C (2026-06-14) — both red items closed
- **Renderer parity VERIFIED.** `render/mod.rs` mirrors the upstream shell converter byte-for-byte
  (`source_field`/`source_body`/`slugify`/`output_slug`); new `--ignored` test diffs the real
  `scripts/convert.sh` → **232 agents × 5 transform tools = 1160/1160 byte-identical**. The
  `current`/Diff/Update model is now proven, not assumed.
- **Uninstall safety RESOLVED.** `remove_agent_files` backs up modified files FIRST (separate pass),
  byte-identical files need no backup, backup failure aborts the delete (original preserved). Tests cover
  every path.
- **Cross-platform chrome DONE.** Config split: base `tauri.conf.json` (decorations, opaque, no
  macOS-only keys) + `tauri.macos.conf.json` override (overlay titlebar/traffic-light/transparency).
- **Cleanup:** brew→Agency rename finished in `lib.rs`; dead `Settings` fields purged; docs overhauled;
  stale release notes removed; new `tools/phase-c/` validation runner. **Catalog now = 232 agents**
  (the re-org landed). Green: cargo 258/0 + parity 1/0, svelte-check 0, build clean.

## 🟣 Tahoe app icon (read first if touching icons)
macOS 26 renders icons from a compiled **`Assets.car`** (Icon Composer Liquid Glass), NOT `.icns` — `.icns`
-only = blank/gray squircle ("icon jail"). FIXED: `actool` (full Xcode only, by path) compiles
`docs/icon/AppIcon.icon` → `src-tauri/Assets.car` (in `bundle.resources`) + Tahoe-aware
`src-tauri/icons/icon.icns`; `src-tauri/Info.plist` adds `CFBundleIconName=AppIcon` (Tauri merges it).
**Don't run `npm run tauri icon`** (clobbers the glass icns). Full recipe: `docs/icon/README-liquid-glass.md`.
Dev Dock hack REMOVED (lib.rs plain `.run()`, objc2 deps dropped).


## Current state (read NEXT-SESSION.md for the full picture + IMMEDIATE backlog)
- **Phase B + nav + Tools (2026-06-09):** Dashboard has 4 dependency-free charts (`HealthDonut`,
  `CoverageMatrix` category×tool, coverage-by-tool bars, category distribution). **Back/forward nav**
  (titlebar ◀▶, ⌘[/], mouse 3/4) over a `ui` NavLocation history; `agentsCategory`+`agentsSelected`
  lifted into `ui`. **Division pills deep-link** everywhere (`ui.openDivision`); lens counts narrow to
  the division; added "Not installed" lens; zero-count lenses/stats hide. **Tools = list/detail console**
  (`ToolsView` rebuilt): badges (`util/toolBadge`), health bars, versions (`tool_versions`), Reveal
  (`reveal_path`), Default-target Switch, Sync-to-catalog/Track-all/Remove-all, projects list. Dev Dock
  icon set on `RunEvent::Ready` (macOS debug). Icon redrawn as a **macOS squircle** (regenerated).
- **UNIFIED Agents workspace (Phase A done).** Agents + Library are ONE three-pane surface
  (`AgentsWorkspace.svelte`): list pane (filter lens All/Installed/Needs-attention/Untracked + search +
  Category ▾ + Select-mode bulk) · `ResizeHandle` · persistent detail pane (`PersonaBody` + the
  `DeploymentMatrix`). `PersonaDiscover.svelte` + `AgentLibrary.svelte` DELETED.
- **Deployment band under the name/division**: summary pills for installed tools + a "USE WITH ⌄"
  disclosure. User tools = `Switch` (on=installed); project tools = Install/Add-project + per-project
  sub-rows. Drift actions (Diff/Track/Update) inline when applicable. New `Switch.svelte` (shared,
  extracted from Settings→Network), `util/platform.ts` (⌘/Ctrl shortcut glyphs).
- Nav: `library` section retired everywhere; `ui.agentsFilter` + `ui.openAgents(filter)` deep-link
  (Dashboard cards + palette use it). Section id stayed `personas`.
- **Byte-identical foreign → `current`**; **recursive indexing**; `agent_diff` + `DiffModal`; Track (safe).
- Active catalog = **userClone** `/Users/michael/Software/AgentLand/agency-agents` (manage:true).
- **Signed + notarized `.app`/`.dmg`** via `scripts/release.sh` (SKIP_UPDATER=1). 247 Rust tests / 0.
- 🔵 NEXT: **Phase B** = 4 Dashboard charts (coverage matrix · health donut · category distribution ·
  per-tool coverage), dependency-free SVG/CSS, cells deep-link into the workspace. Then **Phase C** =
  Windows/Linux titlebar degradation + "this device" copy + home-path display.
- ✅ CLOSED 2026-06-14: (1) **renderer parity** vs convert.sh — VERIFIED 1160/1160 byte-identical;
  (2) **uninstall safety** — RESOLVED (backup-first for modified, none for byte-identical, abort-on-fail).

## (historical) Earlier this arc
- **Adopt → Track**: destructive Adopt gone. `track_agent` records provenance, writes nothing; every
  write backs up first (`<app_data>/backups/`); `agent_diff` for review-before-Update.
- **categories from tooling**: `discover_categories` parses `AGENT_DIRS` from
  `scripts/convert.sh`. **Data fix: `integrations` (convert.sh output) dropped (210→209); `strategy`
  added.** Removed the orphan `integrations/backend-architect-with-memory.md` from the baseline (it's
  a valid-but-misfiled enrichment example; to ship it for real, promote it UPSTREAM into a real
  category — then it flows in via refresh).
- **#1 slices 2–4 — catalog source**: `CatalogSource` (Bundled | Managed{~/.agency-agents} |
  UserClone{path,manage}) in `state/catalog.json`; corpus reads/writes the RESOLVED root. Detect
  (~/.agency-agents + "Find" scan), provision (git clone or snapshot), pull (git pull or tarball).
  First-run picker (`CatalogFirstRun`) + `Settings → Catalog`. Verbs Track/Update, manage-with-
  permission, picker+Find — all as decided. cargo test 275/0; svelte-check 0 err; build green.
- ⚠️ NOTE: existing installs (incl. Michael's) have no catalog.json → the **first-run picker WILL
  appear** on next launch (by design — one-time source choice; pick "Bundled" to keep current).

> Full plan + sequence: `phases/phase-roadmap.md` (the "v2" block). Detailed resume notes +
> gotchas: `NEXT-SESSION.md`. Build spec: `contracts.md`. Architecture: `systemPatterns.md`.

## How to run (dev)
- `npm run tauri dev` from repo root. **Dev server is on port 1430** (NOT 1420 — that's
  brew-browser; sharing it makes one app load the other's frontend). HMR for frontend; Rust changes
  recompile. The app opens on **Agents** (personas).
- Reference clones (read-only): `/tmp/brew-browser-inspect`, `/tmp/agency-agents-inspect`.

## What works (verified)
- **Agents** catalog (210 agents / 16 categories), search, persona detail with an **Install** menu.
- **Library** — flat list of installs; your ~184 `install.sh` agents show as `foreign` with Adopt.
- **Tools**, **Loadouts** (Agentfile), **Dashboard** (agency rollup), Activity, Settings (⌘,).
- Backend: `corpus · render · install · github · util · commands{github,settings,updater}`.
  `cargo test` ~265/0; `vite build` + `svelte-check` green; app boots clean (210 corpus seeded).
- New brain-circuit **app icon** (dark shipped; light master in `docs/icon/`). About window rebranded.

## Immediate next: Michael runs it, then #2 / #3
**#1 slices 1–4 ✅ done.** Remaining for #1 (deferred refinements, non-blocking):
- `aliases.json` (slug renames across catalog versions) — not yet honored.
- Explicit **orphan** surfacing (ledger rows whose slug left the catalog) + unique-slug enforcement.
- `.agency-cache/` convention + add to the agency-agents repo `.gitignore` (cache not yet written).
- Symlink-aware reconcile (the `~/.claude` alias case) — still the old behavior.

Then: **#2 Track-all / Update-all**; **#3 tool-grouped Library IA** (L1 tools+counts → L2 per-tool)
+ wire `agent_diff` into a review-before-Update UI.

## Decisions locked (this session)
- Build order: **Both, Track first** → Track DONE, now #1.
- Clone detection: **picker-primary + a "Find Agency Agents" button** (opt-in scan, not auto).
- Existing clone: **manage-with-permission**. Managed path: **`~/.agency-agents`**.
- Cache dir: `.agency-cache/`. Verbs: **Track / Update**.
- Categories: **parse from repo tooling** (`AGENT_DIRS` in convert.sh), not a frontmatter heuristic.

## ✅ RESOLVED: "Adopt" is no longer unsafe
Adopt → **Track** (non-destructive) + backup-on-write shipped this session. The old clobber path is
gone.
