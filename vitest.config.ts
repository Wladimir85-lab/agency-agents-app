import { defineConfig } from "vitest/config";

// Deliberately minimal and separate from vite.config.js's sveltekit()
// plugin: the units this project currently unit-tests (intentosCapabilities.ts)
// have no $app/$lib-aliased imports of their own, so pulling in the full
// SvelteKit dev pipeline just to resolve them would be complexity this repo
// doesn't need yet. Add sveltekit() here (or point tests at it) the day a
// test actually needs `$lib`/`$app` resolution.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
