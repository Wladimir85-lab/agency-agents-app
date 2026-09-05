import { beforeEach, describe, expect, it, vi } from "vitest";

// `invoke` must be mocked before the module under test is imported, since
// intentosCapabilities.ts imports it at module scope (same pattern the rest
// of the app uses to reach `local_model_complete` — see local_model.rs).
// vi.hoisted is required here (not a plain outer const) because vi.mock's
// factory is itself hoisted above regular imports by vitest.
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  classifyConversationalIntent,
  classifyConversationalIntentDeterministic,
  composePipeline,
  CREATIVE_TECH_SPECIALIZATIONS,
  INTENTOS_CAPABILITIES,
  isBuildNegation,
  isConversationalMessage,
  isExplicitConfirmation,
  isTeamCompositionQuestion,
  planSolution,
  planSolutionAsync,
  routeIntent,
  type PendingBuildProposal,
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

// Regression suite for the real conversation Wladimir reported live,
// 2026-09-05: he asked Esmeralda to hypothetically describe the team for a
// build, explicitly saying not to build anything yet — she kept re-asking
// to confirm a build across several turns anyway. Root cause: the
// classifier had no concept of negation or hypothetical framing, so every
// reply (including his refusals, which themselves contained "construyas")
// was misread as a new build instruction.
describe("real bug, 2026-09-05: hypothetical team planning + explicit negation must never enter the build-confirmation flow", () => {
  it("the exact reported opening message is chat, not a build instruction", () => {
    expect(
      isConversationalMessage(
        "Diseña hipotéticamente el equipo necesario para construir un sistema de gestión de inventario. No construyas nada todavía; solo explícame cómo organizarías el equipo.",
      ),
    ).toBe(true);
  });

  it("each of his actual follow-up refusals is chat, not a new build description", () => {
    expect(isConversationalMessage("aún no construyamos")).toBe(true);
    expect(isConversationalMessage("no construyas nada")).toBe(true);
    expect(isConversationalMessage("no construyas nada todavía")).toBe(true);
    expect(isConversationalMessage("todavía no lo hagas")).toBe(true);
  });

  it("those same refusals are recognized as explicit build negations, not just generic chat", () => {
    // The distinction matters: isBuildNegation is what tells sendTurn to
    // clear a pending build description instead of silently replacing it
    // with the refusal text (the actual loop Wladimir hit).
    expect(isBuildNegation("aún no construyamos")).toBe(true);
    expect(isBuildNegation("no construyas nada")).toBe(true);
    expect(isBuildNegation("no construyas nada todavía")).toBe(true);
    expect(isBuildNegation("todavía no lo hagas")).toBe(true);
    expect(isBuildNegation("quiero construir un blog")).toBe(false);
  });

  it("a purely hypothetical planning question, with no negation at all, is still chat", () => {
    expect(isConversationalMessage("¿Cómo organizarías el equipo para construir un sistema de reservas?")).toBe(true);
    expect(isConversationalMessage("hipotéticamente, ¿qué equipo usarías para esto?")).toBe(true);
  });

  it("his exact follow-up asking who would be convened is recognized as a team-composition question", () => {
    expect(isTeamCompositionQuestion("solo dime a quiénes convocas")).toBe(true);
    expect(isTeamCompositionQuestion("¿qué equipo necesitaría para un sistema de inventario?")).toBe(true);
    expect(isTeamCompositionQuestion("hola, ¿cómo estás?")).toBe(false);
  });

  it("a real, direct build instruction is never swallowed by the new hypothetical/negation patterns", () => {
    // The fix must not overcorrect — an actual order to build still builds.
    expect(isConversationalMessage("quiero construir un blog personal ahora mismo")).toBe(false);
    expect(isConversationalMessage("necesito un dashboard administrativo")).toBe(false);
    expect(isBuildNegation("quiero construir un blog personal ahora mismo")).toBe(false);
  });
});

