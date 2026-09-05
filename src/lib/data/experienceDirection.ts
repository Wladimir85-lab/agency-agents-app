/** Experience Direction System — 2026-09-06 architectural mandate.
 *
 * Names, as real data, the three dimensions a creative/experiential intent
 * actually mixes today inside `creative-technology`'s flat `keywords`
 * (see intentosCapabilities.ts): how a product should LOOK (`aesthetic`),
 * how it should FEEL/be navigated (`experience`), and what it is
 * materialized WITH (`visualTechnology`). They are combinable — a single
 * intent can touch one element from each — never mutually exclusive
 * "styles" in one list.
 *
 * Deliberately NOT a new capability, NOT a new pipeline stage, NOT a new
 * LLM call. `visualTechnology` migrates `CREATIVE_TECH_SPECIALIZATIONS`
 * verbatim (same ids/status/note); `aesthetic` and `experience` are new,
 * small, explicitly-not-closed seed catalogs (mandate §2). Interpretation
 * authority stays with the existing semantic layer
 * (`classifyIntentSemantic` in intentosCapabilities.ts) — see
 * `resolveExperienceDirection` below: keyword matching here is a
 * deterministic FALLBACK only, used exactly when the semantic call is
 * unavailable, never as the primary classifier. This is the explicit
 * correction Wladimir made before approving the plan: a keyword-first
 * classifier would silently ignore negation ("japonés pero no
 * minimalista") and could never tag an intent by meaning alone
 * ("caminar dentro del astillero" -> Immersive, no matching words).
 *
 * Deeper reference material (real computational cost of 3DGS, browser
 * support matrices, etc.) deliberately does NOT belong in this file —
 * mandate §7's progressive disclosure is already built: drop that
 * material into the Biblioteca de conocimiento (knowledge.rs) and
 * `stage_knowledge` (runtime.rs) surfaces it to architecture/development
 * stages automatically. Nothing new to wire here.
 */

export type ExperienceDimension = "aesthetic" | "experience" | "visualTechnology";
export type ExperienceDirectionStatus = "supported-now" | "known-future";

export interface ExperienceDirectionElement {
  id: string;
  label: string;
  dimension: ExperienceDimension;
  definition: string;
  /** Short phrases for the deterministic fallback match ONLY (see module
   *  doc). Never treated as the primary way to detect this element. */
  keywords: string[];
  avoidWhen?: string;
  /** Which real IntentOSCapability materializes this element, when one
   *  applies — e.g. "creative-technology", or "iot" for physical work.
   *  Keeps materialization flowing through the real capability→pipeline→
   *  agent machinery instead of inventing a persona per buzzword
   *  (mandate §8). */
  requiresCapabilityId?: string;
  /** Same discipline as CreativeTechSpecializationStatus: "supported-now"
   *  requires a real corpus persona AND a real way for IntentOS to build/
   *  run/verify the result — not "the LLM could probably improvise it". */
  status: ExperienceDirectionStatus;
  /** Freeform detail — tooling notes, fallbacks, known tensions. Plays the
   *  role the mandate's proposed 16-field schema would have split into
   *  separate structured fields (compatibilities, costs, accessibility...);
   *  kept as prose deliberately (see module doc / plan Decision 1) until a
   *  real conversation needs one of those fields to be queryable on its
   *  own, not before. */
  note?: string;
}

// ---------- Aesthetic (¿cómo debe verse?) — initial seed, not closed ----------

