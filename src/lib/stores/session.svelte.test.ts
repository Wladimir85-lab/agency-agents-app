import { beforeEach, describe, expect, it, vi } from "vitest";

// Must be mocked before session.svelte is imported — it imports `invoke`
// at module scope. Same vi.hoisted pattern as intentosCapabilities.test.ts.
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { draftBuildConfirmation, draftTeamExplanation, narrateRunForEsmeralda, replyToEsmeraldaChat, summarizeRunForEsmeralda } from "./session.svelte";
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

describe("draftBuildConfirmation — the agreement step before a project is created (2026-09-05)", () => {
  it("asks for confirmation and never claims anything was already built", async () => {
    invokeMock.mockResolvedValueOnce({
      content: "Entendí que querés una landing page para tu estudio de fotografía. ¿Te la construyo ya así, o querés ajustar algo antes?",
    });
    const message = await draftBuildConfirmation("quiero una landing page para mi estudio de fotografía", []);
    expect(message).toContain("¿");
    expect(message).not.toMatch(/ya (lo )?constru|ya (lo )?cre[ée]|ya empec[ée]/i);
  });

  it("the prompt sent to the model is explicit that nothing has been created yet", async () => {
    invokeMock.mockResolvedValueOnce({ content: "ok" });
    await draftBuildConfirmation("necesito un dashboard", []);
    const sentPrompt = invokeMock.mock.calls[0][1].request.prompt as string;
    expect(sentPrompt).toContain("NO se creó ningún proyecto");
    expect(sentPrompt).toContain("necesito un dashboard");
  });

  it("falls back to a real confirmation question, never silence, when the model is unavailable", async () => {
    invokeMock.mockRejectedValueOnce(new Error("Paranoid Mode is on"));
    const message = await draftBuildConfirmation("quiero un blog", []);
    expect(message).toContain("¿");
    expect(message.length).toBeGreaterThan(0);
  });
});

describe("draftTeamExplanation — real catalog roster, never invented roles (2026-09-05)", () => {
  const realRoster = [
    { label: "Dirección de proyecto", agentName: "Senior Project Manager" },
    { label: "UX y dirección creativa", agentName: "UX Architect" },
    { label: "Desarrollo de experiencia", agentName: "Frontend Developer" },
  ];

  it("the prompt sent to the model carries the real roster verbatim, not a generic placeholder", async () => {
    invokeMock.mockResolvedValueOnce({ content: "ok" });
    await draftTeamExplanation("solo dime a quiénes convocas", realRoster, []);
    const sentPrompt = invokeMock.mock.calls[0][1].request.prompt as string;
    expect(sentPrompt).toContain("Senior Project Manager");
    expect(sentPrompt).toContain("UX Architect");
    expect(sentPrompt).toContain("Frontend Developer");
    expect(sentPrompt).toContain("no inventes otros");
  });

  it("the fallback (model unavailable) still uses the real roster, never a generic invented team", async () => {
    invokeMock.mockRejectedValueOnce(new Error("Paranoid Mode is on"));
    const message = await draftTeamExplanation("qué equipo convocarías", realRoster, []);
    expect(message).toContain("Senior Project Manager");
    expect(message).toContain("UX Architect");
    expect(message).not.toMatch(/dise[ñn]ador gen[ée]rico|desarrollador gen[ée]rico/i);
  });

  it("never claims anything was built or created — this is a hypothetical explanation only", async () => {
    invokeMock.mockResolvedValueOnce({ content: "El equipo sería: Senior Project Manager, UX Architect y Frontend Developer." });
    const message = await draftTeamExplanation("cómo organizarías el equipo", realRoster, []);
    expect(message).not.toMatch(/ya (lo )?constru|ya (lo )?cre[ée]|ya empec[ée]|proyecto creado/i);
  });
});
