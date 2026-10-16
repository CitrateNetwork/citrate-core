---
created: 2026-10-04
branch: hup/n7-everyday-packaged-qa
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S10 (+ HUP-S7.6)
wp: US-10.1 (live image run), US-10.2 (sheet_write undo in Journal > Sheets), US-7.4 AC1 (chat-turn plan), g5-everyday prep, US-10.3 / US-10.4 / US-7.4 packaged QA prep
---

# Fan-out 7, lane L15 (everyday packaged QA): evidence

Base: citrate-core `hup/forward-merge-main-0.4.3` @ 81ef7a0. Branch `hup/n7-everyday-packaged-qa`.
citrate-agent-runtime needed no change: `sheet_write` was already checkpointed and undoable in
the sidecar (`agent-sidecar/src/undo_writes_tests.rs`: `sheet_write_is_checkpointed_and_undoable`).

This record keeps three levels apart: **built** (code and tests on this branch), **run live on this
Mac** (a real backend, no test double), and **not run** (needs a person at the packaged app).

## 1. sheet_write undo from Journal > Sheets (built)

PR #209 listed "sheet_write undo" as not done. What was true on the base: the sidecar checkpoints
every `sheet_write`, and the chat card and the Activity monitor could undo it. The Journal > Sheets
view had no undo: it only covered Google Sheets (`gsheets_read` / `gsheets_append`), and its note says
to remove Google rows in Google Sheets itself.

Change: Journal > Sheets now also lists **Sheets Hermes wrote** in this app session
(`src/agent/sheets/AgentSheetChanges.tsx`), newest first, each with Undo. Rows come from the agent
undo slice, which is filled only from sidecar `tool_result` events that name a checkpoint. Undo uses
the same path as the chat card (`agentUndo.undoChange` to Rust `hermes_undo_step` to the sidecar's
checkpoint store). If the sidecar refuses (the file changed since, or the step was pruned), the row
shows the reason as an alert and Undo stays available. The card now says "Wrote sheet" for
`sheet_write` and "Wrote" for `file_write` (both were "Changed" before).

Tests: `src/agent/sheets/agentSheetChanges.test.tsx` (4: empty state, only sheet writes and newest
first, undo restores and says so, a refusal shows its reason), `src/surfaces/journalSheetsUndo.test.tsx`
(1: opening Journal > Sheets shows a recorded sheet write with Undo, and pressing Undo there calls
the app's `undoStep` for that session and step; the reviewer added that last check).

## 2. Plan events for plain chat turns (built)

PR #209 listed "plan events for chat turns" as not done. The Activity monitor's Plan only showed a
workflow run's plan (the sidecar's `plan` event). In a chat turn it always said there was no plan.

Change: in a chat turn, the plan is now what the model asked for, one row per model step that asked
for tools, for example `Step 1: web_search, fetch_url`. Rows are added only when the provider reports
the model's tool call, so the plan never lists a step the model did not take.

- `src/agent/chatPlan.ts`: builds the rows. Call ids are deduplicated within a step only, because
  the sidecar reuses `call_0` on later steps. A replayed event does not add a row twice. At most 50
  rows, and 6 tool names per row.
- In-app loop (`harness.ts createAgentProvider`): reports the step's plan before its first tool runs.
- Sidecar loop (`sidecarProvider.ts drive`): reports from `tool_call` events in chat turns only. A
  workflow run still reports its own plan, unchanged.
- Turn slice: `planReported(steps, "chat")` records `planSource`. A chat plan never replaces a
  workflow plan in the same turn. `chatPlanStates` marks earlier rows done. The latest row shows
  running while the turn runs, done when answered, and stopped when the turn was stopped or failed.
- The snapshot validator accepts the new row states (`running`, `done`, `stopped`) and still refuses
  unknown ones. The resumed-turn path (`store.ts`) now forwards plan events too.

Tests: `src/agent/chatPlan.test.tsx` (13; the reviewer added the 13th, which runs a real workflow
run with `tool_call` events and checks that no chat rows are added to its plan). Existing tests updated for the new behaviour:
`ActivityMonitor.usage.test.tsx` (the empty-plan wording, plus a malformed-state check that now uses
a state that is still invalid) and `stopTurn.test.ts` (the in-app step test now also checks the plan
event between the two steps).

## 3. Live image generation against a loopback OpenAI-images server (run live on this Mac)

