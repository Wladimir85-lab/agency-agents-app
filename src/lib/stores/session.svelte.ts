/** Esmeralda's conversation store — one persistent `ProjectSession` per
 *  project (see session.rs). Replaces `thread.svelte.ts`, which was only
 *  ever a derived view over `RunSummary` (no persisted messages, no
 *  context carried into the next turn). This store is now the real chat:
 *  messages round-trip through the backend immediately, so they survive an
 *  app restart, and `buildConversationalIntent` is what actually gives the
 *  next turn's prompt real conversational memory instead of an isolated
 *  line of text. */
import { invoke } from "@tauri-apps/api/core";
import type { ConversationMessage, MessageRole, ProjectSession, RunSummary } from "$lib/types";

const MAX_CONTEXT_MESSAGES = 8;
const MAX_MESSAGE_PREVIEW = 800;
const ROLE_LABEL: Record<MessageRole, string> = {
  user: "USUARIO",
  esmeralda: "ESMERALDA",
  system: "SISTEMA",
};

function preview(text: string): string {
  const trimmed = text.trim();
  return trimmed.length > MAX_MESSAGE_PREVIEW ? `${trimmed.slice(0, MAX_MESSAGE_PREVIEW)}…` : trimmed;
}

/** What actually gives a follow-up turn conversational memory: the last few
 *  messages (from either side) get prepended to the raw instruction before
 *  it becomes `RunSummary.intent` — which `stage_prompt` in runtime.rs puts
 *  verbatim under "USER INTENT:" for whichever engine (Qwen locally, or the
 *  external CLI) actually builds the turn. Without this, each turn is an
 *  isolated prompt no matter how evolved the workspace is. */
export function buildConversationalIntent(messages: ConversationMessage[], instruction: string): string {
  const recent = messages.slice(-MAX_CONTEXT_MESSAGES);
  const clean = instruction.trim();
  if (!recent.length) return clean;
  const context = recent.map((m) => `${ROLE_LABEL[m.role]}: ${preview(m.content)}`).join("\n");
  return [
    "CONVERSACIÓN PREVIA CON ESMERALDA SOBRE ESTE PROYECTO",
    "(contexto para entender la continuidad del trabajo; no repitas lo ya hecho salvo que la nueva instrucción lo pida explícitamente)",
    context,
    "",
    "---",
    "NUEVA INSTRUCCIÓN DEL USUARIO:",
    clean,
  ].join("\n");
}

/** Esmeralda's own reply after a turn finishes — persisted back into the
 *  conversation so the next turn's context (and the visible chat) carries
 *  what actually happened, not just what was asked for.
 *
 *  Requisito 6 closure (2026-09-04): `terminalState` is checked *before*
 *  falling back to the plain `status`-based branches below, so a run that
 *  IntentOS classified as `humanDecisionRequired` (e.g. no external
 *  executor configured for a stage that needs one) reads as a real
 *  question waiting on Wladimir, never as "something broke" — the
 *  distinction the backend already computes (`runtime.rs`'s
 *  `classify_app_error`/`JobState`) must not disappear at the one place a
 *  human actually reads it. A run with no `terminalState` at all (legacy,
 *  or not yet reclassified) falls straight through to the original
 *  status-based behavior, unchanged. */
export function summarizeRunForEsmeralda(run: RunSummary): string {
  if (run.terminalState?.state === "humanDecisionRequired") {
    return `Necesito que decidas algo antes de seguir: ${run.terminalState.detail} Esto no es una falla técnica — IntentOS está esperando tu decisión o configuración para continuar.`;
  }
  if (run.status === "succeeded") {
    const passed = run.stages.filter((s) => s.status === "passed").length;
    return `Listo — construí y verifiqué esta instrucción (${passed}/${run.stages.length} etapas superadas). Puedes verlo en el Showroom, pedirme otro ajuste, o aplicar los cambios al proyecto original cuando quieras.`;
  }
  if (run.status === "failed") {
    return `No pude completar esta instrucción: ${run.error?.trim() || "una etapa falló sin motivo registrado"}. Dime cómo corregirlo o reformula lo que necesitas.`;
  }
  if (run.status === "cancelled") {
    return "Cancelé esta instrucción a tu pedido.";
  }
  return `Estado actual: ${run.status}.`;
}

