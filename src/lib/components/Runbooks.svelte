<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import PaperclipIcon from "@lucide/svelte/icons/paperclip";
  import XIcon from "@lucide/svelte/icons/x";
  import SquareIcon from "@lucide/svelte/icons/square";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import ArrowLeftIcon from "@lucide/svelte/icons/arrow-left";
  import CheckIcon from "@lucide/svelte/icons/check";
  import Button from "./Button.svelte";
  import { corpus } from "$lib/stores/corpus.svelte";
  import { runbooks } from "$lib/stores/runbooks.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { runs } from "$lib/stores/runs.svelte";
  import { preview } from "$lib/stores/preview.svelte";
  import { publicPreview } from "$lib/stores/publicPreview.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { CREATION_CATALOG, classifyConversationalIntent, findCatalogProduct, INTENTOS_CAPABILITIES, isTeamCompositionQuestion, planSolutionAsync } from "$lib/data/intentosCapabilities";
  import { session, buildConversationalIntent, draftBuildConfirmation, draftTeamExplanation, narrateRunForEsmeralda, replyToEsmeraldaChat } from "$lib/stores/session.svelte";
  import type { PendingBuildProposal, SolutionProposal } from "$lib/data/intentosCapabilities";
  import type { Agent, AutomaticProject, ConversationMessage, Mission, RunEvent } from "$lib/types";

  const LEGACY_DRAFT_KEY = "intentos.productionBrief.v1";
  // 2026-09-05 — the pre-project chat used to be truly ephemeral (see
  // `localMessages`'s original doc comment below), which meant a real,
  // possibly long conversation with Esmeralda vanished the moment the app
  // closed or restarted, with no warning. Fixed by persisting it through
  // the exact same `session_get_or_create`/`session_append_message`
  // backend calls a real project's conversation already uses, keyed by a
  // synthetic "project path" (never touched by any filesystem code — it's
  // just the hash key `ProjectSession` is stored under) instead of a real
  // one. `localMessages` itself is unchanged; this only makes it survive
  // a restart.
  const DRAFT_SESSION_ID_KEY = "agency-agents:draft-session-id:v1";
  function draftSessionPath(): string {
    let id: string | null = null;
    try { id = localStorage.getItem(DRAFT_SESSION_ID_KEY); } catch { /* ignore */ }
    if (!id) {
      id = crypto.randomUUID();
      try { localStorage.setItem(DRAFT_SESSION_ID_KEY, id); } catch { /* ignore */ }
    }
    return `__draft__:${id}`;
  }
  /** Abandons the current draft session (a fresh id is created lazily next
      time a message is sent) — called on an explicit "Nueva intención" so
      a deliberate reset doesn't drag the old conversation back in. The
      abandoned file itself is left on disk, same as any other stale
      session/run IntentOS already never garbage-collects. */
  function resetDraftSession(): void {
    try { localStorage.removeItem(DRAFT_SESSION_ID_KEY); } catch { /* ignore */ }
  }
  // `selected` (a NEXUS scenario runbook from strategy/runbooks.json) is
  // optional correlation metadata, not a prerequisite: runtime_start only
  // needs runbookId to be a non-empty string (validate_start_request) and
  // uses it as one of several equality keys when matching an interrupted
  // run to resume — it never looks the id up against any catalog. On a
  // bundled/unsynced catalog `runbooks.list` is empty (by design — see
  // runbooks.svelte.ts), which must never block approving a build. When a
  // real runbook is selected its slug is still used, unchanged.
  const DEFAULT_RUNBOOK_ID = "intent-build";
  const DEFAULT_SIGNATURE = "Resolver el trabajo real descrito por la intención y recomendar la forma computacional más adecuada, sin asumir de antemano web, aplicación, documento o formato. El resultado debe ser operable, propio y verificable; nunca presentar mocks, datos simulados o funciones incompletas como terminadas. Aplicar claridad, accesibilidad, seguridad, rendimiento, trazabilidad y facilidad de uso cuando correspondan al producto. Cada entrega debe incluir evidencia suficiente para que Reality Check compare lo construido con la propuesta aprobada por el NCTO.";
  onMount(() => {
    // Security baseline v0.1: production briefs can contain client data and
    // must never live in plaintext WebView storage. Remove the legacy draft
    // once and keep the current brief in memory until an encrypted workspace
    // store exists.
    try { localStorage.removeItem(LEGACY_DRAFT_KEY); } catch { /* best effort */ }
    corpus.ensureLoaded(); runbooks.load(); projects.refresh(); runs.load();
    // Restore a pre-project conversation that survived a restart (see
    // DRAFT_SESSION_ID_KEY above). No-op the first time a draft id has
    // never been created, or once a project is already selected.
    if (!projectPath) {
      void session.loadOrCreate(draftSessionPath()).then((restored) => {
        if (restored && restored.messages.length > 0 && localMessages.length === 0) {
          localMessages = restored.messages
            .filter((m) => m.role !== "system")
            .map((m) => ({ id: m.id, role: m.role as "user" | "esmeralda", content: m.content, at: m.at }));
        }
      });
    }
  });
  const bySlug = $derived(new Map(corpus.agents.map((a) => [a.slug, a])));
  let intent = $state("");
  let attachments = $state<string[]>([]);
  let constraints = $state("");
  let acceptance = $state("");
  let projectPath = $state("");
  let useExistingProject = $state(false);
  /** The chat before any project exists yet — 2026-09-04, "quiero que sea
   *  un chat... yo ahí le declaro lo que quiero". 2026-09-06: mirrored into
   *  a real persisted session behind a synthetic path (DRAFT_SESSION_ID_KEY
   *  above) so it survives a restart — a real conversation was lost to
   *  exactly that before this fix. Once a build instruction creates a
   *  project (see sendTurn), the visible conversation switches over to
   *  that project's own persisted `session.messages` — this array is only
   *  ever shown before that point. */
  let localMessages = $state<{ id: string; role: "user" | "esmeralda"; content: string; at: string }[]>([]);
  /** The build description (plus any explicit restrictions attached to it,
   *  e.g. "pero no implementes nada todavía") awaiting an explicit
   *  go-ahead before any project gets created — see sendTurn's own doc
   *  comment. `null` means no build discussion is pending (either none
   *  started, or the last one was confirmed/abandoned via `newIntent`). */
  let pendingBuildProposal = $state<PendingBuildProposal | null>(null);
  let chatLogEl: HTMLOListElement | undefined = $state();
  let selectedSlug = $state("");
  let proposal = $state<SolutionProposal | null>(null);
  let previousProposal = $state<SolutionProposal | null>(null);
  let proposalChanges = $state<string[]>([]);
  let proposalRevision = $state(0);
  let rejectionReason = $state("");
  let rejecting = $state(false);
  let approving = $state(false);
  let validation = $state("");
  // True only while the hybrid router's semantic fallback is actually
  // running (an unambiguous intent never sets this — see
  // isFastPathConfident in intentosCapabilities.ts) so the UI can show
  // real activity instead of looking frozen during the one case that adds
  // latency.
  let resolvingProposal = $state(false);
  let catalogDraftActive = $state(false);
  let loadedCatalogProductId = $state<string | null>(null);
  const catalogProduct = $derived(findCatalogProduct(ui.catalogProductId));
  const catalogArea = $derived(CREATION_CATALOG.find((area) => area.products.some((product) => product.id === ui.catalogProductId)) ?? null);
  const selected = $derived(runbooks.list.find((r) => r.slug === selectedSlug) ?? null);
  const activeCapabilities = $derived(proposal?.capabilities ?? []);
  const activePipeline = $derived(proposal?.pipeline ?? []);
  const activeCapability = $derived(activeCapabilities[0] ?? INTENTOS_CAPABILITIES[0]);
  const capabilityAgents = $derived(activePipeline.map((stage) => bySlug.get(stage.agent)).filter((agent): agent is Agent => Boolean(agent)));
  const teamReady = $derived(capabilityAgents.length === activePipeline.length);
  // Runtime engines are infrastructure, not a product decision. IntentOS
  // resolves the first healthy binding internally and keeps its identity out
  // of the human intention flow.
  const availableProviders = $derived(runs.providers.filter((p) => p.available));
  const provider = $derived(availableProviders[0] ?? null);
  const output = $derived(runs.events.filter((item) => item.event.kind === "output"));
  // A "follow-up" turn continues an already-chosen project: every further
  // instruction on it builds straight onto the isolated workspace without
  // asking for approval again. The gate that matters — writing to the real
  // project — still happens once, explicitly, at applyWorkspace().
  //
  // 2026-09-04: no longer gated on the `useExistingProject` checkbox — that
  // checkbox only ever controlled whether the project *picker* was visible
  // in the options panel, not whether a conversation exists. Requiring it
  // to reach the chat forced a "expand panel → check a box → pick from a
  // dropdown" ceremony before Esmeralda would even talk, which is exactly
  // what "quiero que sea un chat... ahí le declaro lo que quiero" objected
  // to. `projectPath` alone (set either by picking an existing project, or
  // auto-created the moment a build instruction needs one — see sendTurn)
  // is the only real signal of "is there a project-scoped conversation".
  const followUp = $derived(Boolean(projectPath));
  /** What the chat panel actually renders — the persisted, real
   *  conversation once a project exists, or the ephemeral pre-project
   *  chat before that. See `localMessages`'s own doc comment. */
  const chatMessages = $derived(projectPath ? session.messages : localMessages);
  const sending = $derived(resolvingProposal || approving || runs.starting);
  const busy = $derived(runs.current?.status === "running" || runs.current?.status === "queued");
  let draining = $state(false);
  let summarizedRunId = $state<string | null>(null);

  $effect(() => { if (!selectedSlug && runbooks.list.length) selectedSlug = runbooks.list[0].slug; });
  $effect(() => {
    const productId = ui.catalogProductId;
    if (productId === loadedCatalogProductId) return;
    loadedCatalogProductId = productId;
    catalogDraftActive = false;
    intent = "";
    proposal = null;
    previousProposal = null;
    proposalChanges = [];
    proposalRevision = 0;
    validation = "";
  });
  $effect(() => {
    if (!runs.current) return;
    projectPath = runs.current.projectPath;
    useExistingProject = true;
    if (runbooks.list.some((rb) => rb.slug === runs.current?.runbookId)) selectedSlug = runs.current.runbookId;
  });
  // Auto-scroll the chat to the newest message — 2026-09-05: "que la
  // ventana del chat vaya subiendo a medida va avanzando el chat". Reads
  // chatMessages/queue/busy purely to re-trigger on every new bubble; the
  // actual scroll always jumps straight to the bottom, matching how every
  // other chat (this one included) behaves.
  $effect(() => {
    void chatMessages.length;
    void (projectPath ? session.queue.length : 0);
    void busy;
    if (chatLogEl) chatLogEl.scrollTop = chatLogEl.scrollHeight;
  });
  // Showroom: the moment a run's isolated workspace exists, show it living —
  // never the original project. Re-fires per new workspace so a follow-up
  // turn's fresh copy replaces the previous turn's preview automatically.
  $effect(() => {
    const workspacePath = runs.current?.workspacePath;
    if (workspacePath && workspacePath !== preview.workspacePath) void preview.start(workspacePath);
  });
  // Esmeralda's memory follows the project, not the window: the moment a
  // project is in conversation, load (or start) its persisted session so
  // the message history — and the workspace it's already evolved — comes
  // back exactly as it was, whether that's from a minute ago or from
  // before IntentOS was last closed.
  $effect(() => {
    const path = followUp ? projectPath : "";
    if (path) {
      if (session.current?.projectPath !== path) void session.loadOrCreate(path);
    } else if (session.current) {
      session.clear();
    }
  });
  // Esmeralda reports back once a turn actually finishes — persisted into
  // the conversation (not just shown in the run log) so it becomes context
  // for the *next* turn via buildConversationalIntent, and survives a
  // restart like every other message.
  $effect(() => {
    const run = runs.current;
    if (!run?.sessionId) return;
    if (run.status !== "succeeded" && run.status !== "failed" && run.status !== "cancelled") return;
    if (summarizedRunId === run.id) return;
    summarizedRunId = run.id;
    void narrateRunForEsmeralda(run, session.messages).then((message) =>
      session.appendMessage(run.projectPath, "esmeralda", message, run.id),
    );
  });
  // The composer never blocks: a message sent while Esmeralda is already
  // building goes to `session.queue` instead (see sendChatMessage). This is
  // what actually processes that queue, one instruction at a time, the
  // moment the active turn reaches a terminal state.
  $effect(() => {
    if (!followUp || busy || draining || sending) return;
    if (session.queue.length === 0) return;
    void drainQueue();
  });

  function productionBrief(sourceProposal: SolutionProposal | null = proposal): string {
    const capabilities = sourceProposal?.capabilities ?? activeCapabilities;
    const pipeline = sourceProposal?.pipeline ?? activePipeline;
    const sections = [
      ["INTENCIÓN DEL PRODUCTO", intent],
      ["CAPACIDADES INTENTOS ACTIVADAS", capabilities.map((capability) => `${capability.label}: ${capability.description}`).join("\n")],
      ["PIPELINE ESPECIALISTA", pipeline.map((stage, index) => `${index + 1}. ${stage.label}: ${bySlug.get(stage.agent)?.name ?? stage.agent}`).join("\n")],
      ["ESTÁNDAR INTERNO INTENTOS", DEFAULT_SIGNATURE],
      ["ARCHIVOS ADJUNTOS AUTORIZADOS", attachments.map((path) => `- ${path}`).join("\n")],
      ["RESTRICCIONES Y FUERA DE ALCANCE", constraints],
      ["CRITERIOS DE APROBACIÓN", acceptance],
    ].filter(([, value]) => value.trim());
    return sections.map(([heading, value]) => `${heading}:\n${value.trim()}`).join("\n\n");
  }

  async function addAttachments() {
    try {
      const picked = await openDialog({ multiple: true, directory: false, title: "Adjuntar activos al brief de producción", filters: [{ name: "Activos de producción", extensions: ["png", "jpg", "jpeg", "webp", "svg", "pdf", "docx", "txt", "md", "csv", "xlsx", "json"] }] });
      const paths = typeof picked === "string" ? [picked] : (picked ?? []);
      attachments = [...new Set([...attachments, ...paths])];
    } catch (e) { toast.error("No se pudieron adjuntar los archivos", readableError(e)); }
  }

  function fileName(path: string): string { return path.split(/[\\/]/).pop() ?? path; }
  function removeAttachment(path: string) { attachments = attachments.filter((item) => item !== path); }

  function newIntent() {
    if (runs.current?.status === "running" || runs.current?.status === "queued") return;
    intent = "";
    attachments = [];
    constraints = "";
    acceptance = "";
    projectPath = "";
    useExistingProject = false;
    localMessages = [];
    resetDraftSession();
    pendingBuildProposal = null;
    proposal = null;
    previousProposal = null;
    proposalChanges = [];
    proposalRevision = 0;
    rejectionReason = "";
    rejecting = false;
    validation = "";
    catalogDraftActive = false;
    ui.clearCatalogProduct();
    runs.clearCurrent();
    session.clear();
    void preview.stop();
    void publicPreview.stop();
    toast.success("Nueva intención preparada");
  }

  function startCatalogProduct() {
    if (!catalogProduct) return;
    intent = catalogProduct.intentTemplate;
    catalogDraftActive = true;
    proposal = null;
    validation = "";
  }

  async function prepareProposal() {
    validation = "";
    if (!intent.trim()) validation = "Describe la intención del producto.";
    if (validation) return;
    resolvingProposal = true;
    try {
      const nextProposal = await planSolutionAsync({ intent, constraints, acceptance, requiredCapabilityIds: catalogProduct?.capabilityIds });
      proposalChanges = previousProposal ? proposalDiff(previousProposal, nextProposal) : [];
      proposal = nextProposal;
      proposalRevision += 1;
      rejecting = false;
      rejectionReason = "";
    } finally {
      resolvingProposal = false;
    }
  }

  function rejectProposal() {
    if (!rejectionReason.trim()) {
      validation = "Explica qué debe cambiar antes de generar otra propuesta.";
      return;
    }
    previousProposal = proposal;
    constraints = [constraints.trim(), `OBSERVACIÓN NCTO (propuesta v${proposalRevision} rechazada): ${rejectionReason.trim()}`].filter(Boolean).join("\n");
    proposal = null;
    proposalChanges = [];
    rejecting = false;
    toast.success("Observación incorporada; ajusta la intención o genera una nueva propuesta");
  }

  function proposalDiff(previous: SolutionProposal, next: SolutionProposal): string[] {
    const changes: string[] = [];
    if (previous.user !== next.user) changes.push(`Usuario redefinido: ${next.user}`);
    if (previous.job !== next.job) changes.push("Trabajo y alcance reformulados según la observación NCTO.");
    if (previous.outcome !== next.outcome) changes.push("Resultado verificable actualizado.");
    const beforeCapabilities = previous.capabilities.map((item) => item.id).join(",");
    const afterCapabilities = next.capabilities.map((item) => item.id).join(",");
    if (beforeCapabilities !== afterCapabilities) changes.push("Capacidades internas y equipo recompuestos.");
    if (next.revisionNotes.length > previous.revisionNotes.length) changes.push("Nueva observación NCTO incorporada al contrato de construcción.");
    return changes;
  }

  async function approveAndStart() {
    validation = "";
    if (!proposal) { validation = "La propuesta debe estar disponible."; return; }
    if (!teamReady) { validation = "El catálogo activo no contiene todos los especialistas internos requeridos."; return; }
    if (useExistingProject && !projectPath) { validation = "Selecciona el proyecto existente."; return; }
    approving = true;
    try {
      // Recompute the approved plan at the decision boundary. This prevents a
      // stale HMR-era proposal from ever starting an incomplete pipeline.
      const approvedProposal = await planSolutionAsync({ intent, constraints, acceptance, requiredCapabilityIds: catalogProduct?.capabilityIds });
      proposal = approvedProposal;
      const approvedCapabilities = approvedProposal.capabilities;
      const approvedPipeline = approvedProposal.pipeline;
      if (!approvedCapabilities.length || approvedPipeline.length < 5) {
        throw new Error("El Router no pudo componer el equipo mínimo de producción. Vuelve a visualizar la propuesta.");
      }
      if (!useExistingProject && !projectPath) {
        const created = await invoke<AutomaticProject>("project_create_automatic", { request: { suggestedName: approvedProposal.projectSlug } });
        projectPath = created.path;
        projects.register(created.path);
      }
      // Every build now belongs to that project's conversation with
      // Esmeralda — including a brand-new project's very first turn, so
      // its workspace is already the one every follow-up turn will reuse
      // (see resolve_session_workspace in runtime.rs). `priorMessages` is
      // captured *before* this turn's own message is appended, so
      // buildConversationalIntent doesn't echo the instruction back to
      // itself as "previous" context.
      await session.loadOrCreate(projectPath);
      const priorMessages = session.messages;
      await session.appendMessage(projectPath, "user", intent.trim());
      const sessionId = session.current?.id ?? null;
      const criteria = [acceptance.trim(), "La solución debe ser operable y no presentar mocks como terminados.", "QA y Reality Check deben aportar evidencia verificable.", "Lo construido debe corresponder a la propuesta aprobada."].filter(Boolean);
      const draft = await invoke<Mission>("mission_create", { request: {
        projectPath,
        objective: intent.trim(),
        scopeStatement: `PROPUESTA NCTO v${proposalRevision}\nForma recomendada: ${approvedProposal.solutionForm}\nUsuario: ${approvedProposal.user}\nTrabajo: ${approvedProposal.job}\nResultado: ${approvedProposal.outcome}\n\n${productionBrief(approvedProposal)}`,
        exclusions: constraints.split(/\r?\n/).map((item) => item.trim()).filter(Boolean),
        acceptanceCriteria: criteria,
        clientLocale: "es-CL",
        targetMarkets: [],
        deliveryLocales: ["es-CL"],
        agencyJurisdiction: "CL",
        engagementRegime: "fixed",
        adjustmentBudget: null,
        changePolicyNote: "Cualquier cambio respecto de la propuesta aprobada requiere una nueva decisión NCTO.",
      } });
      // Prefers Temporal's durable record; falls back to a local decision
      // (never blocking on Temporal being reachable) inside mission_approve
      // itself. `mission.approvalChannel` reports which one actually ran.
      const mission = await invoke<Mission>("mission_approve", {
        missionId: draft.id,
        proposalRevision,
      });
      if (mission.approvalChannel === "localFallback") {
        toast.warning(
          "Producción aprobada sin Temporal",
          "Temporal no está disponible ahora mismo; IntentOS registró tu aprobación localmente y sigue igual.",
        );
      }
      const brief = productionBrief(approvedProposal);
      const conversationalIntent = buildConversationalIntent(priorMessages, brief);
      await runs.start({ intent: conversationalIntent, projectPath, runbookId: selected?.slug ?? DEFAULT_RUNBOOK_ID, capabilityId: approvedCapabilities[0].id, capabilityIds: approvedCapabilities.map((item) => item.id), stageIds: approvedPipeline.map((stage) => stage.id), stageKinds: approvedPipeline.map((stage) => stage.kind), stageLabels: approvedPipeline.map((stage) => stage.label), agentSlugs: approvedPipeline.map((stage) => stage.agent), providerId: provider?.id ?? null, missionId: mission.id, sessionId });
    } catch (e) { toast.error("No se pudo iniciar la producción aprobada", readableError(e)); }
    finally { approving = false; }
  }

  /** A chat turn on a project already in conversation: compute the
   *  proposal and immediately build, no manual "Aprobar y construir" click.
   *  Reuses prepareProposal()/approveAndStart() untouched — this only
   *  chains them and clears the composer once the turn actually started.
   *
   *  2026-09-04 — real chat with Esmeralda: a message classified as plain
   *  conversation (`isConversationalMessage`) never reaches
   *  prepareProposal/Mission/runs.start at all — Esmeralda just replies,
   *  the same way a person would say "hola" back before you describe what
   *  you actually want built. Only gated on `projectPath` already existing
   *  (an established conversation) — a brand-new project's very first
   *  message keeps today's behavior unchanged, since deciding whether a
   *  first-ever message should even create a project is a different,
   *  bigger question this change doesn't attempt to answer. */
  function appendLocalMessage(role: "user" | "esmeralda", content: string) {
    localMessages = [
      ...localMessages,
      { id: crypto.randomUUID(), role, content, at: new Date().toISOString() },
    ];
    // Persist alongside the in-memory push (see DRAFT_SESSION_ID_KEY above)
    // so a restart mid-conversation no longer loses it. Best-effort: a
    // failure here (Paranoid Mode, disk issue) still leaves the message
    // visible for the rest of this session via `localMessages` — it just
    // won't survive a restart, same as before this fix existed.
    void session.appendMessage(draftSessionPath(), role, content);
  }

  /** The single place a conversational (non-build) message becomes
   *  Esmeralda's actual reply — used for both the persisted (real project)
   *  and ephemeral (no project yet) chat paths, so the team-composition
   *  grounding below only has to exist once.
   *
   *  2026-09-05 requirement: a hypothetical "qué equipo convocarías"
   *  question must be answered from IntentOS's real agent catalog, never
   *  invented generic roles. `planSolutionAsync` — the exact same routing
   *  a real build would use — is called here purely informationally: it
   *  only reads the catalog and computes a pipeline in memory, it never
   *  touches disk or creates anything, so this stays the side-effect-free
   *  query the Constitución requires for planning/consulting. */
  async function computeEsmeraldaChatReply(
    text: string,
    priorMessages: ConversationMessage[],
  ): Promise<string> {
    if (isTeamCompositionQuestion(text)) {
      const hypothetical = await planSolutionAsync({ intent: text });
      const roster = hypothetical.pipeline.map((stage) => ({
        label: stage.label,
        agentName: bySlug.get(stage.agent)?.name ?? stage.agent,
      }));
      return draftTeamExplanation(text, roster, priorMessages);
    }
    return replyToEsmeraldaChat(text, priorMessages);
  }

  /** The single dispatcher for a chat turn — 2026-09-05 cognitive
   *  architecture mandate. Replaces the earlier if/else chain of regex
   *  booleans (`isConversationalMessage`/`isBuildNegation`/
   *  `isExplicitConfirmation` checked in a specific, fragile order) with
   *  one structured comprehension step (`classifyConversationalIntent`)
   *  followed by a small, deterministic decision table. The model
   *  interprets (negation, hypothesis, confirmation, restrictions); this
   *  function decides and acts — it is the only place authority actually
   *  lives, exactly as much as it did before this mandate.
   *
   *  `act === "execute"` is the only branch that can ever reach
   *  `approveAndStart` (a real project_create_automatic + Mission +
   *  production run). Every other act — including "confirm" with nothing
   *  pending, or "unclear" — falls through to a conversational reply and
   *  touches nothing on disk. */
  async function sendTurn(text: string) {
    const priorMessages = projectPath ? session.messages : localMessages;
    const decision = await classifyConversationalIntent(text, priorMessages, pendingBuildProposal);

    if (decision.act === "confirm" && !projectPath && pendingBuildProposal) {
      const agreed = pendingBuildProposal;
      pendingBuildProposal = null;
      appendLocalMessage("user", text);
      intent = agreed.restrictions.length
        ? `${agreed.text}\n\nRESTRICCIONES EXPLÍCITAS DEL USUARIO: ${agreed.restrictions.join("; ")}`
        : agreed.text;
      await prepareProposal();
      if (validation || !proposal) return;
      await approveAndStart();
      if (!validation && !runs.error) intent = "";
      return;
    }

    if (decision.act !== "execute") {
      // Every non-executive act (explain/plan/smalltalk/confirm-with-
      // nothing-pending/cancel/unclear) is conversation, never a build.
      // "cancel" additionally clears whatever was pending — there is
      // nothing left to confirm once the human explicitly refused it (the
      // real infinite-loop bug this mandate started from).
      if (decision.act === "cancel") pendingBuildProposal = null;
      if (projectPath) {
        await session.loadOrCreate(projectPath);
        const prior = session.messages;
        await session.appendMessage(projectPath, "user", text);
        const reply = await computeEsmeraldaChatReply(text, prior);
        await session.appendMessage(projectPath, "esmeralda", reply);
      } else {
        // No project exists yet — this is chat before any work has
        // started, so there is nothing to persist it against. Kept purely
        // in-memory (localMessages); the moment a later message actually
        // needs a project, the conversation continues in that project's
        // real, persisted session instead.
        const prior = localMessages;
        appendLocalMessage("user", text);
        const reply = await computeEsmeraldaChatReply(text, prior);
        appendLocalMessage("esmeralda", reply);
      }
      return;
    }

    // act === "execute". Inside an already-agreed project, every further
    // instruction still builds immediately — the human already committed
    // to this project once, so re-confirming every single turn would be
    // exactly the ceremony the chat redesign was meant to remove.
    if (projectPath) {
      intent = text;
      await prepareProposal();
      if (validation || !proposal) return;
      await approveAndStart();
      if (!validation && !runs.error) intent = "";
      return;
    }
    // No project exists yet — 2026-09-05: "que las carpetas se creen
    // cuando el mandato sea explícito, ese chat se puede ver como una
    // reunión de acuerdo". Creating a project is a real filesystem
    // commitment (project_create_automatic inside approveAndStart), so the
    // first build-sounding message opens a discussion instead of
    // triggering it. Either the first mention of wanting something built,
    // or a refinement of what was already discussed — either way, still
    // just conversation until an explicit go-ahead arrives. Replacing (not
    // appending to) pendingBuildProposal on every pass means the *latest*
    // description is always what actually gets built once confirmed.
    pendingBuildProposal = { text, restrictions: decision.restrictions };
    const priorLocal = localMessages;
    appendLocalMessage("user", text);
    const reply = await draftBuildConfirmation(text, priorLocal);
    appendLocalMessage("esmeralda", reply);
  }

  /** The chat composer's send action: never blocks on a build in progress.
   *  Busy → the instruction goes into `session.queue` (shown immediately as
   *  a "en cola" bubble — see the template) and `drainQueue` (driven by the
   *  effect above) starts it for real the moment the active turn ends,
   *  which is also when it's actually persisted as a message (inside
   *  approveAndStart, once for every turn — queued or not — so a queued
   *  instruction is never recorded twice). Idle → starts immediately,
   *  same as today. */
  async function sendChatMessage() {
    const text = intent.trim();
    if (!text) return;
    intent = "";
    if (busy || session.queue.length > 0) {
      session.enqueue(text);
      return;
    }
    await sendTurn(text);
  }

  async function drainQueue() {
    draining = true;
    try {
      const next = session.dequeue();
      if (next) await sendTurn(next);
    } finally {
      draining = false;
    }
  }

  function readableError(error: unknown): string {
    if (error instanceof Error && error.message.trim()) return error.message;
    if (typeof error === "string") return error;
    if (error && typeof error === "object") {
      const record = error as Record<string, unknown>;
      for (const key of ["message", "error", "reason", "detail"]) {
        if (typeof record[key] === "string" && record[key].trim()) return record[key];
      }
      try { return JSON.stringify(error); } catch { /* fall through */ }
    }
    return "Error desconocido del motor nativo.";
  }
  function eventText(event: RunEvent): string {
    if (event.kind === "output") return event.text;
    if (event.kind === "gatePassed") return `Gate aprobado: ${event.stageId}`;
    if (event.kind === "gateFailed") return `Gate fallido: ${event.stageId} — ${event.reason}`;
    return `Estado: ${event.run.status}`;
  }
  async function reviewWorkspace() {
    try { await runs.reviewCurrent(); }
    catch (e) { toast.error("No se pudo comparar la copia de trabajo", readableError(e)); }
  }
  async function applyWorkspace() {
    if (!runs.review) await reviewWorkspace();
    if (!runs.review || !runs.review.sourceUnchanged) return;
    const changeCount = runs.review.changes.length;
    if (!confirm(`Se aplicarán ${changeCount} cambios al proyecto original. IntentOS creará un backup recuperable antes de continuar. ¿Aplicar ahora?`)) return;
    try {
      await runs.applyCurrent();
      // The evolving workspace keeps living (and the Showroom keeps
      // pointing at it) — applying only copies its current state onto the
      // protected original, it does not end the conversation.
      if (projectPath) await session.appendMessage(projectPath, "system", `${changeCount} cambios aplicados al proyecto original, con backup de seguridad.`);
      toast.success("Cambios aplicados con backup de seguridad");
    } catch (e) { toast.error("No se pudieron aplicar los cambios", readableError(e)); }
  }
  async function loadDeliveryReceipt() {
    try { await runs.loadReceipt(); }
    catch (e) { toast.error("No se pudo construir el comprobante de entrega", readableError(e)); }
  }
  async function discardWorkspace() {
    if (!confirm("Se eliminará toda la copia de trabajo de esta conversación (todos los turnos aún no aplicados). El proyecto original y los registros de evidencia se conservarán. La próxima instrucción partirá de una copia nueva del proyecto original. ¿Descartar copia?")) return;
    try {
      // Stop the preview dev-server BEFORE trying to delete the directory
      // it's serving — real bug found live 2026-09-06: on Windows, a still-
      // running preview process holds file handles open inside the
      // workspace, so `remove_dir_all` fails with "access denied" and the
      // discard silently never completes. The next turn then builds a
      // fresh workspace copy on top of the one that was never actually
      // removed, which is what looked like IntentOS "still making fake
      // copies" no matter how many times discard was pressed.
      await preview.stop();
      await publicPreview.stop();
      await runs.discardCurrentWorkspace();
      if (projectPath) {
        await session.appendMessage(projectPath, "system", "Copia de trabajo descartada. La próxima instrucción parte de una copia nueva del proyecto original.");
        await session.loadOrCreate(projectPath); // resync: backend cleared the session's workspace pointer too
      }
      toast.success("Copia de trabajo descartada");
    } catch (e) { toast.error("No se pudo descartar la copia", readableError(e)); }
  }
