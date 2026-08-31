/**
 * Structural link between the two "8 ramos" taxonomies that intentionally
 * coexist in this codebase — see memory-bank/intentosAgencyModel.md and the
 * 2026-08-23 decision to keep them separate:
 *
 *   - `INTENTOS_CAPABILITIES` (intentosCapabilities.ts) is Runtime v0.1's
 *     execution contract: a fixed 5-agent team + stage labels per capability.
 *     Its `id` values are WIRE FORMAT — persisted in run evidence
 *     (`state/runs/<uuid>.json`) and matched literally in the Rust backend
 *     (`runtime.rs`, e.g. `run.capability_id == "iot"`). FROZEN since commit
 *     404f4b1 — never rename these ids from this file.
 *   - `AGENCY_RAMAS` (agencyOrganization.ts) is a browsing/exploration lens
 *     over the full catalog — an open pool of specialists per ramo, used only
 *     by the Divisions UI (`corpus.organizationTiles("ramos")`). Its `slug`
 *     values are UI-only and free to evolve.
 *
 * These are NOT functional duplicates: one is a fixed execution team, the
 * other an open browsing pool. This file changes neither taxonomy's
 * behavior, and nothing imports it — its only job is to let TypeScript
 * (via `npm run check`, already the gate this repo runs before closing any
 * line of work) catch accidental drift the moment it happens:
 *
 *   - a capability added/removed/renamed in intentosCapabilities.ts without
 *     a matching entry below → compile error (missing/excess object key);
 *   - a ramo slug that no capability links to any more → the exhaustiveness
 *     check at the bottom fails to compile.
 *
 * The correspondence is NOT string-derivable (compare `tinyml-edge-ai` to
 * `ramo-tinyml`), so it is written out by hand, once, here.
 */

import { INTENTOS_CAPABILITIES } from "./intentosCapabilities";
import { AGENCY_RAMAS } from "./agencyOrganization";

type CapabilityId = (typeof INTENTOS_CAPABILITIES)[number]["id"];
type RamoSlug = (typeof AGENCY_RAMAS)[number]["slug"];

/**
 * One entry per Runtime capability, pointing at the ramo slug that
 * represents the same real-world area of work. TypeScript requires every
 * `CapabilityId` to appear exactly once as a key here — a capability added
 * or removed in `intentosCapabilities.ts` without updating this table fails
 * `npm run check` immediately (missing- or excess-property error).
 */
export const RAMO_CAPABILITY_LINKS: Record<CapabilityId, RamoSlug> = {
  "digital-experience": "ramo-digital-experience",
  "systems-data": "ramo-systems-data",
  "iot": "ramo-iot",
  "tinyml-edge-ai": "ramo-tinyml",
  "cybersecurity": "ramo-cybersecurity",
  "devops-quality": "ramo-devops-quality",
  "creative-technology": "ramo-creative-technology",
  "strategy-product": "ramo-strategy-product",
};

/**
 * Compile-time-only check for the other direction: every `RamoSlug` must
 * appear at least once among the values above (a ramo may receive more than
 * one capability; it just can't receive zero). `Unlinked` resolves to a real,
 * non-`never` type — and the assignment below stops compiling — the moment a
 * ramo is added, removed, or renamed in `agencyOrganization.ts` without a
 * matching update here. Nothing here executes at runtime; a broken link only
 * ever shows up as an `npm run check` / `tsc` error, naming the unlinked
 * slug(s) directly in the error message.
 *
 * IMPORTANT: do not add `as AllRamosLinked` (or any assertion) to the const
 * below — the whole check depends on the plain literal `true` being
 * type-checked for real against the computed type.
 */
type LinkedRamoSlugs = (typeof RAMO_CAPABILITY_LINKS)[CapabilityId];
type Unlinked = Exclude<RamoSlug, LinkedRamoSlugs>;
type AllRamosLinked = Unlinked extends never
  ? true
  : ["agencyRamosLink: ramo(s) with no linked Runtime capability — update RAMO_CAPABILITY_LINKS:", Unlinked];

export const RAMO_LINKS_ARE_EXHAUSTIVE: AllRamosLinked = true;