// 2026-09-05 cognitive-architecture mandate ("CEREBRO COGNITIVO DE
// ESMERALDA"): the regex classifier above is now only the deterministic
// fallback. These tests cover the mandate's own adversarial phrase list
// (its section 12) against `classifyConversationalIntentDeterministic`
// directly, and separately verify the semantic layer's plumbing
// (`classifyConversationalIntent`) — prompt content, JSON parsing,
// validation, and fail-closed fallback.
describe("classifyConversationalIntentDeterministic — mandate §12 phrase battery (fallback path, no model)", () => {
  const noPending: PendingBuildProposal | null = null;
  const pending: PendingBuildProposal = { text: "una tienda online", restrictions: [] };

  it("a direct order to build is 'execute'", () => {
    expect(classifyConversationalIntentDeterministic("Construye una tienda.", noPending).act).toBe("execute");
    expect(classifyConversationalIntentDeterministic("Diseña y construye la aplicación.", noPending).act).toBe("execute");
  });

  it("an explicit negation is 'cancel', with negated: true", () => {
    const result = classifyConversationalIntentDeterministic("No construyas una tienda.", pending);
    expect(result.act).toBe("cancel");
    expect(result.negated).toBe(true);
    expect(classifyConversationalIntentDeterministic("Todavía no.", pending).act).toBe("cancel");
  });

  it("a question naming its own team-planning verb is non-executive ('explain', via the team-composition check)", () => {
    // "primero dime qué equipo usarías" matches the team-composition
    // pattern (checked before the generic hypothetical pattern) — either
    // way it must never become 'execute' despite containing "construir".
    const result = classifyConversationalIntentDeterministic("Quiero construir una tienda; primero dime qué equipo usarías.", noPending);
    expect(result.act).not.toBe("execute");
    expect(result.act).toBe("explain");
  });

  it("'Sí, pero solo explícame.' never becomes an execute confirmation, even with a proposal pending", () => {
    // The mandate's own adversarial case: a bare confirmation word
    // ('sí') wrapped around an explanation-only request must not trigger
    // a real build just because it's short and contains 'sí'.
    const result = classifyConversationalIntentDeterministic("Sí, pero solo explícame.", pending);
    expect(result.act).not.toBe("execute");
    expect(result.act).not.toBe("confirm");
  });

  it("a bare 'Sí.' or 'Ahora sí.' against a real pending proposal is 'confirm'", () => {
    expect(classifyConversationalIntentDeterministic("Sí.", pending).act).toBe("confirm");
    expect(classifyConversationalIntentDeterministic("Ahora sí.", pending).act).toBe("confirm");
  });

  it("the same 'Sí.' with nothing pending is never treated as a confirmation to build", () => {
    expect(classifyConversationalIntentDeterministic("Sí.", noPending).act).not.toBe("confirm");
  });

  it("'Cancela.' and 'Continúa.' read as cancel/execute respectively when something is pending", () => {
    expect(classifyConversationalIntentDeterministic("Cancela.", pending).act).toBe("cancel");
  });

  it("known, accepted fallback limitation: rhetorical/hypothetical phrasing that reuses a build verb without a recognized hedge word degrades to 'execute', never silently to something unsafe", () => {
    // '¿Cómo construirías una tienda?', '¿Puedes construir una tienda?' and
    // 'Imagínate que construimos una tienda.' are all genuinely
    // hypothetical per the mandate, but none contain a word the
    // deterministic patterns recognize as a hedge (no 'hipotéticamente',
    // no 'organizarías/armarías/compondrías', no 'imagínate' keyword). The
    // semantic layer (tested below) resolves these correctly; this
    // fallback path only exists for when that layer is unavailable, and
    // its worst-case failure mode here is asking for confirmation before
    // building — never an unconfirmed build, never a crash, never a loop.
    expect(classifyConversationalIntentDeterministic("¿Cómo construirías una tienda?", noPending).act).toBe("execute");
    expect(classifyConversationalIntentDeterministic("¿Puedes construir una tienda?", noPending).act).toBe("execute");
  });
});

describe("classifyConversationalIntent — semantic layer plumbing (mocked model)", () => {
  it("uses the model's structured verdict when it replies with valid JSON", async () => {
    invokeMock.mockResolvedValueOnce(
      semanticReply({ act: "plan", negated: false, restrictions: [], confidence: 0.82, reason: "Pregunta hipotética sobre el equipo." }),
    );
    const result = await classifyConversationalIntent("¿Cómo construirías una tienda?", [], null);
    expect(result.act).toBe("plan");
    expect(result.confidence).toBe(0.82);
  });

  it("correctly resolves the mandate's hardest case — 'Sí, pero solo explícame.' — via the model, not the regex fallback", async () => {
    invokeMock.mockResolvedValueOnce(
      semanticReply({ act: "explain", negated: false, restrictions: [], confidence: 0.75, reason: "Acepta que se le explique, no que se ejecute." }),
    );
    const pending: PendingBuildProposal = { text: "una tienda online", restrictions: [] };
    const result = await classifyConversationalIntent("Sí, pero solo explícame.", [], pending);
    expect(result.act).toBe("explain");
  });

  it("carries the pending proposal and recent conversation into the prompt", async () => {
    invokeMock.mockResolvedValueOnce(semanticReply({ act: "execute", negated: false, restrictions: [], confidence: 0.9, reason: "ok" }));
    const pending: PendingBuildProposal = { text: "una landing page de fotografía", restrictions: [] };
    await classifyConversationalIntent("dale, hazlo", [{ role: "user", content: "quiero algo simple" }], pending);
    const sentPrompt = invokeMock.mock.calls[0][1].request.prompt as string;
    expect(sentPrompt).toContain("una landing page de fotografía");
    expect(sentPrompt).toContain("quiero algo simple");
  });

  it("captures explicit restrictions the model extracts, e.g. 'pero no implementes nada todavía'", async () => {
    invokeMock.mockResolvedValueOnce(
      semanticReply({ act: "execute", negated: false, restrictions: ["no implementes nada todavía"], confidence: 0.7, reason: "Pide diseñar, con una restricción explícita." }),
    );
    const result = await classifyConversationalIntent("Diseña la arquitectura, pero no implementes nada todavía.", [], null);
    expect(result.act).toBe("execute");
    expect(result.restrictions).toContain("no implementes nada todavía");
  });

  it("falls back to the deterministic classifier when the model call fails", async () => {
    invokeMock.mockRejectedValueOnce(new Error("Paranoid Mode is on"));
    const result = await classifyConversationalIntent("Construye una tienda.", [], null);
    expect(result.act).toBe("execute");
  });

  it("falls back to the deterministic classifier when the model returns an unrecognized act", async () => {
    invokeMock.mockResolvedValueOnce(semanticReply({ act: "delete_everything", confidence: 0.9 }));
    const result = await classifyConversationalIntent("No construyas una tienda.", [], { text: "x", restrictions: [] });
    expect(result.act).toBe("cancel"); // deterministic fallback still gets this one right
  });

  it("falls back to the deterministic classifier when the model returns unparseable content", () => {
    invokeMock.mockResolvedValueOnce({ content: "no soy json" });
    return classifyConversationalIntent("Construye una tienda.", [], null).then((result) => {
      expect(result.act).toBe("execute");
    });
  });
});
