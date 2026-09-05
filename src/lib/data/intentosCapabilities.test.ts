import { beforeEach, describe, expect, it, vi } from "vitest";

// `invoke` must be mocked before the module under test is imported, since
// intentosCapabilities.ts imports it at module scope (same pattern the rest
// of the app uses to reach `local_model_complete` — see local_model.rs).
// vi.hoisted is required here (not a plain outer const) because vi.mock's
// factory is itself hoisted above regular imports by vitest.
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  composePipeline,
  CREATIVE_TECH_SPECIALIZATIONS,
  INTENTOS_CAPABILITIES,
  isConversationalMessage,
  isExplicitConfirmation,
  planSolution,
  planSolutionAsync,
  routeIntent,
} from "./intentosCapabilities";

function semanticReply(body: Record<string, unknown>) {
  return { content: JSON.stringify(body) };
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("fast path — unambiguous intents never call the model", () => {
  it("routes a plain landing-page intent to digital-experience without invoking anything", async () => {
    const proposal = await planSolutionAsync({ intent: "Necesito una landing page para mi estudio de fotografía." });
    expect(invokeMock).not.toHaveBeenCalled();
    expect(proposal.capabilities.map((c) => c.id)).toContain("digital-experience");
    expect(proposal.routing?.source).toBe("deterministic");
  });

  it("planSolution (sync) and planSolutionAsync agree, byte for byte, on an unambiguous intent", async () => {
    const input = { intent: "Necesito un dashboard administrativo para gestionar inventario y CRM." };
    const sync = planSolution(input);
    const async_ = await planSolutionAsync(input);
    expect(invokeMock).not.toHaveBeenCalled();
    expect(async_.capabilities.map((c) => c.id)).toEqual(sync.capabilities.map((c) => c.id));
    expect(async_.pipeline.length).toBe(sync.pipeline.length);
  });
});

describe("requirement #7 — the four mandated cases", () => {
  it("A: rotating/exploring the ship by mouse (no 3D/WebGL/inmersivo keywords) reaches creative-technology via the semantic fallback", async () => {
    invokeMock.mockResolvedValueOnce(
      semanticReply({
        capabilityId: "creative-technology",
        confidence: 0.88,
        reason: "El usuario pide navegación libre alrededor de un objeto 3D; eso es una experiencia inmersiva por su propia naturaleza, aunque no use esas palabras.",
        creativeTechnologyJustified: true,
        creativeTechnologyReason: "Rotar, acercarse y recorrer el barco desde cualquier ángulo requiere geometría real navegable, no una imagen estática.",
      }),
    );
    const text = "Quiero que el visitante pueda girar el barco con el mouse, acercarse y recorrerlo desde cualquier ángulo.";
    expect(text.toLocaleLowerCase("es")).not.toMatch(/3d|webgl|inmersiv/);
    const proposal = await planSolutionAsync({ intent: text });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    // Deliberate: this classification forces DeepSeek (open-weight, cheap
    // API — chosen after Groq was rejected for not being open/sovereign,
    // and local Qwen2.5-Coder-3B proved unreliable on
    // creativeTechnologyJustified — see classifyIntentSemantic's doc
    // comment), independent of whatever INTENTOS_INFERENCE_BACKEND
    // Esmeralda's own build engine uses.
    expect(invokeMock).toHaveBeenCalledWith("local_model_complete", expect.objectContaining({ request: expect.objectContaining({ backend: "deepseek" }) }));
    expect(proposal.routing?.source).toBe("semantic");
    expect(proposal.capabilities.map((c) => c.id)).toContain("creative-technology");
    expect(proposal.creativeTechnology?.justified).toBe(true);
    const developmentStage = proposal.pipeline.find((s) => s.id === "creative-technology:development");
    expect(developmentStage?.agent).toBe("xr-immersive-developer");
  });

  it("B: a conventional client/invoice/payments admin panel is routed to systems-data and explicitly rejects creative-technology", async () => {
    invokeMock.mockResolvedValueOnce(
      semanticReply({
        capabilityId: "systems-data",
        confidence: 0.82,
        reason: "Panel administrativo CRUD sobre clientes, facturas y pagos; sin componente visual/experiencial avanzado.",
        creativeTechnologyJustified: false,
        creativeTechnologyReason: "Un panel administrativo convencional no mejora de forma material con 3D/WebGL/shaders.",
      }),
    );
    const proposal = await planSolutionAsync({ intent: "Necesito un panel para administrar clientes, facturas y pagos." });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(proposal.capabilities.map((c) => c.id)).not.toContain("creative-technology");
    expect(proposal.creativeTechnology?.justified).toBe(false);
    expect(proposal.risks.some((r) => r.includes("NO incluida"))).toBe(true);
  });

  it("C: an incidental '3D' mention on a product that doesn't need it still defers to (and can be rejected by) the semantic check, not a bare substring", async () => {
    const text = "Necesito un catálogo de productos con fotos en alta resolución, algunas en formato 3D de referencia, para mi tienda online.";
    // The deterministic fast path alone would weakly match creative-technology
    // on the single word "3d" — prove that alone is NOT trusted (requirement
    // #3 / the audit's case C), i.e. the semantic fallback is actually invoked.
    const routed = routeIntent(text);
    const ctEntry = routed.ranked.find((r) => r.capability.id === "creative-technology");
    expect(ctEntry?.matched).toEqual(["3d"]);

    invokeMock.mockResolvedValueOnce(
      semanticReply({
        capabilityId: "digital-experience",
        confidence: 0.8,
        reason: "Catálogo de productos ecommerce; la mención a 3D es incidental (fotos de referencia), no una experiencia 3D real.",
        creativeTechnologyJustified: false,
        creativeTechnologyReason: "La mención de '3D' es incidental; el catálogo no requiere una escena 3D real.",
      }),
    );
    const proposal = await planSolutionAsync({ intent: text });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(proposal.capabilities.map((c) => c.id)).not.toContain("creative-technology");
    expect(proposal.creativeTechnology?.justified).toBe(false);
  });

  it("D: the real, previously-executed Naval Studio intent still resolves creative-technology — and still does it on the fast path", async () => {
    // A faithful excerpt of the actual intent recorded for the real,
    // already-executed run (state/runs/a50e4ac6-....json, capability
    // creative-technology:development -> xr-immersive-developer, status
    // "passed") — not a rewritten paraphrase.
    const navalStudioIntent = `
# NICOLÁS NAVAL STUDIO
## Design · Measure · Model
Crear un estudio naval digital independiente que permita a Nicolás presentar su trabajo,
documentar embarcaciones, capturar mediciones preliminares mediante iPhone y continuar el
desarrollo técnico de cada proyecto en Rhino.

# EXPERIENCIA PÚBLICA: EL PORTAFOLIO
## Inicio
Una apertura cinematográfica a pantalla completa:
NICOLÁS NAVAL STUDIO — Design · Measure · Model
"From physical vessels to precise digital models."

## Proyectos
- Modelo 3D interactivo cuando exista.
- Galería cinematográfica.

## Servicios
- Visualización 3D.

# DIRECCIÓN VISUAL
## Recursos visuales
- Visor tridimensional interactivo.
`.trim();
    const proposal = await planSolutionAsync({ intent: navalStudioIntent });
    expect(invokeMock).not.toHaveBeenCalled(); // strong multi-keyword match stays fast-path
    expect(proposal.routing?.source).toBe("deterministic");
    expect(proposal.capabilities.map((c) => c.id)).toContain("creative-technology");
    expect(proposal.creativeTechnology?.justified).toBe(true);
    const developmentStage = proposal.pipeline.find((s) => s.id === "creative-technology:development");
    expect(developmentStage?.agent).toBe("xr-immersive-developer");
  });
});

describe("requirement #2 — the semantic router can never invent a capability", () => {
  it("ignores a capabilityId outside INTENTOS_CAPABILITIES and falls back to the deterministic guess", async () => {
    invokeMock.mockResolvedValueOnce(
      semanticReply({ capabilityId: "quantum-computing-experience", confidence: 0.9, reason: "hallucinated" }),
    );
    const proposal = await planSolutionAsync({ intent: "Quiero que el visitante pueda girar el barco con el mouse y verlo desde cualquier ángulo." });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(INTENTOS_CAPABILITIES.some((c) => c.id === "quantum-computing-experience")).toBe(false);
    expect(proposal.capabilities.every((c) => INTENTOS_CAPABILITIES.some((known) => known.id === c.id))).toBe(true);
    expect(proposal.routing?.source).not.toBe("semantic");
  });

  it("keeps a valid creativeTechnologyJustified verdict even when capabilityId is hallucinated (real Qwen2.5-Coder-3B behavior, probed live this session)", async () => {
    // Reproduces, verbatim, what the actual local model returned for case A
    // when probed for real over Ollama during this session: a made-up
    // capabilityId ("i3d") inside an otherwise well-reasoned, correct reply.
    invokeMock.mockResolvedValueOnce(
      semanticReply({
        capabilityId: "i3d",
        confidence: 0.85,
        reason: "El producto necesita la capacidad de 3D para permitir al usuario girar y explorar el barco desde cualquier ángulo.",
        creativeTechnologyJustified: true,
        creativeTechnologyReason: "El uso de 3D ofrece una experiencia visual más inmersiva, aumentando la interacción y la comprensión del usuario.",
      }),
    );
    const proposal = await planSolutionAsync({ intent: "Quiero que el visitante pueda girar el barco con el mouse, acercarse y recorrerlo desde cualquier ángulo." });
    expect(INTENTOS_CAPABILITIES.some((c) => c.id === "i3d")).toBe(false);
    expect(proposal.capabilities.map((c) => c.id)).toContain("creative-technology");
    expect(proposal.creativeTechnology?.justified).toBe(true);
    expect(proposal.routing?.source).toBe("semantic");
  });

  it("degrades gracefully (never throws) when the local model is unreachable", async () => {
    invokeMock.mockRejectedValueOnce(new Error("CapabilityProviderUnavailable: inference.local"));
    const proposal = await planSolutionAsync({ intent: "Quiero que el visitante pueda girar el barco con el mouse y verlo desde cualquier ángulo." });
    expect(proposal.routing?.source).toBe("semantic-unavailable");
    expect(proposal.capabilities.length).toBeGreaterThan(0);
  });

  it("ignores a non-JSON reply instead of crashing", async () => {
    invokeMock.mockResolvedValueOnce({ content: "no puedo ayudarte con eso, disculpa" });
    const proposal = await planSolutionAsync({ intent: "Quiero que el visitante pueda girar el barco con el mouse y verlo desde cualquier ángulo." });
    expect(proposal.routing?.source).toBe("semantic-unavailable");
  });
});

describe("requirement #4 — specializations never grow the pipeline", () => {
  it("composePipeline without intentText (every pre-existing caller) still produces the original stage shape", () => {
    const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "creative-technology")!];
    const stages = composePipeline(capabilities);
    expect(stages).toHaveLength(5); // direction, architecture, development, qa, reality — unchanged
    expect(stages.find((s) => s.kind === "development")?.agent).toBe("xr-immersive-developer");
  });

  it("known-future specializations still resolve to the generalist agent, never a persona IntentOS can't actually run", () => {
    for (const spec of CREATIVE_TECH_SPECIALIZATIONS) {
      if (spec.status === "known-future") {
        // known-future specializations must not silently claim a different,
        // unverified agent — composePipeline only trusts supported-now.
        const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "creative-technology")!];
        const stages = composePipeline(capabilities, spec.keywords[0]);
        expect(stages.find((s) => s.kind === "development")?.agent).toBe("xr-immersive-developer");
      }
    }
  });

  it("multi-capability pipelines don't grow just because the router became hybrid", async () => {
    const input = { intent: "Necesito una landing page para mi estudio de fotografía." };
    const sync = planSolution(input);
    const async_ = await planSolutionAsync(input);
    expect(async_.pipeline.length).toBe(sync.pipeline.length);
  });
});

