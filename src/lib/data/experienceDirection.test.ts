import { describe, expect, it } from "vitest";

import {
  elementsByDimension,
  findExperienceDirectionElement,
  isEmptyExperienceDirection,
  pickExperienceDirectionElementsDeterministic,
  resolveExperienceDirection,
  type VisualTechnologyElement,
} from "./experienceDirection";

describe("visualTechnology migration — parity with the former CREATIVE_TECH_SPECIALIZATIONS", () => {
  it("carries all 8 families with their ids, status and developmentAgent intact", () => {
    // Same narrowing intentosCapabilities.ts itself uses for this exact
    // dimension (elementsByDimension's declared return type is the base
    // ExperienceDirectionElement; visualTechnology's real backing array is
    // always VisualTechnologyElement, which is where developmentAgent lives).
    const tech = elementsByDimension("visualTechnology") as VisualTechnologyElement[];
    expect(tech.map((t) => t.id).sort()).toEqual(
      [
        "audio-reactive",
        "generative-visuals",
        "physical-interaction",
        "realtime-web-3d",
        "shaders-graphics",
        "simulation-digital-twin",
        "spatial-capture",
        "spatial-xr",
      ].sort(),
    );
    const realtimeWeb3d = tech.find((t) => t.id === "realtime-web-3d");
    expect(realtimeWeb3d?.status).toBe("supported-now");
    expect(realtimeWeb3d?.developmentAgent).toBe("xr-immersive-developer");
    const physicalInteraction = tech.find((t) => t.id === "physical-interaction");
    expect(physicalInteraction?.status).toBe("known-future");
    expect(physicalInteraction?.requiresCapabilityId).toBe("iot");
  });
});

describe("mandatory case 1 (Wladimir's precision) — negation must beat a keyword match", () => {
  const text = "Quiero algo japonés pero no minimalista.";

  it("the deterministic fallback alone WOULD wrongly include Minimalism — this is exactly why it can never be primary", () => {
    const result = pickExperienceDirectionElementsDeterministic(text);
    expect(result.aesthetic.map((e) => e.id)).toContain("minimalism");
  });

  it("resolveExperienceDirection with a semantic verdict never returns an explicitly excluded id, even though the word is in the text", () => {
    const result = resolveExperienceDirection(text, {
      aesthetic: ["japanese-digital", "minimalism"],
      excluded: ["minimalism"],
    });
    expect(result.source).toBe("semantic");
    const ids = result.aesthetic.map((e) => e.id);
    expect(ids).toContain("japanese-digital");
    expect(ids).not.toContain("minimalism");
  });
});

describe("mandatory case 2 (Wladimir's precision) — tagging by meaning, not by substring", () => {
  const text = "Quiero que entrar a esta página se sienta como caminar dentro del astillero.";

  it("the deterministic fallback finds nothing — the text contains none of the catalog's keywords", () => {
    const result = pickExperienceDirectionElementsDeterministic(text);
    expect(result.experience).toEqual([]);
  });

  it("resolveExperienceDirection trusts a semantic tag by meaning even with zero literal keyword overlap", () => {
    const result = resolveExperienceDirection(text, { experience: ["immersive", "spatial-ui"] });
    expect(result.source).toBe("semantic");
    expect(result.experience.map((e) => e.id).sort()).toEqual(["immersive", "spatial-ui"]);
  });
});

describe("resolveExperienceDirection — presence, not non-emptiness, decides authority", () => {
  const text = "Quiero un catálogo de productos con fotos bonitas.";

  it("falls back to deterministic matching when semantic is entirely absent (model unavailable)", () => {
    const result = resolveExperienceDirection(text, undefined);
    expect(result.source).toBe("deterministic-fallback");
  });

  it("an empty-but-present semantic object is still authoritative — never topped up by the keyword fallback", () => {
    const result = resolveExperienceDirection("Algo cinematográfico y japonés.", {});
    expect(result.source).toBe("semantic");
    expect(isEmptyExperienceDirection(result)).toBe(true);
  });

  it("a semantic id outside the closed catalog is dropped, same discipline as capabilityId elsewhere", () => {
    const result = resolveExperienceDirection(text, { aesthetic: ["quantum-vaporwave"] });
    expect(result.aesthetic).toEqual([]);
  });

  it("a semantic id from the wrong dimension is dropped rather than silently miscategorized", () => {
    const result = resolveExperienceDirection(text, { aesthetic: ["realtime-web-3d"] });
    expect(result.aesthetic).toEqual([]);
  });
});

describe("findExperienceDirectionElement / elementsByDimension", () => {
  it("finds a known id and reports its real dimension", () => {
    expect(findExperienceDirectionElement("cinematic")?.dimension).toBe("experience");
  });

  it("returns undefined for an unknown id", () => {
    expect(findExperienceDirectionElement("does-not-exist")).toBeUndefined();
  });

  it("every seeded element resolves to exactly one dimension bucket", () => {
    for (const dimension of ["aesthetic", "experience", "visualTechnology"] as const) {
      for (const element of elementsByDimension(dimension)) {
        expect(element.dimension).toBe(dimension);
      }
    }
  });
});
