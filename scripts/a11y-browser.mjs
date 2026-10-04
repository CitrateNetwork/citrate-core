#!/usr/bin/env node
// =====================================================================
// HUP-S10.6 follow-up — run axe-core in a REAL browser (Chromium) over the pop-outs and the
// approval surfaces, with the app's real stylesheet, so the colour-contrast rule judges real
// pixels (jsdom cannot; see src/a11y/axeHarness.ts).
//
//   node scripts/a11y-browser.mjs [--json out.json] [--scene <id>]...
//
// Needs, on this machine only (nothing is added to package.json):
//   - playwright-core, found through PLAYWRIGHT_CORE (a path to its folder) or the normal module
//     resolution (for example `npm i playwright-core` in a scratch folder, then set PLAYWRIGHT_CORE)
//   - a Chromium build: CHROMIUM_PATH, or the newest "Chrome for Testing" under
//     ~/Library/Caches/ms-playwright (what `npx playwright install chromium` downloads)
// It builds scripts/a11y-browser/scenes.tsx with the repo's Vite into a temporary folder, serves
// it on a loopback port, opens each scene in both its register and at 200% zoom, runs axe with
// the same tags as the jsdom tests (WCAG 2.0/2.1/2.2 A + AA and best-practice) PLUS
// color-contrast, prints one line per finding, and exits 1 when anything is found.
// =====================================================================
import { createServer } from "node:http";
import { mkdtempSync, readFileSync, readdirSync, rmSync, existsSync, writeFileSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { join, resolve, extname, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const require = createRequire(join(repo, "package.json"));

const args = process.argv.slice(2);
const jsonOut = args.includes("--json") ? args[args.indexOf("--json") + 1] : null;
const only = args.flatMap((a, i) => (args[i - 1] === "--scene" ? [a] : []));

async function loadPlaywright() {
  const p = process.env.PLAYWRIGHT_CORE;
  try {
    return p ? await import(pathToFileURL(join(p, "index.mjs")).href) : await import("playwright-core");
  } catch (e) {
    console.error("playwright-core is not installed. Install it in a scratch folder and set PLAYWRIGHT_CORE to its path:");
    console.error("  npm i --prefix /tmp/pw playwright-core && PLAYWRIGHT_CORE=/tmp/pw/node_modules/playwright-core node scripts/a11y-browser.mjs");
    console.error(String(e));
    process.exit(2);
  }
}

function findChromium() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  const cache = join(homedir(), "Library", "Caches", "ms-playwright");
  if (!existsSync(cache)) return null;
  const dirs = readdirSync(cache)
    .filter((d) => /^chromium-\d+$/.test(d))
    .sort((a, b) => Number(b.split("-")[1]) - Number(a.split("-")[1]));
  for (const d of dirs) {
    for (const arch of ["chrome-mac-arm64", "chrome-mac"]) {
      const exe = join(cache, d, arch, "Google Chrome for Testing.app", "Contents", "MacOS", "Google Chrome for Testing");
      if (existsSync(exe)) return exe;
    }
    const linux = join(cache, d, "chrome-linux", "chrome");
    if (existsSync(linux)) return linux;
  }
  return null;
}

async function buildScenes(out) {
  const { build } = await import(pathToFileURL(require.resolve("vite")).href);
  const react = (await import(pathToFileURL(require.resolve("@vitejs/plugin-react")).href)).default;
  await build({
    configFile: false,
    root: join(repo, "scripts", "a11y-browser"),
    base: "./",
    logLevel: "warn",
    plugins: [react()],
    define: { __BUILD_SHA__: '"a11y"', __BUILD_TIME__: '"a11y"', __APP_VERSION__: '"a11y"' },
    build: { outDir: out, emptyOutDir: true, minify: false },
  });
}

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png" };

function serve(dir) {
  return new Promise((ok) => {
    const srv = createServer((req, res) => {
      const path = decodeURIComponent((req.url ?? "/").split("?")[0].split("#")[0]);
      const file = resolve(dir, "." + (path === "/" ? "/index.html" : path));
      if (!file.startsWith(dir) || !existsSync(file)) {
        res.writeHead(404).end();
        return;
      }
      res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" }).end(readFileSync(file));
    });
    srv.listen(0, "127.0.0.1", () => ok(srv));
  });
}

const AXE_OPTIONS = {
  runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] },
  // The same two page-level rules the jsdom harness turns off: each scene is one surface of the
  // app, not a whole page. color-contrast stays ON here (that is the point of this pass).
  rules: { "page-has-heading-one": { enabled: false }, region: { enabled: false } },
};