/** Esmeralda's real conversational reply (2026-09-04 — "quiero que
 *  Esmeralda pueda responder"). Until this, `summarizeRunForEsmeralda`'s
 *  fixed templates were the only thing that ever appeared as her message
 *  — no model ever generated anything she "said". This function keeps
 *  that fact-generator as the untouchable ground truth (what actually
 *  happened is never left to a model to decide or invent) and asks the
 *  already-configured completion engine — `local_model_complete`, the
 *  same generic command the rest of the app already uses, on whichever
 *  backend Esmeralda's own build/conversation engine currently points to
 *  (see local_model.rs's `InferenceBackend`) — to phrase those exact
 *  facts naturally, with real conversational context. Reuses the
 *  classifier's own completion command deliberately, not a new one:
 *  "ya todo está inventado" — the primitive already exists.
 *
 *  Falls back to the deterministic fact string verbatim on *any* failure
 *  (Paranoid Mode, no backend configured, network blocked, malformed
 *  reply) — Esmeralda must always answer with something real, never
 *  silence, and never a phrasing that could drift from the actual
 *  outcome. */
export async function narrateRunForEsmeralda(
  run: RunSummary,
  priorMessages: ConversationMessage[],
): Promise<string> {
  const facts = summarizeRunForEsmeralda(run);
  const context = priorMessages
    .slice(-MAX_CONTEXT_MESSAGES)
    .map((m) => `${ROLE_LABEL[m.role]}: ${preview(m.content)}`)
    .join("\n");
  const prompt = [
    "Eres Esmeralda, la inteligencia conversacional de IntentOS. Un usuario te pidió construir algo y esa etapa de trabajo acaba de terminar.",
    "",
    "HECHOS REALES DE ESTE RESULTADO (no inventes nada fuera de esto, no cambies ni suavices el resultado real):",
    facts,
    context ? `\nCONVERSACIÓN PREVIA CON ESTE USUARIO:\n${context}` : "",
    "",
    "Respóndele ahora en español, en primera persona, en 1 a 3 frases, con un tono cercano y directo — como si fueras vos quien construyó esto. Comunica exactamente los hechos reales de arriba, sin inventar ni un resultado distinto ni detalles que no están ahí.",
  ].join("\n");
  try {
    const completion = await invoke<{ content: string }>("local_model_complete", {
      request: { prompt, maxTokens: 220, purpose: "esmeralda_conversational_reply" },
    });
    const reply = completion.content.trim();
    return reply || facts;
  } catch (error) {
    console.warn(
      "[session] Esmeralda's conversational reply is unavailable right now (Paranoid Mode, no backend configured, or a real error) — falling back to the deterministic report:",
      error,
    );
    return facts;
  }
}

/** Esmeralda's reply to a plain conversational message — no run, no
 *  Mission, no build involved (2026-09-04 — "quiero que sea un chat junto
 *  a la entrada... digo hola esmeralda, ella responde, y de ahí le digo
 *  quiero construir x cosa"). Its one call site (Runbooks.svelte's
 *  `sendTurn`) only reaches this for messages `isConversationalMessage`
 *  (intentosCapabilities.ts) classified as chat, not a build instruction
 *  — this function never decides that itself, and never starts anything.
 *  Same completion primitive and fallback discipline as
 *  `narrateRunForEsmeralda`: if the model is unavailable for any reason,
 *  a real (if generic) reply is still returned — never silence. */
