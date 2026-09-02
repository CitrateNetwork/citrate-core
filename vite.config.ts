import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Build stamp baked at `vite build` time (which `tauri build` runs) so old vs. new installs are always
// distinguishable in-app — the app version alone (0.1.0) is identical across rebuilds. Falls back to
// "unknown" when git isn't available (e.g. CI without full history), never fails the build.
function gitShort(): string {
  try {
    return execSync("git rev-parse --short HEAD", { stdio: ["ignore", "pipe", "ignore"] })
      .toString()
      .trim();
  } catch {
    return "unknown";
  }
}
function appVersion(): string {
  try {
    return JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version ?? "0.0.0";
  } catch {
    return "0.0.0";
  }
}
const BUILD_SHA = gitShort();
// @ts-expect-error process is a nodejs global
const BUILD_TIME = process.env.SOURCE_DATE_EPOCH
  // @ts-expect-error process is a nodejs global
  ? new Date(Number(process.env.SOURCE_DATE_EPOCH) * 1000).toISOString()
  : new Date().toISOString();

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  define: {
    __BUILD_SHA__: JSON.stringify(BUILD_SHA),
    __BUILD_TIME__: JSON.stringify(BUILD_TIME),
    __APP_VERSION__: JSON.stringify(appVersion()),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
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
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
