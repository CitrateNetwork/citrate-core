---
created: 2026-10-01T12:00:00Z
branch: hup/n4-widgets-daemons
author: Larry Klosowski + Claude Opus 5.5
status: draft
wp: HUP-S10.3
---

# Widgets and daemons

HUP-S10.3 (planset `2026-09-30-hermes-upskill`, US-10.3; architecture §1 "Daemon" and "Widget",
§4 HIC-3, §10 D-35). Both live on the Hermes home (the `dashboard` route), in the right rail.

- **Widgets** are small tiles that Hermes (or the member, or the gallery) writes as HTML and JS.
  Each one runs in a sandbox and can only read the data it declared, from a short read-only list.
- **Daemons** are recurring Hermes tasks on a local schedule. Each has a budget, never spends,
  shows up in the Activity monitor and can be paused. Anything a daemon wants to change waits for
  the member's explicit approval.

## Widgets

### What a widget can do

| It can | It cannot |
|---|---|
| Run its own inline script and style | Load anything from the network (`connect-src 'none'`, no remote script, style, image or frame) |
| Ask the main window for a declared catalog query with `await citrate.query(name)` | Call any Tauri command, reach `invoke`, or read app storage (opaque origin, no capability applies) |
| Show text and `data:` images | Open windows, submit forms, navigate the app, or frame anything |

### The layers

1. **Its own document.** The main window frames `citrate-widget://localhost/<id>` (Windows:
   `http://citrate-widget.localhost/<id>`). Rust serves it from the widget store
   (`src-tauri/src/widgets.rs`, registered with `register_asynchronous_uri_scheme_protocol` in
   `lib.rs`). Only the `main` webview may load it; a pop-out or any other label gets 403.
2. **A strict CSP header on that document** (`widgets::widget_csp`): `default-src 'none'`,
   `script-src 'unsafe-inline'` (inline only), `connect-src 'none'`, every other fetch directive
   `'none'`, `form-action 'none'`, `base-uri 'none'`, `sandbox allow-scripts` (sandboxed even if
   loaded outside the iframe) and `frame-ancestors` limited to the app's own origins (plus the
   Vite dev server in debug builds).
3. **The app CSP** allows framing only that scheme: `frame-src citrate-widget:
   http://citrate-widget.localhost https://citrate-widget.localhost`. The app's own `script-src`
   is unchanged.
4. **A sandboxed iframe** (`src/widgets/WidgetFrame.tsx`): `sandbox="allow-scripts"` and nothing
   else (no `allow-same-origin`, popups, forms, modals, downloads or top navigation),
   `referrerpolicy="no-referrer"`, `allow=""`.
5. **A narrow bridge** (`src/widgets/host.ts`): one host per frame hears `message` events, keeps
   only those whose `source` is its own frame, drops malformed messages, and answers a
   `widget.query` only when the name is in the catalog AND in the widget's declared list. At most
   30 queries a minute per widget. There is no other message type.

The SDK each document starts with (`widgets::SDK_JS`) defines a frozen, non-writable,
non-configurable `window.citrate` before the widget's own markup, so a widget cannot replace it.

### The query catalog (Rule 7 data sources)

| Query | Returns | Source |
|---|---|---|
| `node.status` | `{height, peers, state, finalityAgeSec}` | `store.snapshot()` (the node's local RPC) |
| `wallet.summary` | `{liquidSalt, stakedSalt, claimableSalt}`, never the address | `store.snapshot()` |
| `model.active` | `{label, id}` | the ModelRouter's active choice (the chat header's source) |
| `daemons.summary` | `{allPaused, total, running, paused, budgetUsedUp}` | the daemons slice (`daemons_list`) |

`src/widgets/catalog.ts` and `widgets::WIDGET_QUERIES` list the same names; a Rust test reads the
TypeScript file to keep them equal. A widget is saved only if every query it declares is in the
catalog.

### Writing a widget

- **Gallery:** four starter templates (`src/widgets/gallery.ts`): block height, SALT at a glance,
  Hermes model, daemons. They read through `citrate.query` and write data as text, never HTML.
- **Hermes:** the `widget_create` tool (effect `write`, trust `trusted`). The member always sees
  the source and the declared queries on an approval card first; a decline saves nothing. A tool
  call with an unknown query is refused before any card.
- Limits: 24 widgets, 64 KB of source each, one JSON file per widget in
  `<app local data>/widgets/`.

## Daemons

### What a daemon is

A name, a task (the prompt Hermes gets), a 5-field cron schedule in local time (`minute hour day
month weekday`, with `*`, `N`, `a-b`, `*/n`, lists, and `@hourly`, `@daily`, `@weekly`) and a
budget. Rust (`src-tauri/src/daemons.rs`) owns every schedule, budget and ledger and keeps them in
`<app local data>/daemons.json`.

