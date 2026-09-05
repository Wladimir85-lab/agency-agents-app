import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vitest/config";

// sveltekit() added 2026-09-04 (Requisito 6 closure): session.svelte.ts's
// summarizeRunForEsmeralda needed a real unit test, and that file (a) uses
// Svelte 5 runes (`$state`) at module scope, which only the Svelte
// compiler transforms, and (b) is reached via `$lib/types` — both need
// SvelteKit's own Vite pipeline, not just plain esbuild/TS. Kept as the
// same plugin vite.config.js already uses for the real app build, so test
// resolution never drifts from how the app itself actually resolves.
export default defineConfig({
  plugins: [sveltekit()],
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
