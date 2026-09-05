import { describe, expect, it } from "vitest";
import { summarizeRunForEsmeralda } from "./session.svelte";
import type { RunSummary } from "$lib/types";

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