export async function replyToEsmeraldaChat(
  text: string,
  priorMessages: ConversationMessage[],
): Promise<string> {
  const context = priorMessages
    .slice(-MAX_CONTEXT_MESSAGES)
    .map((m) => `${ROLE_LABEL[m.role]}: ${preview(m.content)}`)
    .join("\n");
  const prompt = [
    "Eres Esmeralda, la inteligencia conversacional y soberana de IntentOS. Un usuario te está hablando — todavía no te pidió construir nada en este mensaje.",
    context ? `\nCONVERSACIÓN PREVIA CON ESTE USUARIO:\n${context}` : "",
    `\nMENSAJE NUEVO DEL USUARIO:\n${text.trim()}`,
    "\nRespóndele en español, en primera persona, en 1 a 3 frases, con un tono cercano y natural — como una conversación real, no un formulario. Si simplemente te está saludando o charlando, respóndele igual de natural; no le exijas que 'construya algo'. Si en este mensaje sí te pide construir, crear o cambiar algo concreto, dile que puede describirlo con más detalle y vas a preparar la propuesta.",
  ].join("\n");
  try {
    const completion = await invoke<{ content: string }>("local_model_complete", {
      request: { prompt, maxTokens: 220, purpose: "esmeralda_conversational_reply" },
    });
    const reply = completion.content.trim();
    return reply || "Hola — dime en qué te ayudo.";
  } catch (error) {
    console.warn(
      "[session] Esmeralda's chat reply is unavailable right now (Paranoid Mode, no backend configured, or a real error):",
      error,
    );
    return "Hola — puedo conversar contigo o construir lo que necesites; contame qué tenés en mente.";
  }
}

/** Esmeralda's reply when a build instruction arrives but no project
 *  exists yet — 2026-09-05: "que las carpetas se creen cuando el mandato
 *  sea explícito, ese chat se puede ver como una reunión de acuerdo".
 *  Creating a project is a real filesystem commitment (a new folder on
 *  disk), so the first mention of wanting something built opens a
 *  discussion, not an execution — this drafts Esmeralda's side of that
 *  discussion (summarize + ask for an explicit go-ahead) without ever
 *  claiming anything was built or started, since at this point nothing
 *  has been. Its one call site (Runbooks.svelte's `sendTurn`) only
 *  reaches this when there is no `projectPath` yet; a genuinely explicit
 *  confirmation (`isExplicitConfirmation`, intentosCapabilities.ts) is
 *  what actually triggers `approveAndStart` afterward — this function
 *  itself never creates anything. */
export async function draftBuildConfirmation(
  text: string,
  priorMessages: ConversationMessage[],
): Promise<string> {
  const context = priorMessages
    .slice(-MAX_CONTEXT_MESSAGES)
    .map((m) => `${ROLE_LABEL[m.role]}: ${preview(m.content)}`)
    .join("\n");
  const prompt = [
    "Eres Esmeralda, la inteligencia conversacional de IntentOS. El usuario acaba de describir algo que quiere que construyas, pero todavía NO se creó ningún proyecto ni se inició ninguna construcción real — este es un momento de acuerdo entre ambos, no de ejecución.",
    context ? `\nCONVERSACIÓN PREVIA CON ESTE USUARIO:\n${context}` : "",
    `\nLO QUE EL USUARIO DESCRIBIÓ QUERER CONSTRUIR:\n${text.trim()}`,
    "\nRespóndele en español, en primera persona, en 2 a 4 frases: resume brevemente lo que entendiste que quiere, y preguntale explícitamente si quiere que lo construyas ahora tal cual, o si prefiere ajustar algo antes. No digas en ningún momento que ya lo construiste, que ya lo creaste o que ya empezaste a trabajar en eso — nada de eso ocurrió todavía.",
  ].join("\n");
  try {
    const completion = await invoke<{ content: string }>("local_model_complete", {
      request: { prompt, maxTokens: 220, purpose: "esmeralda_build_confirmation" },
    });
    const reply = completion.content.trim();
    return reply || `Entendí que querés esto: "${text.trim()}". ¿Querés que lo construya ahora, o preferís ajustar algo antes?`;
  } catch (error) {
    console.warn(
      "[session] Esmeralda's build-confirmation draft is unavailable right now (Paranoid Mode, no backend configured, or a real error):",
      error,
    );
    return `Entendí que querés esto: "${text.trim()}". ¿Querés que lo construya ahora, o preferís ajustar algo antes?`;
  }
}

/** Esmeralda's answer to a hypothetical "qué equipo convocarías" question —
 *  2026-09-05, real bug/requirement: describing the team must use
 *  IntentOS's real agent catalog, never invented generic roles ("un
 *  diseñador, un desarrollador..."), when a canonical source exists. The
 *  caller (Runbooks.svelte) computes the real roster first — via the same
 *  `planSolutionAsync` routing a real build would use, applied here purely
 *  informationally, no project/run/side-effect involved — and passes it
 *  in as `roster`; this function never invents the team itself, only asks
 *  the model to phrase the real one naturally. Same fallback discipline as
 *  every other Esmeralda-reply function here: a real, useful answer even
 *  if the model call fails, never silence. */