`media_generate_image` was a single Tauri command. Its body is now `media::generate_image`, which the
command calls. The behaviour is the same: pick the route, refuse one that is not available, check the
destination grant before spending anything, ask the backend, check the reply, write a new file into
the granted folder, and record it in the gallery with its cost line. The only differences are that a
corrupt grants file or a gallery that cannot be opened is now reported before an unknown route. Moving the body into a function lets a
test drive the real pipeline without a window.

New tests in `src-tauri/src/media_tests.rs`:
- `generate_writes_into_the_granted_folder_and_records_the_cost_line`
- `generate_checks_the_route_and_the_destination_before_asking_any_backend` (the backend is never
  asked when the grant is read-only or the route is not offered)
- `a_provider_link_instead_of_image_bytes_saves_nothing`
- `live_local_image_generation_into_a_granted_folder` (`#[ignore]`, live)

Live run, 2026-10-04, macOS 15.6.1 (Apple silicon):

| Item | Value |
|---|---|
| Backend | stable-diffusion.cpp `sd-server`, release `master-929-3f8527a` (macOS arm64 build), `--listen-ip 127.0.0.1 --listen-port 18731 --steps 1 --cfg-scale 1.0` |
| Model | `Green-Sky/SD-Turbo-GGUF` `sd_turbo-f16-q8_0.gguf`, sha256 `d50be7655f0a554cf8041c145d88b210bd5f3c545423119dee62ae08cae51580` (matches the Hugging Face LFS oid) |
| Command | `CITRATE_MEDIA_LIVE_URL=http://127.0.0.1:18731/v1 CITRATE_MEDIA_LIVE_MODEL=sd-turbo CITRATE_MEDIA_LIVE_OUT=<folder> cargo test -p citrate-core --lib media::tests::live_ -- --ignored --nocapture` |
| Route | `local`, tier T1, destination `this device (127.0.0.1:18731)` |
| Cost line on the item | `No charge: runs on this device` (the same string the route showed before the request) |
| Output | `citrate-image-20261004-235342-f6a33039.png`, 512 x 512, 570,615 bytes, sha256 `2daf6079b71a6347ac50fcbd3f9a19b8b6b398ea6935691ee7ac8c6aa09fc16d`, written into the write-granted folder (`in_granted_folder: true`) |
| Gallery | 1 item recorded. `data_url_for` read it back as `data:image/png;base64,…` (the Media pop-out displays this data URL) |
| Time | 8.1 s end to end (server: `generate_image completed in 7.85s`, sampling 0.77 s) |
| Prompt | "a ripe lemon on a wooden table, soft daylight, photo" (the image shows a lemon on a wooden table) |

Not run: opening that file in the Media pop-out window of a packaged app. The pop-out shows the
gallery's data URL, and that read-back is what the live test checks. Showing it on screen in
WKWebView is part of the click-through in section 5. The image file is not committed. Its sha256 is
recorded above.

## 4. Gates on this branch

| Gate | Before | After | Command |
|---|---|---|---|
| tsc | clean | clean | `npx tsc --noEmit` |
| vitest | 2,188 passed, 33 skipped (computed: after minus the 17 new tests) | 2,205 passed, 33 skipped (251 files) at the builder's commit; the review adds 1 test and 1 assertion. In the full run, `scripts/stage-knowledge-corpus.test.mjs` timed out once under machine load and passes on its own (23/23); it is not touched here | `npx vitest run` |
| cargo fmt | clean | clean | `cargo fmt --all -- --check` |
| clippy | clean | clean (citrate-core, all targets) | `rustup run 1.98.1 cargo clippy --locked --no-deps -p citrate-core --all-targets -- -D warnings` |
| core lib tests | 1,716 passed, 15 ignored (computed: after minus 3 new passing and 1 new ignored) | 1,719 passed, 0 failed, 16 ignored | `cargo test --locked -p citrate-core --lib` |
| media tests | 16 passed | 19 passed, 1 ignored (live) | `cargo test -p citrate-core --lib media::` |
| release pin tripwire | OK | OK | `scripts/ci/check-release-pins.sh` |

No Tauri command was added or renamed, so the ACL and capability files are unchanged.

## 5. Packaged-app click-through on macOS (not run: needs a person)

Not done by this lane, for two reasons:

