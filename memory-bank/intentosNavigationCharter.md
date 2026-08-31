# IntentOS Navigation Charter (Carta de Navegación) — v2

**Status**: APPROVED by the NCTO (Wladimir), 2026-08-23. **Scope**: the full IntentOS
architecture — Hunter, IntentOS/Agency, Factory, Mission/Engagement, Offer/Delivery — and how
they relate. This is the reference future sessions must read before extending IntentOS beyond
what is already built. It contains **no code changes**; it is pure architecture. `Runtime v0.1`
remains frozen at commit `404f4b1` and is not touched by anything in this document.

Companion doc: `intentosAgencyModel.md` covers the detailed division/profession/agent mapping
(the "who can do the work" layer). This charter covers the outer system the Agency sits inside
(the "why work exists, how it's found, and what happens to it" layers). Neither duplicates the
other; read both.

## How this document came to be

Drafted as an HTML artifact after a full architecture-reconstruction request, reviewed by the
NCTO in two passes: a first pass with ten corrections (sequencing, dual-nature concepts, a
missing conceptual layer), and a second pass with two microcorrections on placement detail.
Approved as final on the second pass. The full visual version (diagrams, status legends, phase
timeline) lives as a Claude Artifact; this file is the durable, version-controlled record.

## The meta-flow (approved, unchanged since v1)

```
Mundo → Hunter → Opportunity → NCTO → Mission → IntentOS/Agency → Factory → Resultado
  → Offer/Delivery → Mercado → Evidencia → Aprendizaje → (retroalimenta a Hunter)
```

## Confirmed decisions (fixed points — do not relitigate without the NCTO)

- **Agency** represents the workforce/capabilities. It answers "who can do the work," never
  "for whom" or "why."
- **Hunter** discovers opportunities; it does not execute work.
- **Mission/Engagement** explains why and for whom a piece of work exists.
- **Factory** executes an approved mission.
- The **human NCTO** keeps GO/NO-GO and all strategic/commercial decisions — always. Automation
  reduces operational load; it never absorbs the directive function.
- Departments, Ramos, Verticales, and Capabilities must not absorb concepts that belong to other
  axes (mission type, discovery strategy, commercial offer).
- **Runtime v0.1 stays frozen at `404f4b1`.** No architecture decision in this charter touches
  its IDs, contracts, or five-stage pipeline.
- A new idea does not automatically become a tab, ramo, agent, or module — run it through the
  checklist in §9 first.

## §1 — Hunter (discovery)

Nothing is implemented yet. The closest adjacent code is `routeIntent()` in
`intentosCapabilities.ts`, which routes an intent a human already formulated toward one of the
8 capabilities — it classifies, it does not discover.

**`Opportunity` is a shared domain entity/contract, not a module.** Every future Hunter mode
must produce the same minimal representation — evidence, problem/opportunity, source, value
hypothesis, and a few basic fields sufficient for the NCTO to decide. No incompatible per-mode
data shapes. A sophisticated `Hunter Score` comes later, once real evidence has accumulated —
not before.

**Hunter Core is a platform of strategies, not many independent Hunters.** All of the following
are modes/strategies plugable onto the same Core and the same `Opportunity` object — none
implemented yet, preserved here for future research:

- Job / Work Hunter
- Service Hunter
- Back-office / Process Compression Hunter
- Organization Compression Hunter
- Legacy Software Hunter
- Pain / Friction Hunter
- Technology / Capability Hunter
- Local / Global Market Hunter
- Competition / Pricing scouting
- Signals from reviews, communities, marketplaces, and other public sources

**The first experiment must be narrow.** No universal crawler in v0. Preferred candidate:
**Legacy Hunter**, with a deliberately limited mission — find categories of old, still-used,
economically relevant software whose workflows carry friction that AI/agents could remove or
compress. The goal of the experiment is not the definitive Hunter; it's proving the loop once:
Hunter finds an Opportunity → NCTO evaluates evidence → GO → a Mission is created → IntentOS
forms/uses a team → Factory/Runtime produces an experiment → evidence comes back. Expand only
after that loop works.

