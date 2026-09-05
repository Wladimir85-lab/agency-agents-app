import { beforeEach, describe, expect, it, vi } from "vitest";

// Must be mocked before session.svelte is imported — it imports `invoke`
// at module scope. Same vi.hoisted pattern as intentosCapabilities.test.ts.
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { narrateRunForEsmeralda, replyToEsmeraldaChat, summarizeRunForEsmeralda } from "./session.svelte";
import type { RunSummary } from "$lib/types";

beforeEach(() => {
  invokeMock.mockReset();
});

// Minimal fixture — only the fields summarizeRunForEsmeralda actually
// reads are given real values; the rest are structurally required but
// irrelevant to this function's behavior.
function baseRun(overrides: Partial<RunSummary>): RunSummary {
  return {
    id: "run-1",
    intent: "intent",
    projectPath: "/tmp/project",
    workspacePath: null,
    runbookId: "startup-mvp",
    capabilityId: "digital-experience",
    capabilityIds: ["digital-experience"],
    providerId: null,
    status: "running",
    currentStage: null,
    stages: [],
    createdAt: "2026-09-04T00:00:00Z",
    updatedAt: "2026-09-04T00:00:00Z",
    completedAt: null,
    error: null,
    ...overrides,
  };
}

describe("summarizeRunForEsmeralda — Requisito 6 closure: terminalState must reach the human distinctly", () => {
  it("a humanDecisionRequired run reads as a real question, never as a generic failure", () => {
    const run = baseRun({
      status: "failed",
      error: "la etapa de capability 'stage.development' requiere una decisión humana: No hay proveedor externo configurado.",
      terminalState: {
        state: "humanDecisionRequired",
        detail: "la etapa de capability 'stage.development' requiere una decisión humana: No hay proveedor externo configurado.",
      },
    });
    const message = summarizeRunForEsmeralda(run);
    expect(message).toContain("Necesito que decidas algo");
    expect(message).not.toMatch(/no pude completar/i);
  });

  it("a blockedWithEvidence run still reads as a technical block, unchanged from today", () => {
    const run = baseRun({
      status: "failed",
      error: "stage did not provide INTENTOS_GATE:PASS",
      terminalState: {
        state: "blockedWithEvidence",
        detail: "stage did not provide INTENTOS_GATE:PASS",
      },
    });
    const message = summarizeRunForEsmeralda(run);
    expect(message).toMatch(/no pude completar/i);
    expect(message).not.toContain("Necesito que decidas algo");
  });

  it("a resultVerified run is still reported as success", () => {
    const run = baseRun({
      status: "succeeded",
      stages: [
        { id: "direction", label: "Dirección", agentSlug: "pm", kind: "direction", status: "passed", attempt: 1 },
      ],
      terminalState: { state: "resultVerified" },
    });
    const message = summarizeRunForEsmeralda(run);
    expect(message).toMatch(/listo/i);
  });

  it("E2E wire fixture: parses the exact JSON runtime.rs's real 'no provider configured' path persisted to disk", () => {
    // Captured verbatim from
    // e2e_real_disk_a_missing_provider_job_persists_the_exact_wire_shape_the_frontend_parses
    // (src-tauri/src/runtime.rs) — not hand-written, so this test fails if
    // the real backend's serialization shape ever drifts from what this
    // frontend code expects.
    const terminalStateJson =
      '{"detail":"la etapa de capability \'stage.development\' requiere una decisión humana: La etapa \'Development\' requiere un ejecutor externo (Codex o Claude) que no fue configurado para esta corrida; deteniendo de forma controlada.","state":"humanDecisionRequired"}';
    const run = baseRun({
      status: "failed",
      error: "human decision required",
      terminalState: JSON.parse(terminalStateJson),
    });
    const message = summarizeRunForEsmeralda(run);
    expect(message).toContain("Necesito que decidas algo");
    expect(message).toContain("requiere un ejecutor externo");
  });

  it("a run with no terminalState at all keeps the original legacy behavior and never crashes", () => {
    const succeeded = baseRun({ status: "succeeded", stages: [] });
    const failed = baseRun({ status: "failed", error: "algo falló" });
    const cancelled = baseRun({ status: "cancelled" });
    expect(() => summarizeRunForEsmeralda(succeeded)).not.toThrow();
    expect(() => summarizeRunForEsmeralda(failed)).not.toThrow();
    expect(() => summarizeRunForEsmeralda(cancelled)).not.toThrow();
    expect(summarizeRunForEsmeralda(failed)).toMatch(/no pude completar/i);
    expect(summarizeRunForEsmeralda(cancelled)).toMatch(/cancelé/i);
  });
});