async function main() {
  const { chromium } = await loadPlaywright();
  const exe = findChromium();
  if (!exe) {
    console.error("No Chromium found. Set CHROMIUM_PATH, or run `npx playwright install chromium`.");
    process.exit(2);
  }
  const out = mkdtempSync(join(tmpdir(), "a11y-scenes-"));
  await buildScenes(out);
  const srv = await serve(out);
  const port = srv.address().port;
  const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
  const axeVersion = JSON.parse(readFileSync(require.resolve("axe-core/package.json"), "utf8")).version;
  const browser = await chromium.launch({ executablePath: exe, headless: true });
  const results = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1024, height: 900 } });
    await page.goto(`http://127.0.0.1:${port}/index.html#none`);
    await page.waitForSelector("body[data-scene-ready]");
    const ids = (await page.evaluate(() => window.__sceneIds)).filter((s) => only.length === 0 || only.includes(s));
    for (const zoom of [1, 2]) {
      for (const id of ids) {
        const p = await browser.newPage({ viewport: { width: Math.round(1024 / zoom), height: Math.round(900 / zoom) }, deviceScaleFactor: zoom });
        await p.goto(`http://127.0.0.1:${port}/index.html#${encodeURIComponent(id)}`);
        await p.waitForSelector(`body[data-scene-ready="${id}"]`);
        // Let entrance animations (the dialogs' fade-up) finish: axe must judge the settled colours,
        // not a frame half-way through a fade.
        await p.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))));
        await p.addScriptTag({ content: axeSource });
        const r = await p.evaluate(async (opts) => {
          const res = await window.axe.run(document, opts);
          return {
            violations: res.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.map((n) => ({ target: n.target.join(" "), summary: n.failureSummary })) })),
            passes: res.passes.length,
            incomplete: res.incomplete.map((v) => ({ id: v.id, nodes: v.nodes.length })),
          };
        }, AXE_OPTIONS);
        results.push({ scene: id, zoom, ...r });
        await p.close();
      }
    }
  } finally {
    await browser.close();
    srv.close();
    rmSync(out, { recursive: true, force: true });
  }
  const version = await (async () => {
    const b = await chromium.launch({ executablePath: exe, headless: true });
    const v = b.version();
    await b.close();
    return v;
  })();
  let findings = 0;
  for (const r of results) {
    const tag = `${r.scene} @${r.zoom * 100}%`;
    if (r.violations.length === 0) console.log(`ok    ${tag}  (${r.passes} rules passed, ${r.incomplete.length} needs review)`);
    for (const v of r.violations) {
      findings += v.nodes.length;
      for (const n of v.nodes) console.log(`FAIL  ${tag}  ${v.id} (${v.impact})  ${n.target}  ${n.summary?.split("\n").slice(1).join(" ").trim() ?? ""}`);
    }
  }
  console.log(`\nChromium ${version}, axe-core ${axeVersion}: ${results.length} scene runs, ${findings} finding(s).`);
  if (jsonOut) writeFileSync(jsonOut, JSON.stringify({ chromium: version, axe: axeVersion, results }, null, 2));
  process.exit(findings ? 1 : 0);
}

main().catch((e) => {
  console.error(e);
  process.exit(2);
});