1. **Disk gate.** The shared core target holds only a debug build (15 GB). This Mac had 17 to 22 GB
   free during the lane. A packaged build means a release build of the app plus the sidecar, comms
   and cluster daemons and the llama-server bundle, which would take free space below the 12 GB
   floor while other lanes are building.
2. **The member's own data.** A local-run packaged build uses bundle id `ai.citrate.core`, the same
   as the installed `/Applications/Citrate Core.app`, so it opens the owner's real app data, keychain
   entries and wallet. Recovery kit and journal import are not steps an agent should click through
   on the owner's account, and screenshots of the recovery kit would capture the phrase sheet.

What is already proven without a window (tests on this stack):

| Area | Proof on this stack |
|---|---|
| Widget CSP, no network or Tauri reach | `widgets_tests.rs`: `the_document_carries_a_csp_that_forbids_every_network_and_ipc_path`, `the_app_csp_frames_only_the_widget_scheme`, `only_the_main_window_may_load_a_widget_document`; `WidgetFrame.test.tsx` (sandboxed iframe, scripts only, no referrer; bridge answers only its own frame) |
| Daemon on the local model, monitor, pause and stop | `daemons_tests.rs`, `hermes_daemon_tests.rs`, `src/daemons/*.test.ts` (measured tokens, run log), `ActivityMonitor` tests (tokens/s, approvals, Stop first in tab order) |
| Journal export and import | `journal_export_tests.rs` (Argon2id and AES-256-GCM round trip); `encryptedExport.ts` uses the Tauri dialog plugin, which only a packaged app exercises |
| Recovery kit | `recovery_kit_tests.rs` (covers exactly the device-minted keys, never the wallet; phrase sheet round trip; a wrong word writes nothing); `src/privacy/recovery.test.ts` |
| Activity monitor plan, approvals, Stop | `ActivityMonitor.usage.test.tsx`, `ActivityMonitor.undo.test.tsx`, and the chat-plan tests in section 2 |

Click-through script for a person (packaged local-run build of the stack's top branch, signed in to
a QA member account, not the owner's main account). Record pass or fail, the time and a screenshot
for each step, except where a step says not to:

1. **Widget under the WKWebView CSP.** Ask Hermes to create a widget that shows node height. It
   renders in the widget panel. Open Safari Web Inspector on the widget frame and run
   `fetch("https://example.com")` and `window.__TAURI__`. Expected: the fetch is blocked by CSP and
   `__TAURI__` is undefined.
2. **Daemon on the local model.** Create a daemon on the local model (T0 or T1) with a 1-minute
   schedule. Open the Activity monitor. Expected: the daemon is listed with a measured tokens/s value
   after its first run, plus its approvals. Pause it: no run starts during the next 2 minutes. Resume
   it, then Stop it: the run ends and the run log shows the stop.
3. **Activity monitor in a chat turn.** Ask a question that needs two tools ("what is my node height,
   and what did I write in my journal today?"). Expected: Plan shows `Step 1: …` as running, then
   done, with a later row while the turn runs. A gated tool (for example `gsheets_append` or
   `schedule_add`) shows under Approvals as waiting, then decided. Stop is visible the whole time and
   ends the turn, and the latest plan row then reads stopped.
4. **sheet_write undo from Journal > Sheets.** Grant a folder for writing. Ask Hermes to write a CSV
   there. Open Journal > Sheets. Expected: "Sheets Hermes wrote" lists it. Press Undo: the file is
   restored (or removed if it was new). Edit the file by hand and try again: the refusal says it
   changed.
5. **Journal export and import dialogs.** Journal > Export > Encrypted file: the native macOS save
   dialog opens, and the saved `.citrate-journal` is ciphertext. Import it through the native open
   dialog with the passphrase: the entries come back. A wrong passphrase is refused and nothing is
   written.
6. **Recovery kit.** Settings > Privacy > Recovery kit on the QA account only. Do not screenshot the
   phrase sheet. Expected: it covers the device-minted keys and says it does not include the wallet.
   The restore check refuses a sheet with one wrong word.
7. **Media pop-out.** Settings > Media: set the local server to `http://127.0.0.1:<port>/v1` (the
   sd-server from section 3). Generate an image into a granted folder. Expected: the cost line
   "No charge: runs on this device" is shown before you press Generate, the file appears in the
   folder, and the Media pop-out shows it.

Linux and Windows packaged runs are DGX team work (steps on the sprint issue).