describe("routeIntent().ranked stays additive", () => {
  it("still exposes the original capability/capabilities/confidence/matched fields unchanged", () => {
    const routed = routeIntent("Necesito una landing page para mi estudio de fotografía.");
    expect(routed.capability.id).toBe("digital-experience");
    expect(routed.capabilities.map((c) => c.id)).toContain("digital-experience");
    expect(routed.confidence).toBeGreaterThan(0);
    expect(Array.isArray(routed.matched)).toBe(true);
    expect(Array.isArray(routed.ranked)).toBe(true);
    expect(routed.ranked).toHaveLength(INTENTOS_CAPABILITIES.length);
  });
});

describe("architecture audit fixes — dormant corpus personas connected, broken refs removed", () => {
  it("no agent slug referenced by any capability roster or override is missing from INTENTOS_CAPABILITIES' own agent lists (regression guard for the two dangling refs found in the audit)", () => {
    // These two slugs never existed in the real corpus and silently
    // degraded their stage to "Catalog persona unavailable" (runtime.rs's
    // stage_prompt). The fix replaced them with real personas; this test
    // guards against either name ever reappearing.
    const serialized = JSON.stringify(INTENTOS_CAPABILITIES);
    expect(serialized).not.toContain("engineering-data-visualization-engineer");
    expect(serialized).not.toContain("testing-test-automation-engineer");
  });

  it("iot: a firmware-first intent (ESP32/RTOS, no MQTT/backend/dashboard signal) gets the real embedded-firmware persona", () => {
    const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "iot")!];
    const stages = composePipeline(capabilities, "Necesito firmware para un ESP32 con FreeRTOS que lea un sensor y controle un relé.");
    expect(stages.find((s) => s.kind === "development")?.agent).toBe("engineering-embedded-firmware-engineer");
  });

  it("iot: a full-vertical intent (ESP32 + MQTT + dashboard) keeps the breadth generalist, not the firmware-only specialist", () => {
    const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "iot")!];
    const stages = composePipeline(capabilities, "Necesito un ESP32 que publique por MQTT a un backend con dashboard en tiempo real.");
    expect(stages.find((s) => s.kind === "development")?.agent).toBe("engineering-rapid-prototyper");
  });

  it("digital-experience: an explicit native-mobile intent (React Native) gets the real mobile-app-builder persona instead of the web frontend developer", () => {
    const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "digital-experience")!];
    const stages = composePipeline(capabilities, "Quiero una app multiplataforma con React Native para iOS y Android.");
    expect(stages.find((s) => s.kind === "development")?.agent).toBe("engineering-mobile-app-builder");
  });

  it("digital-experience: a plain landing-page intent still uses the web frontend developer, not the mobile builder", () => {
    const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "digital-experience")!];
    const stages = composePipeline(capabilities, "Necesito una landing page para mi estudio de fotografía.");
    expect(stages.find((s) => s.kind === "development")?.agent).toBe("engineering-frontend-developer");
  });

  it("ai-agents: an explicit MCP/tooling intent gets the real mcp-builder persona instead of the generalist AI engineer", () => {
    const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "ai-agents")!];
    const stages = composePipeline(capabilities, "Quiero construir un servidor MCP que le dé herramientas a mi agente.");
    expect(stages.find((s) => s.kind === "development")?.agent).toBe("specialized-mcp-builder");
  });

  it("ai-agents: a plain RAG/chatbot intent still uses the generalist AI engineer, not the tooling specialist", () => {
    const capabilities = [INTENTOS_CAPABILITIES.find((c) => c.id === "ai-agents")!];
    const stages = composePipeline(capabilities, "Necesito un chatbot con RAG sobre mis documentos internos.");
    expect(stages.find((s) => s.kind === "development")?.agent).toBe("engineering-ai-engineer");
  });
});