## §2 — IntentOS / Agency (decision and organization)

Already built and correctly scoped. 209 agents / 16 source categories → 14 Departments, 8
Ramos, 3 Verticals (`agencyOrganization.ts`), 8 execution Capabilities (`intentosCapabilities.ts`),
the guard between both eight-lists (`agencyRamosLink.ts`), a three-lens UI
(`DivisionsLanding.svelte`), and Runtime v0.1 closed and committed. Partial: the 6-level
conceptual model (División → Profesión → Especialista → Capacidad → Equipo → Workflow) only has
two levels implemented in code (Department, individual Agent) — the rest lives in
`intentosAgencyModel.md` prose, not data. See that file for the full detail; this charter does
not restate it.

## §3 — Factory (production)

`brief → research → specification → construcción → QA → security → remediation → evidence →
packaging/delivery`. Produces a **Resultado** — a technical output that is not yet the same
thing as a commercial offer (see §4). Runtime v0.1 is the only automated circuit (single
provider, no formal security gate, no explicit packaging/delivery step); TaskFlow, Automotora
Don Pablito, and the IoT Factory Test are three hand-orchestrated validation exercises that
motivated Runtime v0.1, not a competing system. Expanding Factory (multi-provider, security
gate, packaging, non-software outputs) is a new branch — an eventual Runtime/Factory v2 —
never a modification to what's closed in `404f4b1`.

## §4 — Offer / Delivery / Comercialización (new in v2)

A conceptual layer between Factory and Mercado, added because a technical Resultado is not
automatically a commercial offer. It answers: what are we selling, to whom, what outcome do we
promise, what's the scope, how is it packaged, what's the pricing/commercial model, how is it
delivered, and what post-delivery support exists. It does not need to become an independent
module yet — for now it only needs to exist in the architecture, so no future phase assumes
`Factory output == sellable product`. Classified as strategy/conceptual layer (§9), not module,
until a real case forces it to be built.

## §5 — Mission / Engagement

Explains why and for whom work exists. Orthogonal to Departments/Ramos/Verticales/Capabilities —
never lives there. **Correction (2026-08-23, Mission v1):** the original Fase 3 plan assumed
Mission would extend `ProjectInfo` — inspection of `types.rs` found that `ProjectInfo`
(`path`/`label`/`installed_count`) belongs to the pre-existing, unrelated "Projects"
install-tracking registry, not to Mission semantics. Mission is instead its own type
(`src-tauri/src/mission.rs`), independent of `ProjectInfo` and of Runtime v0.1's `RunSummary`,
keyed by `project_path` the same way a run is. `Client Services` and `Internal
Products/Ventures` remain initial modalities of an open, extensible field — not a closed
two-value enum. A mission can evolve, e.g. from opportunity research to internal experiment to
commercial offer.

**Mission Brief.** Every Mission produces a deterministic, plain-text serialization of its own
fields — `mission::mission_brief()` — that Runtime v0.1's `stage_prompt()` interpolates ahead of
the raw intent when a run has a Mission attached (`StartRunRequest.mission_id`, optional). No
LLM, no agent, and no interpretation sit in that path; same Mission in, same string out, always.
Omitting `mission_id` preserves Runtime v0.1's original prompt output byte-for-byte — this is
what keeps the frozen `404f4b1` contract intact while adding the capability.

**Why it comes before Hunter:** Hunter is meaningless without a formal destination for what it
finds. An `Opportunity` the NCTO approves needs to become something — and that something is a
Mission. That's why Mission/Engagement mínimo is Fase 3 and the first Hunter experiment is
Fase 4, not the reverse (see §8).

## §6 — Legacy software modernization — two faces

Not a ninth Ramo. Split into a discovery face and an execution face:

