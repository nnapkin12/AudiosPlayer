import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  test: {
    environment: "node",
    // The default forks pool cannot spawn in some sandboxed environments.
    pool: "threads",
    // Component tests opt in with `// @vitest-environment jsdom`.
    environmentMatchGlobs: [["src/**/*.dom.test.{ts,tsx}", "jsdom"]],
    setupFiles: ["./src/test/setup.ts"],
  },
}));
