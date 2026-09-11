# Citrate Core — docs screenshot harness

Reproducible screenshots of the Citrate Core onboarding flow and every app
surface, for the documentation site. Runs the React frontend in **sim mode**
(`vite dev`) driven by Playwright.

## Why this is safe to run anywhere

The app detects its host: in a packaged build it runs in **`tauri`** mode against
a real node + keyring; in a plain browser it runs in **`sim`** mode, where
`src/bridge/sim/*` fabricates preview data for every surface. This harness only
ever loads the browser build, so:

- **no node process starts** (nothing to crash — this is why it is safe here even
  though launching the real node on this machine has crashed before),
- no keyring, no signing, no network writes,
- output is deterministic and regenerates on every UI change.

It captures the same screens on Linux, macOS, or in CI.

## Run (out-of-box)

From a fresh clone, two commands — no separate browser install, no separate root
`npm install`:

```sh
cd screenshots
npm install                # also installs Playwright's Chromium (postinstall)
npm run capture            # headless; installs the browser if missing (precapture),
                           # installs the parent app's deps if missing, starts Vite
npm run capture:headed     # watch it drive the app in a real window
```

Node 20+ is the only prerequisite. `npm run capture` is idempotent — a warm tree
skips the browser + parent-deps installs and goes straight to Vite.

PNGs land in `screenshots/out/` (gitignored):

- `onboarding-s0…s6-*.png` — the 7 onboarding stages (welcome → sign in →
  verify identity → membership → wallet → grant+stake → node ignition).
- `app-00-shell.png` + `app-<route>.png` — the shell and all 17 surfaces
  (dashboard, wallet, node, models, storage, files, journal, groups, comms,
  cluster, train, agent, connections, community, commissary, settings, alf).
- `app-agent-<tab>.png` — the Hermes agent's tabbed panels (overview · contracts),
  which a bare surface shot doesn't reach (the tab is component-local state).

Shots are 1440×900 at 2× (retina-crisp) in the dark theme.

## How it works

- `playwright.config.ts` boots `npm run dev` (Vite on :1420) as its web server.
- `capture.spec.ts` drives two seams exposed only in dev:
  - `window.__citrateStore` (added in `src/main.tsx` behind `import.meta.env.DEV`)
    — sets the onboarding `stage` and completes onboarding to reach the shell.
  - the hash router (`#/dashboard`, `#/node`, …) — navigates between surfaces.
- The sim-only "Prototype" affordance (`.sim-proto-affordance`) is hidden so the
  captures look like the packaged app.

Both dev seams compile out of production/Tauri builds, so nothing here ships.

## Adding or reordering screens

Edit the `ONBOARDING` and `SURFACES` arrays at the top of `capture.spec.ts`. The
route list must stay in sync with the hash allowlist in `src/App.tsx`.

## Capturing true native macOS window chrome (optional)

This harness captures the app *content*, not the native window frame. If you want
a few hero shots with the real macOS title bar, run the packaged app on a Mac
(`npm run tauri dev` with the sidecar overrides — see the repo's launch docs) and
screenshot the window directly. The content is identical to what this harness
produces.
