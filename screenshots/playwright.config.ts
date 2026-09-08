import { defineConfig, devices } from "@playwright/test";

// Docs screenshot harness for Citrate Core.
//
// Runs the React frontend in SIM MODE (`vite dev` — no Tauri runtime, no node
// process, no keyring) and captures every onboarding stage and app surface to
// PNG for the documentation site. Because nothing native launches, it is safe
// to run on this Linux box or on a Mac and is fully reproducible in CI.
//
// The Vite dev server is started for you (webServer below) on port 1420.
export default defineConfig({
  testDir: ".",
  outputDir: "out/_artifacts",
  timeout: 90_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:1420",
    viewport: { width: 1440, height: 900 }, // desktop-app window proportions
    deviceScaleFactor: 2, // retina-crisp for docs
    colorScheme: "dark",
    // Use Playwright's bundled Chromium (already cached on this machine).
    ...devices["Desktop Chrome"],
    channel: undefined,
  },
  webServer: {
    command: "npm run dev",
    cwd: "..",
    url: "http://localhost:1420",
    reuseExistingServer: true,
    timeout: 120_000,
    // Vite prints to stdout; surface it if the server fails to boot.
    stdout: "pipe",
    stderr: "pipe",
  },
});
