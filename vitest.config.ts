import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// CORE-A1 A1.5 — frontend test runner. jsdom gives the bridge mode-detector a
// `window` to probe; tests mock `invoke` at the boundary so no Tauri runtime is
// needed.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    globals: false,
  },
});