- **Legacy Hunter** (discovery) — a Hunter Core mode that discovers existing/old software
  categories with real market, users, and workflows susceptible to transformation. Also the
  preferred candidate for Hunter's first narrow experiment (§1).
- **Legacy Modernization Mission** (execution) — a mission pattern inside Mission/Engagement
  that takes an NCTO-approved opportunity and studies/rebuilds that software under an AI-native
  paradigm, combining existing Ramos (Estrategia/Producto, Sistemas/Datos, Experiencia Digital,
  Ciberseguridad, DevOps).

Strategic thesis, unchanged: don't just look for new applications. Look for software built
under the technical limitations of the past and ask how the same problem would be solved today
if the product were built from scratch with AI, agents, vision, voice, automation, APIs, and
current paradigms.

## §7 — Fábrica de Trabajos Digitales — two faces, plus placement microcorrections

Original formula: `trabajo/proceso existente → funciones → coste/precio → herramientas →
% comprimible vía IA/agentes/APIs → HITL restante → servicio/producto vendible`.

- **Discovery** — a Hunter Core mode oriented at discovering economically compressible existing
  jobs/processes/operations (the left half of the formula).
- **Execution** — a mission pattern that takes an approved opportunity and turns it into a
  service, automation, or operational system (the right half).

Neither is implemented; this section only corrects classification.

**Approval microcorrection**: Back-office/Process Compression and Organization Compression are
no longer "unplaced." Their discovery face already has a named home:

- **Back-office / Process Compression Hunter** — a Hunter Core mode. Its execution face is the
  same Fábrica de Trabajos Digitales — Execution described above; no separate mission pattern
  is needed.
- **Organization Compression Hunter** — a Hunter Core mode. Unlike Back-office/Process
  Compression, its execution face is **not** assumed equal to Fábrica — Execution; it stays
  unplaced until a real case forces a decision.

**Medios sintéticos — declassified.** v1 asserted it "would eventually be a new Capability."
That is not decided. A synthetic-media mission might be solved by composing existing
capabilities plus specialized tools/APIs; it should only become its own Capability if real
cases demonstrate the current capabilities don't adequately represent that work. For now:
strategy / potential output class / undecided future capability — nothing firmer than that.

Back-office agentic work and studying compressible organizations more broadly remain living
knowledge with zero implementation and zero placement decision beyond the two Hunter modes
above.

## §8 — Placement table (piece → layer → where it fits → doesn't duplicate)

| Piece | Layer | Where it fits | Doesn't duplicate with |
|---|---|---|---|
| Hunter Core | Module | Single platform of pluggable strategies/modes, not many independent Hunters | Not a Ramo or Capability — it searches, it doesn't execute |
| `Opportunity` object | **Entity/shared contract** (not a module) | Produced by Hunter Core; consumed by NCTO and Mission/Engagement as decision input | Doesn't belong to Departments/Ramos |
| NCTO | Core | Human role — GO/NO-GO and commercial/strategic decisions, always | Not automated; doesn't compete with any agent |
| Mission / Engagement | Module | Own type (`mission.rs`), independent of `ProjectInfo`; open field, not a closed enum; must exist before Hunter's first experiment | Doesn't live in Departments/Ramos/Verticales/Capabilities |
| Offer / Delivery | Strategy | Conceptual layer between Factory and Mercado; avoids assuming Resultado == commercial offer | Doesn't duplicate Factory — Factory produces the technical Resultado, not the offer |
| Legacy Hunter | Module (Hunter Core mode) | Discovers legacy software categories with compressible friction; first-experiment candidate | Not the 9th Ramo; distinct from Legacy Modernization Mission |
| Legacy Modernization Mission | Mission type | Template inside Mission/Engagement combining existing Ramos | Not the 9th Ramo; distinct from Legacy Hunter |
| Fábrica de Trabajos Digitales — Discovery | Module (Hunter Core mode) | Left half of the formula: detects compressible work | Not a new division |
| Fábrica de Trabajos Digitales — Execution | Mission type | Right half: turns an approved opportunity into a service/automation | Not a new division; subcategory of Internal Products/Ventures |
| Back-office / Process Compression Hunter | Module (Hunter Core mode) | Discovers compressible back-office processes; feeds directly into Fábrica — Execution | Not a new division |
| Organization Compression Hunter | Module (Hunter Core mode) | Discovers organizations with compressible processes; execution not yet decided | Not a new division; distinct from Fábrica — Execution |
| Medios sintéticos | Strategy | Future Capability **not decided**; might be solved by composing existing capabilities | Not a new division on its own |
| Factory (expanded) | Module | New branch (v2): multi-provider, security gate, packaging, non-software outputs | Never modifies Runtime v0.1 / `404f4b1` |