describe("narrateRunForEsmeralda — Esmeralda's real conversational reply (2026-09-04)", () => {
  it("uses the model's phrased reply when the completion call succeeds", async () => {
    invokeMock.mockResolvedValueOnce({ content: "¡Listo! Ya quedó construido y verificado." });
    const run = baseRun({
      status: "succeeded",
      stages: [{ id: "direction", label: "Dirección", agentSlug: "pm", kind: "direction", status: "passed", attempt: 1 }],
    });
    const message = await narrateRunForEsmeralda(run, []);
    expect(message).toBe("¡Listo! Ya quedó construido y verificado.");
    expect(invokeMock).toHaveBeenCalledWith(
      "local_model_complete",
      expect.objectContaining({
        request: expect.objectContaining({ purpose: "esmeralda_conversational_reply" }),
      }),
    );
  });

  it("the prompt sent to the model carries the real facts, never a different outcome", async () => {
    invokeMock.mockResolvedValueOnce({ content: "ok" });
    const run = baseRun({ status: "failed", error: "la base de datos no respondió" });
    await narrateRunForEsmeralda(run, []);
    const sentPrompt = invokeMock.mock.calls[0][1].request.prompt as string;
    expect(sentPrompt).toContain("la base de datos no respondió");
    expect(sentPrompt).toContain("no inventes");
  });

  it("falls back to the deterministic report when the model call fails — never silent", async () => {
    invokeMock.mockRejectedValueOnce(new Error("Paranoid Mode is on"));
    const run = baseRun({ status: "failed", error: "una etapa falló" });
    const message = await narrateRunForEsmeralda(run, []);
    expect(message).toBe(summarizeRunForEsmeralda(run));
  });

  it("falls back to the deterministic report when the model replies with empty content", async () => {
    invokeMock.mockResolvedValueOnce({ content: "   " });
    const run = baseRun({ status: "cancelled" });
    const message = await narrateRunForEsmeralda(run, []);
    expect(message).toBe(summarizeRunForEsmeralda(run));
  });
});

describe("replyToEsmeraldaChat — real chat with Esmeralda, no run involved (2026-09-04)", () => {
  it("returns the model's real reply for a plain conversational message", async () => {
    invokeMock.mockResolvedValueOnce({ content: "¡Hola! Todo bien por acá, ¿en qué te ayudo hoy?" });
    const message = await replyToEsmeraldaChat("hola esmeralda", []);
    expect(message).toBe("¡Hola! Todo bien por acá, ¿en qué te ayudo hoy?");
    expect(invokeMock).toHaveBeenCalledWith(
      "local_model_complete",
      expect.objectContaining({
        request: expect.objectContaining({ purpose: "esmeralda_conversational_reply" }),
      }),
    );
  });

  it("carries prior conversation into the prompt for real continuity", async () => {
    invokeMock.mockResolvedValueOnce({ content: "ok" });
    await replyToEsmeraldaChat("¿te acuerdas de lo que hablamos?", [
      { id: "m1", role: "user", content: "me llamo Wladimir", at: "2026-09-04T00:00:00Z" },
    ]);
    const sentPrompt = invokeMock.mock.calls[0][1].request.prompt as string;
    expect(sentPrompt).toContain("me llamo Wladimir");
  });

  it("never crashes and always returns a real reply when the model is unavailable", async () => {
    invokeMock.mockRejectedValueOnce(new Error("Paranoid Mode is on"));
    const message = await replyToEsmeraldaChat("hola", []);
    expect(typeof message).toBe("string");
    expect(message.length).toBeGreaterThan(0);
  });
});
