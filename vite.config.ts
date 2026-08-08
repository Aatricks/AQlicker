import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    // Git worktrees carry their own copy of the tree and of node_modules, so
    // they would otherwise be collected twice and against the wrong React.
    exclude: ["**/node_modules/**", "**/dist/**", "**/.worktrees/**"],
  },
});