## §9 — Core / Module / Mission type / Strategy classification

**Core** (nothing works without it): Agency (Departments/Ramos/Verticales/Capabilities +
corpus), Runtime v0.1, the Projects registry, the NCTO (human role).

**Module** (its own subsystem, versioned separately, connected to core by contract): Hunter
Core — Legacy Hunter, Fábrica-Discovery, Back-office/Process Compression Hunter, and
Organization Compression Hunter are modes of it, not separate modules — and it produces the
shared `Opportunity` contract; the Mission/Engagement layer, which consumes `Opportunity`;
Factory v2.

**Mission type** (configuration/template over what already exists — not new code): Legacy
Modernization Mission; Fábrica de Trabajos Digitales — Execution; Client Services · Internal
Products/Ventures (initial, open values).

**Strategy / knowledge** (living documentation, no implementation required yet): Medios
sintéticos, avatares, producción audiovisual (undecided); Offer/Delivery/Comercialización (not
yet a module); scouting de servicios, organizaciones comprimibles; Hunter modes not yet designed
in detail (Job, Service, Technology, Pricing, …).

**Approval microcorrection**: `Opportunity` is not a module. It is the entity/domain contract
that Hunter Core (the module) produces, and that NCTO and Mission/Engagement consume as decision
input.

## §10 — Phased build sequence (approved — do not resequence without the NCTO)

- **Fase 0 — done.** Agency + Runtime v0.1. Departments/Ramos/Verticales/Capabilities surveyed;
  Runtime v0.1 closed and committed at `404f4b1`.
- **Fase 1 — done.** Internal-organization guard. `agencyRamosLink.ts` prevents `AGENCY_RAMAS`
  and `INTENTOS_CAPABILITIES` from drifting apart accidentally.
- **Fase 2 — done (this document).** Persist the corrected architecture. This file, and the
  linked agentLog/decisions/toc entries, are that persistence.
- **Fase 3 — minimal Mission/Engagement implemented, 2026-08-23, verification pending.**
  Preceded by an Agency Operational Pattern Survey (real-world evidence, not assumption-first
  design) and an Agency Operating Specification v1 consolidating it. Delivered: `mission.rs`
  (new — `Mission` type, `MissionStatus`/`EngagementRegime` enums, disk persistence at
  `state/missions/<id>.json` mirroring the existing run-persistence pattern, `mission_create`/
  `mission_get`/`mission_list`/`mission_update` commands, and the pure `mission_brief()`
  serialization function); `lib.rs` (registers the module + 4 commands); `runtime.rs`
  (`StartRunRequest.mission_id: Option<String>` and `RunSummary.mission_id: Option<String>`,
  both additive with `#[serde(default)]`; `stage_prompt()` interpolates the Mission Brief ahead
  of `USER INTENT:` only when a Mission is attached — byte-identical output otherwise). New Rust
  unit tests cover Mission Brief determinism and the backward-compatibility guarantee
  explicitly. **Deferred out of this pass, per the Operating Specification**: `sourceLocale`,
  locale propagation logic, localization QA, a Policy Ledger engine, a third gate signal
  distinct from PASS/FAIL, and Mission Brief mid-run versioning. **Still pending — could not be
  done in this session**: creating the git branch and running `cargo test`, because this session
  had no `device_bash`/terminal access to this machine. See NEXT-SESSION.md for the exact
  commands to run.
