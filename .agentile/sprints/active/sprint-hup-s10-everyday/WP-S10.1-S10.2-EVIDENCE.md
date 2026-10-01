---
created: 2026-10-01T00:00:00Z
branch: hup/n4-media-sheets
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S10
wp: HUP-S10.1 (media) + HUP-S10.2 (sheets, calendar, Hermes schedule)
---

# HUP-S10.1 + S10.2 evidence: media, sheets and calendar

Spec: planset `04_FEATURES_BDD.md` US-10.1 (AC1-AC3) and US-10.2 (AC1-AC3),
`05_SPRINTS_AND_WPS.md` S10.1 and S10.2, `02_ARCHITECTURE.md` section 3 (tier table, media
column) and section 5 (office / media tools). Federation sprint issue #287.

Branches (stacked on the folder grants work, HUP-S2.1):
- citrate-core `hup/n4-media-sheets`, base `hup/n4-grants-e2e` (uses its `GrantStore`).
- citrate-agent-runtime `hup/n4-media-sheets`, base `hup/n4-grants-e2e` (uses its session
  grants). Runtime write-up: `.agentile/sprints/active/2026-10-01-HUP-S10.2-rt-sheet-tools.md`.

## Acceptance criteria

| AC | Where | Proof |
|---|---|---|
| US-10.1 AC1 tiered local vs registry/endpoint | `media.rs` `tier_caps`, `image_routes`, `video_routes` | `tier_caps_follow_the_planset_table`, `routes_are_honest_about_what_is_available`, `video_has_no_backend_yet_and_says_so`; panel test "lists every route" |
| US-10.1 AC2 outputs open in the Media pop-out | `popout.rs` (Media available), `src/popout/MediaPlayer.tsx`, `mediaBridge.ts`, `mediaHost.ts` | `hup_s10_1_the_media_player_opens_from_the_main_window_only`; vitest "the host answers ready...", "the player shows the newest image..." |
| US-10.1 AC3 cost is shown | `Route.cost`, `GalleryItem.cost/usage` | `every_route_carries_a_cost_line`, `usage_from_the_provider_is_kept`; panel "shows the cost of the chosen route before generating"; player cost line |
| Generated files go to a granted folder only | `media.rs` `write_targets`, `target_root`, `write_new_file` | `only_live_write_folder_grants_are_targets`, `a_write_never_follows_a_swapped_root_or_overwrites`; panel "without a write grant there is nowhere to save" |
| US-10.2 AC1 read/write xlsx/csv in grants | runtime `agent-office` + sidecar `sheet_read` / `sheet_write` | 22 agent-office tests, 9 `sheets_session_tests` |
| US-10.2 AC2 Google Sheets/Calendar via Connections when linked | `connections.rs` (`gsheets`, `gcal`), `google_workspace.rs`, Settings rows | 14 `google_workspace` tests; schedule panel "shows Google events only when connected", "says Google is not set up when there is no client id" |
| US-10.2 AC3 Hermes's own schedule visible as a calendar | `hermes_schedule.rs`, `src/agent/schedule/SchedulePanel.tsx` (Journal > Schedule) | 12 `hermes_schedule` tests; panel "lays the week out by day", "adds, pauses and removes" |

## What shipped (core)

- **Media** (`src-tauri/src/media.rs`, new). Routes: `local` (a member-configured loopback
  OpenAI-images server; only on T1/T2) and `remote` (one of the member's AI providers; the key
  stays sealed with its base URL, core posts to the stored base URL plus the fixed
  `/images/generations` path through the new `AiManager::post_to_provider`). Video: no backend in
  this build, said per tier. Replies must be base64 PNG/JPEG/WebP; a link is refused (no URL from
  a provider is ever fetched). Output goes only into a live write folder grant, re-checked on disk,
  created new without following a symlink. Gallery in `<app data>/media/gallery.json`.
  Commands: `media_options`, `media_set_settings`, `media_generate_image`, `media_gallery`,
  `media_read`, `media_save_copy`.
- **Media player pop-out**: `PopoutKind::Media` is now available; it renders `MediaPlayer` over its
  own event channel (`citrate-media`), still with no app commands (capability unchanged). The main
  window answers it from `mediaHost.ts`. Files > Media hosts the generator panel.
- **Google Sheets + Calendar** (`google_workspace.rs`, new; `connections.rs` gains `gsheets` and
  `gcal` on the same Google client as Drive). Sheets: read a range; append rows with
  `valueInputOption=RAW`. Calendar: list the primary calendar in a window (max 92 days), create a
  timed event. Settings > Connections lists both and keeps Connect disabled with the reason until
  a Google client id is configured; the Connections surface points there.
- **Hermes schedule** (`hermes_schedule.rs`, new): versioned owner-only store in
  `<app data>/agent/hermes-schedule.json`, one-off/daily/weekly entries, pause, remove, fail-closed
  on a corrupted file (nothing due, reset sets it aside). `hermes_schedule_due(after, upto)` is the
  read-only query for the scheduler lane (HUP-S10.3): each start is returned once across
  contiguous checks, catch-up is capped. Journal > Schedule shows the week with Google events
  beside Hermes entries when connected.
- All new commands are async (`off_main`), registered, and in the main-window ACL only.

## Defaults pending owner sign-off

Media: 4,000-character prompts, 25 MiB images, 300 gallery items, sizes 512 to 1536, no video
backend chosen, no per-image price table (cost lines name who bills and show reported usage).
Sheets (runtime): 10 MiB, 64 MiB inflated, 5,000 rows, 200 columns, 8,192 characters.
Google: 500 rows per append, 2,000 rows per read, 250 events per listing, 92-day window.
Schedule: 200 entries, 1,000 occurrences per listing or due query, 92-day listing window.

## Tests

- core cargo (new): hermes_schedule 12, google_workspace 14, media 13, popout +1.
- vitest 1019 -> 1041 (media 13, schedule 9). tsc clean. Clippy (`-D warnings`) clean.
- runtime workspace 1233 -> 1264.
- Mutation checks, each killed by a test: schedule due lower bound, enabled filter, until;
  media swapped-root check, write-access filter, folder-kind filter, create-new, T1 caps, link
  refusal, read-back sniff; Google RAW append, cancelled filter, not-connected short-circuit.
  Equivalent (survived, redundant with another guard): schedule overlap `>`/`>=` (the window
  lower bound already excludes it), per-entry occurrence cap (the result is capped again), entry
  cap in `add` (save validation refuses too).

## Not done

- No live end-to-end run: no local image server, no Google OAuth client and no image-capable
  provider key on this machine. The Citrate gateway serves chat only today, so "remote" works
  with OpenAI or another OpenAI-images endpoint, and the gateway answers with an honest refusal.
- No Google refresh-token flow: an expired token asks the member to reconnect.
- Hermes chat tools for the schedule and Google Calendar are not added (the sidecar sheet tools
  are); the scheduler that acts on due entries is HUP-S10.3.
- Sheet writes are not covered by the undo checkpoints yet (S2.9 follow-up).