const AESTHETIC_ELEMENTS: ExperienceDirectionElement[] = [
  {
    id: "japanese-digital",
    label: "Japanese Digital Aesthetic",
    dimension: "aesthetic",
    definition: "Composición serena, mucho espacio en blanco, tipografía editorial, paleta contenida — calma y precisión antes que densidad visual.",
    keywords: ["japonés", "japones", "japonesa", "wabi-sabi", "zen", "minimalista serena"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
    note: "Materializable con el stack web actual (CSS/tipografía/espaciado); no requiere tecnología especial por sí sola.",
  },
  {
    id: "editorial",
    label: "Editorial",
    dimension: "aesthetic",
    definition: "Jerarquía tipográfica fuerte, grillas de revista/impreso, texto como protagonista visual.",
    keywords: ["editorial", "revista", "tipográfico", "tipografico"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "brutalism",
    label: "Brutalism / Neo-Brutalism",
    dimension: "aesthetic",
    definition: "Crudo, estructural, bordes duros, tipografía sin adorno, rechazo deliberado del pulido convencional.",
    keywords: ["brutalismo", "brutalist", "neo-brutalismo", "crudo"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "minimalism",
    label: "Minimalism",
    dimension: "aesthetic",
    definition: "Lo mínimo indispensable — poco color, poco ornamento, foco total en el contenido esencial.",
    keywords: ["minimalista", "minimalismo", "minimal", "sobrio"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "cyberpunk",
    label: "Cyberpunk",
    dimension: "aesthetic",
    definition: "Neón, alto contraste, estética tecnológica/distópica, tipografía futurista.",
    keywords: ["cyberpunk", "neón", "neon", "futurista distópico"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "luxury",
    label: "Luxury",
    dimension: "aesthetic",
    definition: "Espacio generoso, fotografía a pantalla completa, tono sobrio y de alta gama — comunica exclusividad, no volumen.",
    keywords: ["lujo", "premium", "alta gama", "exclusivo"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
];

// ---------- Experience (¿cómo debe sentirse/recorrerse?) — initial seed ----------

const EXPERIENCE_ELEMENTS: ExperienceDirectionElement[] = [
  {
    id: "immersive",
    label: "Immersive",
    dimension: "experience",
    definition: "Busca absorción/presencia del usuario en el contenido. No implica necesariamente 3D — puede lograrse con video, audio, motion y composición.",
    keywords: ["inmersivo", "inmersiva", "absorbente", "presencia", "sumergirse"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "cinematic",
    label: "Cinematic",
    dimension: "experience",
    definition: "Lenguaje de secuencia, encuadre, transición, ritmo, profundidad y movimiento tomado del cine.",
    keywords: ["cinematográfico", "cinematografico", "cinematográfica", "cinematica"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "scrollytelling",
    label: "Scrollytelling",
    dimension: "experience",
    definition: "El desplazamiento (scroll) como mecanismo narrativo/interactivo, no solo de navegación.",
    keywords: ["scrollytelling", "scroll narrativo", "narrativa por scroll"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "spatial-ui",
    label: "Spatial / Spatial UI",
    dimension: "experience",
    definition: "Usa posición, profundidad y relaciones espaciales como parte real de la interfaz, no solo decoración.",
    keywords: ["espacial", "spatial ui", "profundidad", "layout espacial"],
    status: "supported-now",
    requiresCapabilityId: "digital-experience",
  },
  {
    id: "interactive-3d-experience",
    label: "Interactive 3D Experience",
    dimension: "experience",
    definition: "Permite explorar o manipular objetos/entornos tridimensionales — el usuario controla el punto de vista.",
    keywords: ["3d interactivo", "explorar en 3d", "recorrer en 3d", "girar", "rotar"],
    status: "supported-now",
    requiresCapabilityId: "creative-technology",
    note: "Cuando esta experiencia es real (no solo mencionada), normalmente requiere un elemento de la dimensión visualTechnology (ej. realtime-web-3d) para materializarse.",
  },
];

// ---------- Visual technology (¿con qué medio se materializa?) ----------
//
// Migrated verbatim from the former CREATIVE_TECH_SPECIALIZATIONS (same
// ids/status/keywords/note/developmentAgent) — this dimension already
// existed, it just wasn't named as one of three. See intentosCapabilities.ts
// for `pickCreativeTechSpecializations`, kept as a thin compatibility
// wrapper over this list.

export interface VisualTechnologyElement extends ExperienceDirectionElement {
  dimension: "visualTechnology";
  /** Preserves the former CreativeTechSpecialization field IntentOS uses to
   *  pick which real corpus persona builds this — see composePipeline's
   *  specializationId wiring in intentosCapabilities.ts. */
  developmentAgent: string;
}

const VISUAL_TECHNOLOGY_ELEMENTS: VisualTechnologyElement[] = [
  {
    id: "realtime-web-3d",
    label: "3D en tiempo real para navegador",
    dimension: "visualTechnology",
    definition: "Geometría 3D real y navegable renderizada en el navegador (WebGL/WebGPU/Three.js).",
    keywords: ["three.js", "threejs", "webgl", "webgpu", "3d interactivo", "girar", "rotar", "recorrer", "recorrido 360", "explorar en 3d", "visor 3d", "modelo 3d"],
    status: "supported-now",
    requiresCapabilityId: "creative-technology",
    developmentAgent: "xr-immersive-developer",
    note: "Probado en producción: Naval Studio (three ^0.169.0 + rhino3dm ^8.32.2, viewer3d-client.ts) — ver corpus/spatial-computing/xr-immersive-developer.md.",
  },
  {
    id: "shaders-graphics",
    label: "Shaders y gráficos en tiempo real (web)",
    dimension: "visualTechnology",
    definition: "Programas GPU (GLSL) para geometría, materiales, píxeles, iluminación, partículas y efectos de post-procesado.",
    keywords: ["shader", "glsl", "webgl", "efecto visual", "post-procesado", "post procesado"],
    status: "supported-now",
    requiresCapabilityId: "creative-technology",
    developmentAgent: "xr-immersive-developer",
    note: "El mismo persona web (WebXR/Three.js) cubre shader tuning para navegador. Shader authoring específico de motor (Unity Shader Graph, Godot) es known-future: IntentOS no construye ni ejecuta proyectos de esos motores.",
  },
  {
    id: "spatial-xr",
    label: "AR/VR/XR nativo (headset, fuera del navegador)",
    dimension: "visualTechnology",
    definition: "Realidad aumentada/virtual nativa, fuera del navegador (Vision Pro, Quest nativo, HoloLens).",
    keywords: ["realidad aumentada nativa", "realidad virtual nativa", "vision pro", "visionos", "hololens", "meta quest nativo", "oculus nativo"],
    status: "known-future",
    requiresCapabilityId: "creative-technology",
    developmentAgent: "xr-immersive-developer",
    note: "Personas reales existen (corpus/spatial-computing/visionos-spatial-engineer.md, macos-spatial-metal-engineer.md) pero IntentOS no tiene toolchain nativo (Xcode/build/simulador) para construir, ejecutar ni verificar el resultado. XR dentro del navegador (WebXR) es 'realtime-web-3d', no esto.",
  },
  {
    id: "generative-visuals",
    label: "Arte/visuales generativos",
    dimension: "visualTechnology",
    definition: "Sistemas visuales creados mediante reglas y variación (creative coding, arte procedural).",
    keywords: ["arte generativo", "generativo", "procedural", "creative coding"],
    status: "known-future",
    requiresCapabilityId: "creative-technology",
    developmentAgent: "xr-immersive-developer",
    note: "Sin persona dedicada en el corpus (no hay 'creative coder'/generative-art specialist) ni precedente ejecutado. El generalista WebXR podría intentarlo, pero sin prueba real no se presenta como supported-now.",
  },
  {
    id: "spatial-capture",
    label: "Captura 3D del mundo físico (fotogrametría/LiDAR)",
    dimension: "visualTechnology",
    definition: "Reconstrucción 3D de objetos/espacios reales vía fotogrametría o LiDAR.",
    keywords: ["lidar", "fotogrametría", "fotogrametria", "escaneo 3d", "nube de puntos", "point cloud"],
    status: "known-future",
    requiresCapabilityId: "creative-technology",
    developmentAgent: "xr-immersive-developer",
    note: "La mitad web (visualizar la nube de puntos/malla resultante en el navegador) es 'realtime-web-3d', ya probada. La captura nativa (ARKit/LiDAR en un dispositivo) requiere una app móvil nativa que IntentOS no construye ni despliega hoy.",
  },
  {
    id: "simulation-digital-twin",
    label: "Simulación / gemelo digital",
    dimension: "visualTechnology",
    definition: "Réplica computacional interactiva de un sistema o proceso físico real.",
    keywords: ["gemelo digital", "digital twin", "simulación física", "simulacion fisica"],
    status: "known-future",
    requiresCapabilityId: "creative-technology",
    developmentAgent: "xr-immersive-developer",
    note: "Sin persona dedicada ni precedente ejecutado. Marcado explícitamente known-future en vez de improvisar una afirmación de capacidad.",
  },
  {
    id: "audio-reactive",
    label: "Audiovisual reactivo",
    dimension: "visualTechnology",
    definition: "Visuales que responden en tiempo real a audio o sonido.",
    keywords: ["audio reactivo", "audio-reactivo", "reactivo al sonido", "visualizador de audio"],
    status: "known-future",
    requiresCapabilityId: "creative-technology",
    developmentAgent: "xr-immersive-developer",
    note: "Web Audio API + Three.js lo haría técnicamente posible, pero sin persona dedicada ni precedente ejecutado no se presenta como supported-now.",
  },
  {
    id: "physical-interaction",
    label: "Instalación física / computación física",
    dimension: "visualTechnology",
    definition: "Proyección mapeada, kioscos interactivos, sensores físicos — computación que actúa en el espacio físico.",
    keywords: ["instalación física", "instalacion fisica", "projection mapping", "proyección mapeada", "kiosco interactivo", "sensor físico"],
    status: "known-future",
    requiresCapabilityId: "iot",
    developmentAgent: "xr-immersive-developer",
    note: "IntentOS ya tiene una capability propia y operativa para esto ('iot', con sus propios agentes y su propio domain_verification_guidance en runtime.rs). Un intent predominantemente físico/IoT debe enrutarse ahí, no a creative-technology.",
  },
];

export const EXPERIENCE_DIRECTION_CATALOG: ExperienceDirectionElement[] = [
  ...AESTHETIC_ELEMENTS,
  ...EXPERIENCE_ELEMENTS,
  ...VISUAL_TECHNOLOGY_ELEMENTS,
];

export function findExperienceDirectionElement(id: string): ExperienceDirectionElement | undefined {
  return EXPERIENCE_DIRECTION_CATALOG.find((element) => element.id === id);
}

export function elementsByDimension(dimension: ExperienceDimension): ExperienceDirectionElement[] {
  return EXPERIENCE_DIRECTION_CATALOG.filter((element) => element.dimension === dimension);
}

/** Renders the closed catalog as prompt text for `classifyIntentSemantic`
 *  — same "elige de esta lista cerrada, nunca inventes un id" discipline
 *  already used for `capabilityId`. */
export function experienceDirectionPromptCatalog(): string {
  const section = (dimension: ExperienceDimension, heading: string) =>
    `${heading}:\n${elementsByDimension(dimension).map((e) => `- ${e.id}: ${e.label} — ${e.definition}`).join("\n")}`;
  return [
    section("aesthetic", "ESTÉTICA (cómo debe verse)"),
    section("experience", "EXPERIENCIA (cómo debe sentirse/recorrerse)"),
    section("visualTechnology", "TECNOLOGÍA VISUAL (con qué se materializa)"),
  ].join("\n\n");
}

export interface ExperienceDirectionResult {
  aesthetic: ExperienceDirectionElement[];
  experience: ExperienceDirectionElement[];
  visualTechnology: ExperienceDirectionElement[];
  /** Which path produced this result — mirrors `RoutingTrace.source`'s
   *  audit-trail role for capability selection. */
  source: "semantic" | "deterministic-fallback";
}

/** The shape `classifyIntentSemantic` should ask for and parse, alongside
 *  its existing capabilityId/creativeTechnologyJustified fields — same
 *  call, same prompt, no new model invocation. `excluded` carries ids the
 *  model determined the user explicitly negated (e.g. "pero no
 *  minimalista"); those must never appear in the positive arrays and must
 *  never be reintroduced by the deterministic fallback. */
export interface ExperienceDirectionSemanticPayload {
  aesthetic?: string[];
  experience?: string[];
  visualTechnology?: string[];
  excluded?: string[];
}

/** Deterministic FALLBACK ONLY (see module doc) — a same-role sibling of
 *  the old `pickCreativeTechSpecializations`, generalized across all three
 *  dimensions. Never called when a semantic result is present; has no
 *  concept of negation or meaning beyond substring matching, which is
 *  exactly why it must never be the primary interpreter. */
export function pickExperienceDirectionElementsDeterministic(text: string): ExperienceDirectionResult {
  const normalized = text.toLocaleLowerCase("es");
  const matchDimension = (dimension: ExperienceDimension) =>
    elementsByDimension(dimension).filter((element) => element.keywords.some((phrase) => normalized.includes(phrase)));
  return {
    aesthetic: matchDimension("aesthetic"),
    experience: matchDimension("experience"),
    visualTechnology: matchDimension("visualTechnology"),
    source: "deterministic-fallback",
  };
}

/** The one entry point callers use. `semantic` is the `experienceDirection`
 *  sub-object from a `classifyIntentSemantic` response (undefined when
 *  that call never ran, or the model omitted the field entirely) —
 *  presence, not non-emptiness, decides authority: a model reply that
 *  explicitly found nothing (`{}`) or only an `excluded` list is still
 *  authoritative and must NOT be topped up by the keyword fallback, which
 *  is exactly how a real negation ("pero no minimalista") could otherwise
 *  silently come back from the fallback path. */
export function resolveExperienceDirection(
  text: string,
  semantic?: ExperienceDirectionSemanticPayload | null,
): ExperienceDirectionResult {
  if (semantic) {
    const excluded = new Set(semantic.excluded ?? []);
    const resolveDimension = (ids: string[] | undefined, dimension: ExperienceDimension) =>
      (ids ?? [])
        .filter((id) => !excluded.has(id))
        .map((id) => findExperienceDirectionElement(id))
        .filter((element): element is ExperienceDirectionElement => element != null && element.dimension === dimension);
    return {
      aesthetic: resolveDimension(semantic.aesthetic, "aesthetic"),
      experience: resolveDimension(semantic.experience, "experience"),
      visualTechnology: resolveDimension(semantic.visualTechnology, "visualTechnology"),
      source: "semantic",
    };
  }
  return pickExperienceDirectionElementsDeterministic(text);
}

/** True when a result has nothing in any dimension — used to decide
 *  whether `SolutionProposal.experienceDirection` should be `null` (never
 *  clutter a proposal that has nothing to do with creative direction). */
export function isEmptyExperienceDirection(result: ExperienceDirectionResult): boolean {
  return result.aesthetic.length === 0 && result.experience.length === 0 && result.visualTechnology.length === 0;
}