### Budgets (defaults are placeholders, pending owner sign-off)

| Limit | Default | Hard ceiling |
|---|---|---|
| Runs per local day | 4 | 48 |
| Tokens per local day | 20,000 | 200,000 |
| Tokens per run | 6,000 | the day's limit |
| Spend | 0 SALT | 0 (a non-zero spend budget is refused) |
| Run time limit (`DAEMON_RUN_TIMEOUT_MS`) | 10 minutes | n/a |

- A due daemon whose day budget is used up is **skipped** (counted, with a note); it never runs
  "just this once".
- A run's tokens are **estimated** (characters / 4): each model round re-reads the system prompt,
  the tool list and the conversation so far. No provider reports usage to the app yet; every
  screen says "estimated".
- The runner stops a run as soon as its estimate passes the run's allowance (`over_budget`), so
  the day's limit can be passed by at most one model round. The run is charged what it used.
- A run the app never reports back is released after 30 minutes and charged its full allowance.
- The budget resets at local midnight.

### Scheduling rules

- Nothing runs until the member creates a daemon.
- One run in flight per daemon. Missed minutes while the app was closed catch up **once**, and
  only minutes in the last 24 hours count.
- A paused daemon (or "Pause all") never fires, and resuming does not replay the minutes it missed.
- Daemons run **only on the local model** (the in-app loop or the Hermes sidecar loop). With any
  other provider the runner claims nothing and says why ("Runs are held: ...").

### HIC rules (planset §4, HIC-3)

- A daemon turn gets every agent tool except `app_navigate` (it never moves the member's screen).
- Read-only tools run as in chat.
- **Every other tool call is HIC-required**: it goes to the member as an explicit decision (an
  approval card or the SignatureCeremony) whose reason names the daemon. There is no budget path
  for these calls; nothing changes and nothing signs without that click.
- On the sidecar loop the session is opened `unattended` (`hermes_session_open_unattended`): the
  sidecar starts the session in its HIC downgrade, so it marks every effectful call
  HIC-required too (or declines it itself if core did not promise to ask). The session is closed
  when the run ends (`hermes_session_close`).

### Where to see them

- **Hermes home, Daemons card:** create, pause or resume, remove, "Pause all"; each row shows the
  status, the schedule and next run, today's runs and estimated tokens against the budget, spend
  (0), skips, the last outcome and note, and (this session) the last full answer.
- **Activity monitor (pop-out):** a Daemons section with the same status and budget line, Pause /
  Resume per daemon, and "Stop the daemon run" while one is running. The pop-out sends
  `daemon.pause {id, paused}` and `daemon.stop` over the validated pop-out bridge; the main window
  does the work (see `docs/POPOUT_WINDOWS.md`).

### Commands

`daemons_list`, `daemon_save`, `daemon_set_paused`, `daemons_set_all_paused`, `daemon_delete`,
`daemons_claim_due`, `daemons_finish_run`; `widgets_list`, `widget_save`, `widget_delete`,
`widget_source`; `hermes_session_open_unattended`, `hermes_session_close`. All are async (off the
main thread) and allowed for the main window only (`permissions/main-window.toml`).

## Proof

- Formal: `src-tauri/formal/DaemonBudget.tla` (see the formal README): runs within the day's
  cap, tokens over the cap by at most one round, every allowance within the day, no empty run,
  spend zero, one run in flight, no start while paused, and nothing a daemon proposes runs without
  approval. TLC and a mutation check of every invariant.
- Tests: `daemons_tests.rs`, `widgets_tests.rs`, `hermes_daemon_tests.rs` (Rust);
  `src/widgets/*.test.ts(x)`, `src/daemons/*.test.ts(x)`, `src/popout/daemonsMonitor.test.tsx`
  (TypeScript); `agent-sidecar/src/daemon_session_tests.rs` (citrate-agent-runtime).

## Not done

- **Not run in the packaged app.** The widget scheme handler, the CSP header and the sandboxed
  iframe are covered by unit and component tests; a click-through in the packaged app (macOS,
  Linux, Windows) has not been done. In particular, that the platform webviews honour the custom
  scheme's CSP header and `frame-ancestors` is asserted by the response we send, not observed.
- Token use is estimated, not measured (no provider reports usage to the app yet).
- Daemon runs are not written to the journal or the metering log; the last answer is kept for
  this session only, the ledger keeps a one-line note.
- Daemons only run while the app is open (the runner lives in the main window).
- Widget queries are not counted in the Activity monitor yet (`WidgetFrame` exposes `onQuery`).
- Time zones: the webview passes its UTC offset with every call; a daylight-saving change can
  shift a run by the hour it skips or repeats.
