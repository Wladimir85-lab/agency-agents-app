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