export async function draftTeamExplanation(
  text: string,
  roster: { label: string; agentName: string }[],
  priorMessages: ConversationMessage[],
): Promise<string> {
  const rosterText = roster.map((s, i) => `${i + 1}. ${s.label} — ${s.agentName}`).join("\n");
  const fallback = `El equipo real que IntentOS convocaría para esto:\n${rosterText}\n\nEsto es solo la explicación que pediste — no construí ni creé nada.`;
  const context = priorMessages
    .slice(-MAX_CONTEXT_MESSAGES)
    .map((m) => `${ROLE_LABEL[m.role]}: ${preview(m.content)}`)
    .join("\n");
  const prompt = [
    "Eres Esmeralda, la inteligencia conversacional de IntentOS. El usuario te pidió, de forma hipotética o explicativa, describir qué equipo convocarías — NO te pidió construir nada, y no se creó ni se va a crear ningún proyecto a partir de este mensaje.",
    context ? `\nCONVERSACIÓN PREVIA CON ESTE USUARIO:\n${context}` : "",
    `\nPREGUNTA DEL USUARIO:\n${text.trim()}`,
    `\nEQUIPO REAL QUE INTENTOS COMPONDRÍA PARA ESTO (datos reales del catálogo de agentes — usa exactamente estos roles y nombres, no inventes otros ni cambies estos):\n${rosterText}`,
    "\nExplícale el equipo al usuario en español, en primera persona, en 3 a 6 frases, con un tono conversacional — usando exactamente los roles y nombres reales de arriba. No digas en ningún momento que ya construiste algo, que ya creaste un proyecto o que ya empezaste a trabajar — esto es solo una explicación hipotética que pidió.",
  ].join("\n");
  try {
    const completion = await invoke<{ content: string }>("local_model_complete", {
      request: { prompt, maxTokens: 320, purpose: "esmeralda_team_explanation" },
    });
    const reply = completion.content.trim();
    return reply || fallback;
  } catch (error) {
    console.warn(
      "[session] Esmeralda's team-explanation draft is unavailable right now (Paranoid Mode, no backend configured, or a real error):",
      error,
    );
    return fallback;
  }
}

class SessionStore {
  current: ProjectSession | null = $state(null);
  loading = $state(false);
  error: string | null = $state(null);
  /** Instructions typed while a turn is already building. Processed FIFO
   *  once the active run reaches a terminal state — see the queue effect
   *  in Runbooks.svelte. In-memory only: the message itself is never lost
   *  (appendMessage persists it the moment it's sent), only the
   *  "auto-continue" ordering is best-effort and resets on app restart —
   *  the deliberate stability trade-off from the authoritative brief. */
  queue: string[] = $state([]);

  get messages(): ConversationMessage[] {
    return this.current?.messages ?? [];
  }

  async loadOrCreate(projectPath: string): Promise<ProjectSession | null> {
    if (!projectPath.trim()) {
      this.current = null;
      this.queue = [];
      return null;
    }
    this.loading = true;
    this.error = null;
    try {
      this.current = await invoke<ProjectSession>("session_get_or_create", { projectPath });
      this.queue = [];
      return this.current;
    } catch (e) {
      this.error = String(e);
      return null;
    } finally {
      this.loading = false;
    }
  }

  async appendMessage(
    projectPath: string,
    role: MessageRole,
    content: string,
    runId?: string | null,
  ): Promise<void> {
    if (!content.trim()) return;
    try {
      this.current = await invoke<ProjectSession>("session_append_message", {
        projectPath,
        role,
        content,
        runId: runId ?? null,
      });
    } catch (e) {
      this.error = String(e);
    }
  }

  enqueue(text: string): void {
    const clean = text.trim();
    if (clean) this.queue = [...this.queue, clean];
  }

  dequeue(): string | null {
    if (!this.queue.length) return null;
    const [next, ...rest] = this.queue;
    this.queue = rest;
    return next;
  }

  clear(): void {
    this.current = null;
    this.queue = [];
  }
}

export const session = new SessionStore();
