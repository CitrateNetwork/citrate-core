---
created: 2026-10-01T09:00:00Z
branch: hup/n3-popouts-monitor
author: Larry Klosowski + Claude Opus 5.5
status: draft
wp: HUP-S5.4, HUP-S7.6
---

# Pop-out windows and the Activity monitor

Pop-outs are separate Tauri windows that show one view of the app (planset D-36). The framework
landed with HUP-S5.4; the first pop-out is the Activity monitor (HUP-S7.6, US-7.4). The Browser
pop-out followed with HUP-S5.1 (see [HERMES_BROWSER.md](HERMES_BROWSER.md)). The Contract reader,
Code and diff, and Media player pop-outs are on the allowlist but have no view yet, so the app
refuses to open them and says "not built yet".

## Rules the framework keeps

| Rule | Where it is enforced |
|---|---|
| Closed allowlist: `browser`, `contract`, `monitor`, `diff`, `media`, labels `popout-<kind>` | `src-tauri/src/popout.rs` (`PopoutKind`), `src/popout/kinds.ts`, `capabilities/popout.json`; a Rust test keeps all three in step |
| Only the main window opens a pop-out | `check_open_request` in `popout.rs` |
| One window per kind; opening again focuses it | `open_sync` in `popout.rs` |
| Least privilege: no app commands, no shell, no fs, no opener | `capabilities/popout.json` grants only `core:event:allow-listen`, `allow-unlisten`, `allow-emit-to`; the app commands are allowed for the main window only (below). One exception, below: the Contract reader's relay |
| A request is run only if its sender is the window that may send it | Tauri events carry no sender and every pop-out may emit to the main window, so the Contract reader's requests go through Rust instead: `popout_contract_send` (granted to `popout-contract` only by `capabilities/popout-contract.json`, and checked against the caller's label in `popout_contract.rs`) queues them, and the main window drains the queue with `popout_contract_take`. A request sent over the event bus is ignored, and the explanation prompt is built in the main window from the function's ABI entry |
| Only the app's own pages load in a pop-out | `navigation_allowed` (`tauri://localhost`, `http(s)://tauri.localhost`, the Vite dev server in debug builds) |
| Size and position persist per kind | `popouts.json` in the app config dir; a saved spot that is no longer on any screen is dropped and the window centres |
| Closing never kills work | a pop-out's close only saves its geometry; when the main window closes, the pop-outs close with it, so quitting behaves as before |
| A pop-out never builds the app store | `src/main.tsx` renders `PopoutRoot` for a pop-out label and never imports the store (no polling, no writes to saved state) |

## The app-command allowlist (read this before adding a Tauri command)

Tauri allows every app command to every local window until the app defines its own command ACL.
This branch defines it: `src-tauri/permissions/main-window.toml` lists every command registered in
`lib.rs`, and only `capabilities/default.json` (the main window) holds that permission. The main
window keeps exactly the commands it had.

**When you add a command, add it to `generate_handler!` in `lib.rs` and to
`permissions/main-window.toml`.** `popout_tests.rs` fails if the two lists differ, and it also
resolves the ACL with Tauri's own resolver to prove no pop-out label can call any app command,
except `popout_contract_send` from `popout-contract` (`permissions/contract-reader.toml`).

## The message bridge

`src/popout/bridge.ts`: Tauri events on one channel (`citrate-popout`), addressed by window label.
Every message has `v: 1` and is validated on receipt; anything malformed is dropped.

| Direction | Message | Meaning |
|---|---|---|
| pop-out to main | `popout.ready {kind}` | send me the current state |
| pop-out to main | `monitor.stop` | stop the running turn |
| pop-out to main | `browser.stop` | stop Hermes's browser (HUP-S5.1); the Browser pop-out re-sends `popout.ready` every 3 s as a heartbeat |
| main to monitor | `monitor.snapshot {snapshot}` | the monitor's whole view, rebuilt on every change (coalesced to one per 150 ms) |
| pop-out to main | `daemon.pause {id, paused}` | HUP-S10.3: pause (stopping its run) or resume one daemon; `id` must be `d` + 16 hex |
| pop-out to main | `daemon.stop` | HUP-S10.3: stop the daemon run in flight (the daemon stays scheduled) |
| pop-out to main | `monitor.undo.request {session, seq}` | undo one agent file change (`seq`), or the whole session (`seq: null`) (HUP-S2.9) |
| main to monitor | `monitor.undo {panel}` | the agent session's recent file changes, sent with each snapshot (HUP-S2.9) |
| main to browser | `browser.view {view}` | the browser status and latest screencast frame, re-checked on receipt (`src/popout/browserView.ts`) |

