import { fileURLToPath } from "node:url";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// Component tests run locally with mocked Tauri IPC, never research/model calls.
export default defineConfig({
  plugins: [svelte({ hot: false })],
  resolve: {
    alias: { $lib: fileURLToPath(new URL("./src/lib", import.meta.url)) },
    conditions: ["browser"],
  },
  test: { environment: "jsdom", include: ["src/**/*.component.test.ts"] },
});
