/** Derives a per-project conversation thread from already-persisted runs.
 *  No new backend state: `runs.list` (see runs.svelte.ts) already accumulates
 *  every run ever started, so "the thread for this project" is just that
 *  list filtered and sorted — nothing new to fetch or store. */
import { runs } from "./runs.svelte";
import type { RunSummary } from "$lib/types";

export function turnsForProject(projectPath: string): RunSummary[] {
  if (!projectPath) return [];
  return runs.list
    .filter((run) => run.projectPath === projectPath)
    .slice()
    .sort((a, b) => new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime());
}