## Activity monitor: data sources (Rule 7)

| Field | Source | When it is not known |
|---|---|---|
| Model | the ModelRouter's active choice (same as the chat header chip) | n/a |
| Provider | the provider the turn started on (`ChatProvider.kind`/`label`) | n/a |
| Tier | the hardware tier report (`tier_recommend`, a local probe) | "unknown" until the probe answers |
| Context window | `popout_monitor_facts`: the local llama-server's `--ctx-size` (`serve::DEFAULT_CTX_SIZE`) | "unknown" for the gateway and the demo agent |
| Tokens used | none: no provider reports usage to the app yet | always "unknown", with that reason |
| Why am I waiting | the turn's phase from the real send path (`store.sendChat` status callbacks) | n/a |
| Step | the sidecar's `step_start` events; the in-app loop's model rounds | "unknown" for providers without steps |
| Tool calls | `store.sendChat`'s tool callback, start and end | n/a |
| Elapsed | the turn's real start time | n/a |
| Spend | 0 for the local model and the demo agent | "unknown" for the gateway: not metered in the app yet |
| Daemons (HUP-S10.3) | `daemons_list` (Rust `daemons.rs`: status, today's runs and estimated tokens against the budget, next run, last outcome) and the runner's state (why runs are held) | "No daemons" when there are none; see `docs/WIDGETS_AND_DAEMONS.md` |
| File changes (HUP-S2.9) | the sidecar's checkpoint store, `hermes_checkpoints` (`GET /checkpoints/:session`), re-read when the monitor opens, after each agent file change and after each undo | "not enabled" (with the sidecar's reason) when the sidecar has no store or the file tools are off |

## Stop

The Stop button sends `monitor.stop`; the main window runs `store.stopAgentTurn()`:

- the chat returns to ready at once and the reply is marked "stopped by you" (with Retry);
- the in-app loop stops before its next model request or tool call (an in-flight model request
  cannot be cancelled; its late answer is discarded);
- the sidecar loop calls the session's stop route (`hermes_session_stop`), drains its events to
  `done`, and runs no tool call after Stop; the next turn starts only after that drain, in a fresh
  sidecar session (a session's stop switch stays on, so a stopped session is not reused; the new
  session does not carry the earlier conversation);
- a tool call the stopped turn asks for afterwards is not run;
- an approval card already open stays open for the member to decide.

## Undo for agent file changes (HUP-S2.9)

The sidecar's file tools (`fs_write`, `fs_edit`, `fs_delete`, `fs_rename`) change files only inside
folders the member granted for writing, after the default-deny list, and take an undo checkpoint
around every change. Each change shows as a card under the agent reply that made it, with Undo; the
monitor lists the session's recent steps with Undo for each and Undo all. The pop-out only asks
(`monitor.undo.request`); the main window runs `hermes_undo_step` or `hermes_undo_session`.

An undo is refused, with nothing restored, when a file changed after the agent's edit; the card and
the monitor show the sidecar's reason. Undo is a member action, never an agent tool.

Off by default: core passes the checkpoint store (`CITRATE_HERMES_CHECKPOINTS`, under the app's
local data) but neither the file-tools switch nor a grants file, so the agent makes no file changes
until a Grants screen exists and turning agent writes on is signed off. The runtime side is
documented in `agent-sidecar/src/files.rs` (citrate-agent-runtime).

## Not done

- Not run in the packaged app: covered by unit and component tests, the capability and ACL tests,
  and Tauri's ACL resolver in a test. No manual click-through on a real machine yet.
- Token usage, tokens per second, and gateway spend are shown as unknown: no source exists yet.
- US-7.4 also lists the live plan, approvals and verifier results; they need the sidecar event
  stream wired into the monitor (the sidecar emits `verifier` events; the core provider does not
  forward them yet).
- A Stop button in the main chat itself is not part of this work.
- Undo (HUP-S2.9) has not been clicked through in the packaged app: the agent file tools are off by
  default, so no member session produces a change yet. Covered by sidecar tests on a real
  filesystem and by core unit and component tests.

## Accessibility

Checks and results for the pop-out framework and the Activity monitor (landmarks, window title,
focus order, live regions, contrast, reduced motion) are recorded in
[A11Y_AUDIT_HUP_S10_6_2026-10-01.md](A11Y_AUDIT_HUP_S10_6_2026-10-01.md).
