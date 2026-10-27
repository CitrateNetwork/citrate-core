// created: 2026-10-23 | branch: test/join-e2e-gate | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
// status: active. Join-E2E harness only (scripts/ci/join-e2e.sh).
//
// Step 1 of the gate (link -> join page): open the share link in a real headless Chromium, the way
// an invitee's browser does, and record what the page offers once its client code has read the key
// from the fragment. Run from the landing checkout (uses its playwright-core):
//
//   node page-check.mjs <chromium-executable> <url-with-#key> <out.json>
//
// out.json: { state, command, appLink, alerts[], title }. `state` is "offer" when the page shows the
// install command, else "no-offer" (expired / unknown / missing key / unavailable pages show none).
// The page is loaded from loopback only; any request to another origin is aborted and counted, so
// the page can't reach production (or send the fragment anywhere) without the gate seeing it.

import { writeFileSync } from "node:fs";
import { chromium } from "playwright-core";

const [exe, url, out] = process.argv.slice(2);
if (!exe || !url || !out) {
  console.error("usage: page-check.mjs <chromium> <url> <out.json>");
  process.exit(2);
}
const origin = new URL(url).origin;
const browser = await chromium.launch({
  executablePath: exe,
  headless: true,
  args: ["--no-sandbox", "--disable-gpu", "--no-first-run"],
});
const result = { state: "no-offer", command: null, appLink: null, alerts: [], title: null, offOrigin: [] };
try {
  const ctx = await browser.newContext({
    userAgent:
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36",
  });
  await ctx.route("**/*", (route) => {
    const u = route.request().url();
    if (u.startsWith(origin) || u.startsWith("data:") || u.startsWith("blob:")) return route.continue();
    result.offOrigin.push(new URL(u).origin);
    return route.abort();
  });
  const page = await ctx.newPage();
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 120_000 });
  // Wait until the client has read the fragment: either the command appears, or a final state
  // (an alert, or a panel that is no longer "Reading your invite link…").
  await page
    .waitForFunction(
      () =>
        document.querySelector(".join-command pre") ||
        document.querySelector('[role="alert"]') ||
        (document.querySelector("main") && !document.querySelector('[aria-busy="true"]')),
      undefined,
      { timeout: 120_000 },
    )
    .catch(() => {});
  // Give hydration a moment to replace the server render if it hasn't yet.
  await page.waitForTimeout(1500);
  result.title = await page.title();
  const cmd = await page.locator(".join-command pre").first();
  if ((await cmd.count()) > 0) {
    result.command = (await cmd.innerText()).trim();
    result.state = "offer";
  }
  const app = page.locator('a[href^="citrate://join/"]').first();
  if ((await app.count()) > 0) result.appLink = await app.getAttribute("href");
  result.alerts = (await page.locator('[role="alert"]').allInnerTexts()).map((s) => s.trim());
} finally {
  await browser.close();
}
result.offOrigin = [...new Set(result.offOrigin)];
writeFileSync(out, JSON.stringify(result, null, 2));
