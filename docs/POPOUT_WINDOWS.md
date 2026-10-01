---
created: 2026-10-01T09:00:00Z
branch: hup/n3-popouts-monitor
author: Larry Klosowski + Claude Opus 5.5
status: draft
wp: HUP-S5.4, HUP-S7.6
---

# Pop-out windows and the Activity monitor

Pop-outs are separate Tauri windows that show one view of the app (planset D-36). The framework
landed with HUP-S5.4; the first pop-out is the Activity monitor (HUP-S7.6, US-7.4). The Browser,
Contract reader, Code and diff, and Media player pop-outs are on the allowlist but have no view
yet, so the app refuses to open them and says "not built yet".

## Rules the framework keeps

| Rule | Where it is enforced |
|---|---|
| Closed allowlist: `browser`, `contract`, `monitor`, `diff`, `media`, labels `popout-<kind>` | `src-tauri/src/popout.rs` (`PopoutKind`), `src/popout/kinds.ts`, `capabilities/popout.json`; a Rust test keeps all three in step |
| Only the main window opens a pop-out | `check_open_request` in `popout.rs` |
| One window per kind; opening again focuses it | `open_sync` in `popout.rs` |
| Least privilege: no app commands, no shell, no fs, no opener | `capabilities/popout.json` grants only `core:event:allow-listen`, `allow-unlisten`, `allow-emit-to`; the app commands are allowed for the main window only (below) |
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
resolves the ACL with Tauri's own resolver to prove no pop-out label can call any app command.

## The message bridge

`src/popout/bridge.ts`: Tauri events on one channel (`citrate-popout`), addressed by window label.
Every message has `v: 1` and is validated on receipt; anything malformed is dropped.

| Direction | Message | Meaning |
|---|---|---|
| pop-out to main | `popout.ready {kind}` | send me the current state |
| pop-out to main | `monitor.stop` | stop the running turn |
| main to monitor | `monitor.snapshot {snapshot}` | the monitor's whole view, rebuilt on every change (coalesced to one per 150 ms) |

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

## Not done

- Not run in the packaged app: covered by unit and component tests, the capability and ACL tests,
  and Tauri's ACL resolver in a test. No manual click-through on a real machine yet.
- Token usage, tokens per second, and gateway spend are shown as unknown: no source exists yet.
- US-7.4 also lists the live plan, approvals and verifier results; they need the sidecar event
  stream wired into the monitor (the sidecar emits `verifier` events; the core provider does not
  forward them yet).
- A Stop button in the main chat itself is not part of this work.
