import { test, expect, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// ---------------------------------------------------------------------------
// Citrate Core — documentation screenshot capture.
//
// The app runs in SIM MODE (bridge.mode === "sim") whenever it loads in a plain
// browser: every surface is populated with fabricated preview data by
// src/bridge/sim/*, so there is no node, no keyring, and nothing to crash. The
// DEV-only `window.__citrateStore` seam (src/main.tsx) lets us drive the
// onboarding state machine and the shell router deterministically.
// ---------------------------------------------------------------------------

const OUT = fileURLToPath(new URL("out/", import.meta.url));
mkdirSync(OUT, { recursive: true });

// Onboarding stages, in order (src/onboarding/Onboarding.tsx).
const ONBOARDING: Array<[string, string]> = [
  ["s0", "welcome"],
  ["s1", "sign-in"],
  ["s2", "verify-identity"],
  ["s3", "membership"],
  ["s4", "wallet-ready"],
  ["s5", "grant-and-stake"],
  ["s6", "node-ignition"],
];

// Shell routes accepted by the hash router (src/App.tsx onHash allowlist).
const SURFACES = [
  "dashboard",
  "wallet",
  "node",
  "models",
  "storage",
  "files",
  "journal",
  "groups",
  "comms",
  "cluster",
  "train",
  "agent",
  "connections",
  "community",
  "commissary",
  "settings",
  "alf",
];

type StorePatch = Record<string, unknown>;

async function patchStore(page: Page, patch: StorePatch) {
  await page.evaluate((p) => {
    const w = window as unknown as {
      __citrateStore?: { setState: (x: Record<string, unknown>) => void; save?: () => void };
    };
    if (!w.__citrateStore) throw new Error("__citrateStore seam missing — is the app in DEV/sim mode?");
    w.__citrateStore.setState(p);
    w.__citrateStore.save?.();
  }, patch);
}

async function bootApp(page: Page) {
  await page.goto("/");
  // The store seam appearing is the definitive "app JS booted in sim mode" signal.
  await expect
    .poll(async () => page.evaluate(() => Boolean((window as any).__citrateStore)), { timeout: 30_000 })
    .toBe(true);
  // App root carries data-register="charter" (several nodes do — take the first).
  await page.locator('[data-register="charter"]').first().waitFor({ state: "visible", timeout: 15_000 });
  // Hide the sim-only "Prototype" affordance so docs shots look like the packaged app.
  await page.addStyleTag({ content: ".sim-proto-affordance{display:none !important}" });
}

async function settle(page: Page, ms = 700) {
  // Let entrance animations and sim data-loads finish before the shot.
  await page.waitForTimeout(ms);
  await page.evaluate(async () => {
    if ((document as any).fonts?.ready) await (document as any).fonts.ready;
  });
}

test("onboarding stages", async ({ page }) => {
  await bootApp(page);
  for (const [stage, name] of ONBOARDING) {
    await patchStore(page, { stage, signedIn: stage !== "s0" && stage !== "s1" });
    await settle(page);
    await page.screenshot({ path: join(OUT, `onboarding-${stage}-${name}.png`) });
  }
});

test("app surfaces", async ({ page }) => {
  await bootApp(page);
  // Land in the completed, signed-in shell so every surface renders.
  await patchStore(page, { stage: "done", signedIn: true });
  await settle(page);
  await page.screenshot({ path: join(OUT, "app-00-shell.png") });

  for (const route of SURFACES) {
    await page.evaluate((r) => {
      window.location.hash = `#/${r}`;
    }, route);
    await settle(page);
    await page.screenshot({ path: join(OUT, `app-${route}.png`) });
  }
});

// Sub-states that a bare surface shot misses — the Hermes agent's tabbed panels.
// The tab is component-local state (not the store seam), so we click it; the button
// always renders in sim mode, keeping this deterministic. Best-effort per tab: a
// missing tab is logged, never a hard failure, so adding/renaming tabs never breaks
// the docs build.
const AGENT_TABS = ["Overview", "Contracts"];

test("agent surface tabs", async ({ page }) => {
  await bootApp(page);
  await patchStore(page, { stage: "done", signedIn: true });
  await page.evaluate(() => {
    window.location.hash = "#/agent";
  });
  await settle(page);
  for (const tab of AGENT_TABS) {
    const btn = page.getByRole("button", { name: tab, exact: true }).first();
    if ((await btn.count()) === 0) {
      // eslint-disable-next-line no-console
      console.log(`[capture] agent tab "${tab}" not found — skipping`);
      continue;
    }
    await btn.click();
    await settle(page, 500);
    await page.screenshot({ path: join(OUT, `app-agent-${tab.toLowerCase()}.png`) });
  }
});