describe("isConversationalMessage — real chat with Esmeralda vs. a build instruction (2026-09-04)", () => {
  it("a plain greeting is chat, not a build instruction", () => {
    expect(isConversationalMessage("hola esmeralda")).toBe(true);
    expect(isConversationalMessage("¿cómo estás?")).toBe(true);
    expect(isConversationalMessage("gracias!")).toBe(true);
  });

  it("an explicit build/change instruction is never treated as chat", () => {
    expect(isConversationalMessage("quiero construir una landing page para mi estudio")).toBe(false);
    expect(isConversationalMessage("necesito un dashboard administrativo")).toBe(false);
    expect(isConversationalMessage("arregla el error del formulario de contacto")).toBe(false);
    expect(isConversationalMessage("agrega un botón para exportar a PDF")).toBe(false);
  });

  it("an empty or whitespace-only message is chat (nothing to build)", () => {
    expect(isConversationalMessage("")).toBe(true);
    expect(isConversationalMessage("   ")).toBe(true);
  });

  it("a generic capability question is chat even when it contains a build verb — real bug found live 2026-09-05", () => {
    // The exact phrase that silently started a full production run instead
    // of just answering: it contains "crear", but it's asking what's
    // possible in general, not asking IntentOS to build something.
    expect(isConversationalMessage("¿Qué se puede crear en frontend?")).toBe(true);
    expect(isConversationalMessage("que se puede hacer en frontend")).toBe(true);
    expect(isConversationalMessage("¿Cómo funciona el sistema de pagos?")).toBe(true);
    expect(isConversationalMessage("¿Para qué sirve un dashboard?")).toBe(true);
    // A real request phrased directly must still build, even though it
    // also reads as a question grammatically.
    expect(isConversationalMessage("¿Puedes construirme una landing page para mi estudio?")).toBe(false);
  });
});

describe("isExplicitConfirmation — real mandate gate before a project is created (2026-09-05)", () => {
  it("short, unmistakable go-ahead words are confirmations", () => {
    expect(isExplicitConfirmation("sí")).toBe(true);
    expect(isExplicitConfirmation("dale")).toBe(true);
    expect(isExplicitConfirmation("hazlo")).toBe(true);
    expect(isExplicitConfirmation("sí, constrúyelo")).toBe(true);
    expect(isExplicitConfirmation("ok, adelante")).toBe(true);
  });

  it("a longer message that happens to contain a confirmation word is not a plain go-ahead", () => {
    // A refined/new description, not an agreement to build what was
    // already discussed — must not be misread as "yes, proceed".
    expect(
      isExplicitConfirmation(
        "sí pero mejor que sea con un fondo azul y que tenga un formulario de contacto también",
      ),
    ).toBe(false);
  });

  it("an empty message or an unrelated message is not a confirmation", () => {
    expect(isExplicitConfirmation("")).toBe(false);
    expect(isExplicitConfirmation("cuánto cuesta esto")).toBe(false);
  });
});