</script>

<section class="workspace">
  <header class="head"><div><h1>{catalogProduct && !catalogDraftActive ? "Catálogo de Creación" : followUp ? "Esmeralda" : "IntentOS Production"}</h1><p>{catalogProduct && !catalogDraftActive ? `${catalogArea?.number} — ${catalogArea?.name}` : followUp ? `Conversación de trabajo · ${projectPath.split(/[\\/]/).pop()}` : "De una intención humana a una entrega tecnológica verificada."}</p></div><div class="head-actions"><Button variant="secondary" onclick={newIntent} disabled={runs.current?.status === "running" || runs.current?.status === "queued"} ariaLabel="Crear nueva intención">Nueva intención</Button></div></header>
  <div class="grid">
    <div class="composer">
      {#if catalogProduct && !catalogDraftActive}
        <article class="catalog-detail">
          <button class="catalog-back" type="button" onclick={() => ui.clearCatalogProduct()}><ArrowLeftIcon size={13}/> Volver al catálogo</button>
          <span class="eyebrow">{catalogArea?.number} · {catalogArea?.name}</span>
          <h2>{catalogProduct.name}</h2>
          <p class="catalog-description">{catalogProduct.description}</p>
          <section><h3>¿Para qué se usa?</h3><p>{catalogProduct.purpose}</p></section>
          <section><h3>Puede incluir</h3><ul>{#each catalogProduct.includes as item}<li><CheckIcon size={12}/><span>{item}</span></li>{/each}</ul></section>
          <section><h3>Para comenzar, IntentOS te pedirá</h3><ul>{#each catalogProduct.requiredInputs as item}<li><CheckIcon size={12}/><span>{item}</span></li>{/each}</ul></section>
          <div class="catalog-capabilities"><small>IntentOS compondrá internamente</small><p>{catalogProduct.capabilityIds.map((id) => INTENTOS_CAPABILITIES.find((capability) => capability.id === id)?.shortLabel ?? id).join(" + ")}</p></div>
          <Button variant="primary" onclick={startCatalogProduct} ariaLabel={`Crear ${catalogProduct.name}`}>Crear este producto</Button>
        </article>
      {:else}
      <div class="card">
        {#if catalogProduct}<div class="catalog-context"><span>PRODUCTO SELECCIONADO</span><strong>{catalogProduct.name}</strong><small>La intención es editable. IntentOS mantendrá las capacidades mínimas necesarias.</small></div>{/if}
        <label for="intent">¿Qué quieres que construya o resuelva IntentOS?</label>
        <textarea id="intent" bind:value={intent} rows="5" placeholder="Cuéntale a IntentOS qué quieres conseguir…" aria-describedby="intent-help validation" oninput={() => proposal = null} onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void sendChatMessage(); } }}></textarea>
        <p id="intent-help" class="hint">IntentOS resolverá internamente usuarios, objetivos, capacidades, criterios y plan de trabajo.</p>
        <details class="options" open={attachments.length > 0 || useExistingProject}>
          <summary>Archivos o proyecto existente <small>Opcional</small></summary>
          <div class="options-body">
            <div class="attachments">
              <div><strong>Archivos autorizados</strong><small>Activos o datos que IntentOS deba utilizar.</small></div>
              <Button variant="secondary" onclick={addAttachments} ariaLabel="Adjuntar archivos"><PaperclipIcon size={14}/> Adjuntar</Button>
              {#if attachments.length}<ul>{#each attachments as path (path)}<li title={path}><span>{fileName(path)}</span><button type="button" onclick={() => removeAttachment(path)} aria-label={`Quitar ${fileName(path)}`}><XIcon size={13}/></button></li>{/each}</ul>{/if}
            </div>
            <label class="existing-toggle"><input type="checkbox" bind:checked={useExistingProject}/> Continuar un proyecto existente</label>
            {#if useExistingProject}
              <div class="fields"><label for="project">Proyecto<select id="project" bind:value={projectPath}><option value="">Seleccionar proyecto</option>{#each projects.list as p (p.path)}<option value={p.path}>{p.label}</option>{/each}</select></label><Button variant="secondary" onclick={() => projects.addViaPicker()} ariaLabel="Añadir proyecto"><PlusIcon size={14}/> Añadir</Button></div>
            {/if}
          </div>
        </details>
        {#if validation}<p id="validation" class="error" role="alert">{validation}</p>{/if}
        <Button variant="primary" onclick={sendChatMessage} loading={sending && !busy} disabled={!intent.trim()} ariaLabel="Enviar a Esmeralda">Enviar</Button>
        <p class="hint">{busy ? "Esmeralda sigue trabajando en tu instrucción anterior — esta se procesará automáticamente en cuanto termine." : followUp ? "Esmeralda conserva el contexto de esta conversación y del proyecto. Cada instrucción se construye sobre la copia de trabajo acumulada; el proyecto original solo cambia cuando pides aplicar." : "Puedes charlar con Esmeralda o pedirle directamente que construya algo — ella decide cuál es cuál."}</p>
      </div>
      {/if}
    </div>

    <aside class="console" aria-live="polite">
      <div class="chat">
        <header class="chat-head"><span class="esmeralda-avatar" aria-hidden="true">E</span><div><strong>Esmeralda</strong><small>{projectPath && session.loading ? "Cargando conversación…" : `${chatMessages.length} mensajes`}</small></div></header>
        <ol class="chat-log" bind:this={chatLogEl}>
          {#each chatMessages as message (message.id)}
            <li class={`bubble ${message.role}`}>
              <span class="bubble-role">{message.role === "user" ? "Tú" : message.role === "esmeralda" ? "Esmeralda" : "Sistema"}</span>
              <p>{message.content}</p>
              <time>{new Date(message.at).toLocaleTimeString()}</time>
            </li>
          {/each}
          {#if projectPath}
            {#each session.queue as queued, index (index)}
              <li class="bubble user queued"><span class="bubble-role">Tú · en cola</span><p>{queued}</p></li>
            {/each}
          {/if}
          {#if busy}<li class="bubble esmeralda pending"><span class="bubble-role">Esmeralda</span><p>Trabajando en tu instrucción…</p></li>{/if}
          {#if !chatMessages.length && !(projectPath && session.queue.length) && !busy}<li class="bubble-empty">Escríbele a Esmeralda — puedes charlar o pedirle directamente que construya algo.</li>{/if}
        </ol>
      </div>
      {#if runs.current}
        <div class="run-head"><div><span class="eyebrow">EJECUCIÓN · {INTENTOS_CAPABILITIES.find((item) => item.id === runs.current?.capabilityId)?.shortLabel ?? runs.current.capabilityId}</span><h2>{runs.current.status}</h2><p title={runs.current.projectPath}>Origen protegido: {runs.current.projectPath}</p>{#if runs.current.workspacePath}<p title={runs.current.workspacePath}>Copia de trabajo: {runs.current.workspacePath}</p>{/if}</div>{#if runs.current.status === "running" || runs.current.status === "queued"}<Button variant="danger" onclick={() => runs.cancel()} loading={runs.cancelling}><SquareIcon size={13}/> Cancelar</Button>{/if}</div>
        {#if runs.current.workspacePath}
          <section class="showroom">
            <div class="showroom-head"><span class="eyebrow">SHOWROOM · VISTA PREVIA EN VIVO</span>{#if preview.url}<a href={preview.url} target="_blank" rel="noreferrer">{preview.url}</a>{/if}</div>
            {#if preview.url}
              <iframe class="showroom-frame" src={preview.url} title="Vista previa en vivo del proyecto"></iframe>
            {:else if preview.error}
              <p class="error">No se pudo levantar la vista previa: {preview.error}</p>
            {:else}
              <p class="hint">{preview.starting ? "Preparando vista previa…" : "Este proyecto no define un script \"dev\"; sin vista previa automática."}</p>
            {/if}
            {#if preview.url}
              <div class="public-preview">
                {#if publicPreview.url}
                  <p class="hint">Pública (anónima, sin cuenta): <a href={publicPreview.url} target="_blank" rel="noreferrer">{publicPreview.url}</a></p>
                  <Button variant="secondary" onclick={() => publicPreview.stop()} loading={publicPreview.stopping}>Detener publicación pública</Button>
                {:else}
                  <Button variant="secondary" onclick={() => publicPreview.start()} loading={publicPreview.starting} ariaLabel="Publicar vista previa pública">
                    {publicPreview.preparing ? "Preparando cloudflared…" : "Publicar vista previa pública"}
                  </Button>
                  <p class="hint">Genera una URL temporal accesible desde internet (tu celular, otro dispositivo, compartirla) apuntando a esta misma vista previa. Es anónima: cualquiera con el enlace puede verla mientras esté activa.</p>
                {/if}
                {#if publicPreview.error}<p class="error">No se pudo publicar: {publicPreview.error}</p>{/if}
              </div>
            {/if}
          </section>
        {/if}
        <ol class="stages">{#each runs.current.stages as stage (stage.id)}<li class:active={stage.status === "running"} aria-current={stage.status === "running" ? "step" : undefined}><span class={`dot ${stage.status}`}></span><div><strong>{stage.label}</strong><small>{stage.agentSlug} · intento {stage.attempt || 1}</small></div><b>{stage.status}</b></li>{/each}</ol>
        {#if runs.current.workspacePath && runs.current.status !== "running" && runs.current.status !== "queued"}
          <section class="workspace-review">
            <div class="review-actions"><Button variant="secondary" onclick={reviewWorkspace} loading={runs.reviewing}>Revisar cambios</Button>{#if runs.current.status === "succeeded"}<Button variant="secondary" onclick={loadDeliveryReceipt}>Comprobante</Button>{/if}<Button variant="primary" onclick={applyWorkspace} loading={runs.applying} disabled={!runs.review?.sourceUnchanged || !runs.review?.changes.length}>Aplicar al original</Button><Button variant="danger" onclick={discardWorkspace} loading={runs.discarding}>Descartar copia</Button></div>
            {#if runs.review}
              <p class:conflict={!runs.review.sourceUnchanged}>{runs.review.sourceUnchanged ? `${runs.review.changes.length} cambios detectados; el origen no cambió durante la ejecución.` : "El proyecto original cambió durante la ejecución. Aplicación bloqueada para evitar sobrescrituras."}</p>
              {#if runs.review.changes.length}<ul class="change-list">{#each runs.review.changes as change (`${change.kind}:${change.path}`)}<li><b class={change.kind}>{change.kind}</b><span title={change.path}>{change.path}</span></li>{/each}</ul>{/if}
            {/if}
            {#if runs.receipt}
              <div class="delivery-receipt"><strong>{runs.receipt.applied ? "Entrega aplicada" : "Entrega verificada"}</strong><span>{runs.receipt.gates.length} gates aprobados · evidencia {runs.receipt.evidenceComplete ? "completa" : "incompleta"}</span><small title={runs.receipt.evidencePath}>ID {runs.receipt.runId}</small></div>
            {/if}
          </section>
        {/if}
        <div class="log" role="log" aria-live="polite" aria-relevant="additions">{#if runs.events.length === 0}<p>Esperando actividad del runtime…</p>{:else}{#each runs.events as item (item.id)}<div><time>{new Date(item.at).toLocaleTimeString()}</time><pre>{eventText(item.event)}</pre></div>{/each}{/if}</div>
        {#if runs.current.terminalState?.state === "humanDecisionRequired"}
          <p class="notice run-decision"><strong>Necesito una decisión tuya:</strong> {runs.current.terminalState.detail} IntentOS no está roto — está esperando tu decisión o configuración para continuar.</p>
        {:else if runs.current.error}<p class="error run-error">{runs.current.error}</p>{/if}
      {:else if proposal && !followUp}
        <section class="proposal" aria-labelledby="proposal-title">
          <div class="proposal-head"><div><span class="eyebrow">PROPUESTA NCTO · v{proposalRevision}</span><h2 id="proposal-title">{proposal.title}</h2></div><span class="proposal-kind">{proposal.solutionForm}</span></div>
          <div class="proposal-summary"><article><small>LO QUE ENTENDIÓ INTENTOS</small><p>{proposal.problem}</p></article><article><small>RESULTADO COMPROBABLE</small><p>{proposal.outcome}</p></article></div>
          {#if proposal.revisionNotes.length}<section class="revision"><strong>Revisión NCTO incorporada</strong><p>{proposal.revisionNotes.at(-1)}</p>{#if proposalChanges.length}<ul>{#each proposalChanges as change}<li>{change}</li>{/each}</ul>{/if}</section>{/if}
          <details><summary>Ver criterio y plan interno</summary><div class="proposal-internal"><p><strong>Usuario inferido</strong><br/>{proposal.user}</p><p><strong>Trabajo a resolver</strong><br/>{proposal.job}</p><p><strong>Proyecto</strong><br/>Documents / IntentOS Projects / {proposal.projectSlug}</p><p><strong>Capacidades</strong><br/>{proposal.capabilities.map((item) => item.shortLabel).join(" + ")}</p><div class="experience-map"><strong>Experiencia propuesta</strong>{#each proposal.experience as step, index}<div><span>{index + 1}</span><p>{step}</p></div>{/each}</div><ol class="dynamic-team">{#each proposal.pipeline as stage, index (stage.id)}<li class:missing={!bySlug.has(stage.agent)}><div><strong>{index + 1}. {stage.label}</strong><small>Responsable: {bySlug.get(stage.agent)?.name ?? stage.agent}</small></div></li>{/each}</ol></div></details>
          {#if rejecting}<label for="rejection">¿Qué debe cambiar?<textarea id="rejection" bind:value={rejectionReason} rows="4" placeholder="Explica por qué rechazas esta propuesta y qué dirección debe tomar IntentOS."></textarea></label>{/if}
          <div class="decision-actions"><Button variant="primary" onclick={approveAndStart} loading={approving} disabled={!teamReady}>Aprobar y construir</Button>{#if rejecting}<Button variant="danger" onclick={rejectProposal}>Enviar rechazo razonado</Button>{:else}<Button variant="secondary" onclick={() => rejecting = true}>Rechazar / pedir cambios</Button>{/if}<Button variant="secondary" onclick={() => proposal = null}>Editar intención</Button></div>
          {#if !provider}<p class="error">El runtime no está disponible; puedes revisar la propuesta, pero no iniciar producción.</p>{/if}
        </section>
      {/if}
    </aside>
  </div>
</section>

<style>
  .workspace{height:100%;display:flex;flex-direction:column;min-height:0;overflow:hidden}.head{flex:0 0 auto;padding:var(--space-3) var(--space-4);border-bottom:1px solid var(--color-border);display:flex;justify-content:space-between;gap:16px;align-items:center}.head h1{font-size:var(--text-h2)}.head p,.hint{color:var(--color-text-secondary);font-size:var(--text-body-sm)}.grid{flex:1;min-height:0;overflow:hidden;padding:var(--space-3);display:grid;grid-template-columns:minmax(340px,460px) minmax(380px,1fr);gap:var(--space-3)}.composer,.console{min-height:0;overflow:auto;scrollbar-gutter:stable}.composer{display:flex;flex-direction:column;gap:var(--space-3)}.card,.console{background:var(--color-surface-raised);border:1px solid var(--color-border);border-radius:var(--radius-lg)}.card{padding:var(--space-4);display:flex;flex-direction:column;gap:10px}label{font-size:var(--text-body-sm);font-weight:var(--fw-semibold);display:flex;flex-direction:column;gap:5px}textarea,select{width:100%;border:1px solid var(--color-border);border-radius:var(--radius-md);background:var(--color-surface);color:var(--color-text-primary);padding:9px;font:inherit}textarea{resize:vertical}.attachments{display:grid;grid-template-columns:1fr auto;gap:8px;align-items:center;padding:9px;border:1px dashed var(--color-border);border-radius:var(--radius-md)}.attachments>div{display:flex;flex-direction:column}.attachments small{font-size:11px;color:var(--color-text-muted)}.attachments ul{grid-column:1/-1;display:flex;flex-direction:column;gap:4px;list-style:none}.attachments li{display:flex;justify-content:space-between;align-items:center;gap:8px;padding:5px 7px;border-radius:var(--radius-sm);background:var(--color-surface-raised);font-size:11px}.attachments li span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.attachments li button{display:flex;color:var(--color-text-muted)}.fields{display:grid;grid-template-columns:1fr auto;align-items:end;gap:8px}.error{font-size:var(--text-body-sm);color:var(--color-danger)}h2{font-size:var(--text-h3)}.console{padding:var(--space-4);display:flex;flex-direction:column;gap:var(--space-3)}.run-head{display:flex;justify-content:space-between;gap:10px}.run-head p{font-size:11px;color:var(--color-text-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:440px}.eyebrow{font-size:10px;color:var(--color-brand);letter-spacing:.08em}.stages{list-style:none;display:flex;flex-direction:column;gap:6px}.stages li{display:grid;grid-template-columns:12px 1fr auto;align-items:center;gap:9px;padding:8px;border-radius:var(--radius-md);background:var(--color-surface)}.stages li.active{outline:1px solid var(--color-brand)}.stages small{display:block;color:var(--color-text-muted);font-size:11px}.stages b{font-size:10px;text-transform:uppercase}.dot{width:9px;height:9px;border-radius:50%;background:var(--color-border)}.dot.running{background:var(--color-brand)}.dot.passed{background:var(--color-success)}.dot.failed{background:var(--color-danger)}.log{flex:1;min-height:180px;overflow:auto;background:var(--color-surface-sunken);border-radius:var(--radius-md);padding:10px}.log div{display:grid;grid-template-columns:76px 1fr;gap:8px;border-bottom:1px solid var(--color-border);padding:5px 0}.log time{font:10px var(--font-mono);color:var(--color-text-muted)}.log pre{white-space:pre-wrap;word-break:break-word;font:11px/1.45 var(--font-mono);color:var(--color-text-secondary)}.run-error{padding:8px;background:color-mix(in srgb,var(--color-danger) 10%,transparent);border-radius:var(--radius-md)}.notice{font-size:var(--text-body-sm);color:var(--color-warning)}.run-decision{padding:8px;background:color-mix(in srgb,var(--color-warning) 10%,transparent);border-radius:var(--radius-md)}button{cursor:pointer}@media(max-width:820px){.workspace{overflow:auto}.grid{overflow:visible;grid-template-columns:1fr}.composer,.console{overflow:visible}.console{min-height:420px}.head{align-items:flex-start;flex-direction:column}.fields{grid-template-columns:1fr}.recipe-actions{justify-content:flex-start;flex-wrap:wrap}}@media(prefers-reduced-motion:reduce){*{scroll-behavior:auto!important}}
  .dynamic-team{display:grid;grid-template-columns:1fr 1fr;gap:5px;list-style:none}.dynamic-team li{padding:7px;border-radius:var(--radius-sm);background:var(--color-surface-raised)}.dynamic-team li div{min-width:0;display:flex;flex-direction:column}.dynamic-team strong,.dynamic-team small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.dynamic-team strong{font-size:10px;color:var(--color-text-primary)}.dynamic-team small{font-size:9px;color:var(--color-text-muted)}.dynamic-team li.missing{outline:1px solid var(--color-danger)}@media(max-width:520px){.dynamic-team{grid-template-columns:1fr}}
  .head-actions{display:flex;align-items:center;gap:8px;flex-wrap:wrap}
  .showroom{display:flex;flex-direction:column;gap:7px;padding:10px;border:1px solid var(--color-border);border-radius:var(--radius-md);background:var(--color-surface)}.showroom-head{display:flex;justify-content:space-between;align-items:center;gap:10px}.showroom-head a{font-size:10px;color:var(--color-brand);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.showroom-frame{width:100%;height:280px;border:1px solid var(--color-border);border-radius:var(--radius-sm);background:var(--color-surface-sunken)}
  .public-preview{display:flex;flex-direction:column;gap:6px;padding-top:7px;border-top:1px dashed var(--color-border)}.public-preview a{color:var(--color-brand);word-break:break-all}
  .dot.succeeded{background:var(--color-success)}.dot.cancelled{background:var(--color-text-muted)}
  .chat{display:flex;flex-direction:column;gap:8px;padding-bottom:var(--space-3);margin-bottom:var(--space-3);border-bottom:1px solid var(--color-border)}.chat-head{display:flex;align-items:center;gap:9px}.esmeralda-avatar{flex:none;width:26px;height:26px;display:grid;place-items:center;border-radius:50%;background:var(--color-brand);color:var(--color-surface);font-size:12px;font-weight:700}.chat-head strong{font-size:12px;color:var(--color-text-primary)}.chat-head small{font-size:10px;color:var(--color-text-muted)}.chat-log{list-style:none;display:flex;flex-direction:column;gap:8px;max-height:320px;overflow-y:auto;padding-right:2px}.bubble{max-width:88%;padding:8px 10px;border-radius:var(--radius-md);background:var(--color-surface)}.bubble.user{align-self:flex-end;background:color-mix(in srgb,var(--color-brand) 14%,var(--color-surface))}.bubble.esmeralda{align-self:flex-start}.bubble.system{align-self:center;max-width:96%;background:transparent;border:1px dashed var(--color-border)}.bubble.queued{opacity:.6}.bubble.pending{font-style:italic;color:var(--color-text-muted)}.bubble-role{display:block;font-size:9px;letter-spacing:.06em;text-transform:uppercase;color:var(--color-brand);margin-bottom:2px}.bubble.system .bubble-role{color:var(--color-text-muted)}.bubble p{font-size:12px;line-height:1.5;color:var(--color-text-primary);white-space:pre-wrap}.bubble time{display:block;margin-top:3px;font-size:9px;color:var(--color-text-muted)}.bubble-empty{font-size:11px;color:var(--color-text-muted);text-align:center;padding:10px}
  .options{border:1px solid var(--color-border);border-radius:var(--radius-md);background:var(--color-surface)}.options>summary{display:flex;justify-content:space-between;gap:8px;padding:9px;cursor:pointer;font-size:11px;font-weight:var(--fw-semibold)}.options>summary small{color:var(--color-text-muted);font-weight:400}.options-body{display:flex;flex-direction:column;gap:10px;padding:0 9px 9px}.existing-toggle{display:flex;flex-direction:row;align-items:center;gap:8px}.existing-toggle input{width:auto}.proposal{display:flex;flex-direction:column;gap:12px;min-height:0;overflow:auto}.proposal-head{display:flex;justify-content:space-between;gap:12px;align-items:flex-start}.proposal-kind{max-width:220px;padding:5px 8px;border-radius:99px;background:color-mix(in srgb,var(--color-brand) 14%,transparent);color:var(--color-brand);font-size:10px;text-align:center}.proposal-summary{display:grid;grid-template-columns:1fr 1fr;gap:8px}.proposal-summary article{padding:10px;border-radius:var(--radius-md);background:var(--color-surface)}.proposal-summary small{font-size:9px;letter-spacing:.08em;color:var(--color-brand)}.proposal-summary p,.proposal-internal p,.experience-map p{font-size:11px;color:var(--color-text-secondary);white-space:pre-wrap}.proposal-internal{display:flex;flex-direction:column;gap:9px;padding-top:9px}.proposal-internal>p{padding:8px;border-radius:var(--radius-sm);background:var(--color-surface)}.experience-map{display:flex;flex-direction:column;gap:6px}.experience-map div{display:grid;grid-template-columns:24px 1fr;gap:8px;align-items:center;padding:8px;background:var(--color-surface);border-radius:var(--radius-md)}.experience-map span{display:grid;place-items:center;width:22px;height:22px;border-radius:50%;background:color-mix(in srgb,var(--color-brand) 16%,transparent);color:var(--color-brand);font-size:10px;font-weight:700}.proposal details{padding:9px;border:1px solid var(--color-border);border-radius:var(--radius-md)}.proposal summary{cursor:pointer;font-size:11px;color:var(--color-text-secondary)}.decision-actions{display:flex;gap:8px;flex-wrap:wrap}
  .revision{padding:11px;border:1px solid color-mix(in srgb,var(--color-brand) 45%,var(--color-border));border-radius:var(--radius-md);background:color-mix(in srgb,var(--color-brand) 7%,var(--color-surface))}.revision p,.revision li{font-size:11px;color:var(--color-text-secondary)}.revision ul{margin:7px 0 0 18px;display:flex;flex-direction:column;gap:3px}
  .workspace-review{display:flex;flex-direction:column;gap:8px;padding:9px;border:1px solid var(--color-border);border-radius:var(--radius-md);background:var(--color-surface)}.review-actions{display:flex;gap:7px;flex-wrap:wrap}.workspace-review p{font-size:11px;color:var(--color-success)}.workspace-review p.conflict{color:var(--color-danger)}.delivery-receipt{display:flex;flex-direction:column;gap:2px;padding:9px;border-radius:var(--radius-md);background:color-mix(in srgb,var(--color-success) 10%,var(--color-surface));color:var(--color-success)}.delivery-receipt span,.delivery-receipt small{font-size:10px;color:var(--color-text-secondary)}.change-list{max-height:160px;overflow:auto;list-style:none;display:flex;flex-direction:column;gap:3px}.change-list li{display:grid;grid-template-columns:58px 1fr;gap:7px;font:10px var(--font-mono)}.change-list span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.change-list b{text-transform:uppercase}.change-list b.added{color:var(--color-success)}.change-list b.modified{color:var(--color-brand)}.change-list b.removed{color:var(--color-danger)}
  .catalog-detail{padding:var(--space-4);display:flex;flex-direction:column;gap:14px;background:var(--color-surface-raised);border:1px solid var(--color-border);border-radius:var(--radius-lg)}.catalog-detail h2{font-size:22px}.catalog-description{font-size:13px;line-height:1.55;color:var(--color-text-secondary)}.catalog-detail section{padding-top:11px;border-top:1px solid var(--color-border)}.catalog-detail h3{margin-bottom:6px;font-size:11px;text-transform:uppercase;letter-spacing:.06em;color:var(--color-text-muted)}.catalog-detail section p,.catalog-detail li{font-size:11px;line-height:1.5;color:var(--color-text-secondary)}.catalog-detail ul{display:flex;flex-direction:column;gap:5px;list-style:none}.catalog-detail li{display:flex;align-items:center;gap:7px}.catalog-detail li :global(svg){flex:none;color:var(--color-success)}.catalog-back{align-self:flex-start;display:flex;align-items:center;gap:5px;color:var(--color-text-muted);font-size:10px}.catalog-back:hover{color:var(--color-text-primary)}.catalog-capabilities{padding:10px;border-radius:var(--radius-md);background:color-mix(in srgb,var(--color-brand) 8%,var(--color-surface))}.catalog-capabilities small,.catalog-context span{font-size:9px;letter-spacing:.08em;color:var(--color-brand)}.catalog-capabilities p{margin-top:3px;font-size:11px}.catalog-context{display:flex;flex-direction:column;gap:2px;padding:9px;border-radius:var(--radius-md);background:color-mix(in srgb,var(--color-brand) 8%,var(--color-surface))}.catalog-context strong{font-size:12px}.catalog-context small{font-size:10px;color:var(--color-text-muted)}
</style>