- **Fase 4.** `Opportunity` mínimo + Hunter Core v0 with a single mode. Preferred candidate:
  Legacy Hunter, with the narrow mission defined in §1/§6. Produces a minimal `Opportunity`
  (evidence, problem, source, value hypothesis) — no scoring yet, feeding the NCTO by hand.
- **Fase 5.** Close one real loop: Opportunity → NCTO GO → Mission → Agency/Runtime →
  Resultado. The goal is not the definitive Hunter — it's proving the full circuit works
  end-to-end once.
- **Fase 6+.** Expand based on evidence: the rest of the Hunter modes, `Hunter Score`,
  Factory v2 (multi-provider, security, packaging), and only then evaluate whether
  Offer/Delivery needs to become its own module. Medios sintéticos only if a real case demands
  it. Guardrail, every phase: `404f4b1` is never touched.

**IMPORTANT for future sessions**: as of this document's persistence, only Fase 0–2 are done.
Fase 3 has **not** been started. Do not begin Fase 3 (or any later phase) without an explicit
go-ahead from the NCTO (Wladimir) in a live session — this charter records the approved
sequence, it is not itself authorization to start building.

## §11 — Reusable decision checklist

Before turning a new idea into a tab, ramo, agent, or subsystem:

1. **Is it a capability?** → Belongs in Ramos/Departments/Capabilities — only if no combination
   of the existing 8 ramos already covers it.
2. **Is it a mission?** → Belongs in Mission/Engagement as a type or template — never as a new
   agent structure.
3. **Is it a discovery strategy?** → Belongs in Hunter, as a mode inside Hunter Core.
4. **Is it a tool?** → Lives inside an existing module (Runtime, Factory) as a provider or
   integration, not as a new layer.
5. **Is it a workflow?** → It's a sequence of stages over what already exists — a Runbook
   candidate, not a new core.
6. **Is it a commercial output?** → Lives as evidence of a Mission, filtered through
   Offer/Delivery — never as the root of an agent taxonomy.
7. **Does it assume a technical result is already an offer?** → Check: every Factory Resultado
   conceptually passes through Offer/Delivery before reaching Mercado.
8. **Does it keep the NCTO at the center of the decision?** → If it automates search or
   execution but moves GO/NO-GO or commercial commitments away from the human, the design is
   wrong — see §12.

## §12 — Business principle

> "Are we building a system to administer agents, or a business machine to discover
> opportunities and turn them into products, services, and economically valuable outcomes?"

The correct answer is the second. Agents are IntentOS's internal workforce, not the system's
final purpose. Human-in-the-loop is explicit and permanent: Hunter can search, investigate,
compare, group, score, and recommend; IntentOS can decompose and form teams; Factory can
execute, test, and remediate — but pursuing an opportunity, taking on commercial commitments, or
turning research into a product remains the NCTO's decision. Automation should reduce
operational load, never eliminate the directive function.

---
*Persisted 2026-08-23 following NCTO approval (v2, ten corrections + two placement
microcorrections on Back-office/Organization Compression Hunter and the `Opportunity`
classification). Runtime v0.1 (`404f4b1`) untouched.*

*Updated 2026-08-23: Fase 3 minimal Mission/Engagement implemented (`mission.rs`, plus additive
edits to `lib.rs` and `runtime.rs`) following NCTO approval of the Agency Operating
Specification v1, itself grounded in a real-world Agency Operational Pattern Survey. Runtime
v0.1 (`404f4b1`)'s own contract remains untouched and byte-compatible; `mission_id` is optional
everywhere it appears. Git branch creation and `cargo test` remain pending — this session had
no terminal access to this machine.*
