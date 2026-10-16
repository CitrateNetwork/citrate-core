---
created: 2026-10-01
branch: hup/n4-search-decide (updated on hup/n7-components-unpack-searxng, 2026-10-04)
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# Hermes web search, page reading and decide() (HUP-S5.2, S5.3)

The tools run in the Hermes sidecar (citrate-agent-runtime, branch `hup/n4-search-decide`):
`agent-search` (`web_search`, `read_url`, the SearXNG supervisor), `agent-loop::decide` (the
System-1 slot) and the sidecar routes `POST /decide`, `GET /decide/stats`, `POST /decide/outcomes`,
`GET /search/status`. The runtime docs are `agent-search/README.md` and `agent-loop/DECIDE.md`.
This page covers what citrate-core owns: the member's choices and how they reach the sidecar.

## Settings › Web search & decisions

| Choice | Default | Effect when on | Leaves the machine |
|---|---|---|---|
| Let Hermes search the web and read pages | off | sessions get `web_search` and `read_url` (untrusted output: the session is tainted after a call) | the page fetch; SearXNG forwards queries to its engines |
| SearXNG program | none | the supervisor starts it on loopback on first search | (as above) |
| How pages are read | on this machine | Jina Reader: the URL goes to r.jina.ai | with Jina, every URL read |
| Use TypeSafe Jev on listed sites | off | `decide()` may use Jev for those https origins, never with a session cookie or in attach mode | the question, options and page snapshot for those sites |
| Also for non-website choices | off | Jev may answer routing and ranking choices too | the question and options |

Core stores the choices in `<app data>/hermes/web-settings.json` (`hermes_web.rs`,
commands `hermes_web_settings_get` / `hermes_web_settings_set`, main window only) and passes them as
`CITRATE_HERMES_*` variables when it next starts Hermes. With the defaults nothing is passed and
the sidecar is unchanged. Only the variables on `SIDECAR_ENV_KEYS` can pass, so these settings can
never override the control bind, the bearer file or the capsule folder.

Keys are files the member chooses (core passes the path and never reads or returns the key).
Keeping them in the custody vault is pending owner sign-off.

## Pending owner sign-off

Conservative placeholders; the defaults change nothing for members:

- Key custody (above): member-chosen key files today, the custody vault later.
- The Jev endpoint and model come from the `system1-agents` adapter; confirm the vendor and its
  terms before any member opts in.
- The decision metering log `<app data>/hermes/decisions.jsonl` (on only with search or Jev, no
  content): retention and rotation.
- The `read_url` and SearXNG limits listed in `agent-search/README.md`.

## Not done here

- SearXNG is not installed for members yet: `scripts/pack-searxng.sh` builds the component
  (macOS arm64), and it installs once the component key exists and a signed manifest is
  published (HUP-S5.5, `docs/COMPONENT_UPDATER.md`). When it is installed and search is on, core
  passes its `bin/searxng-run` (a SearXNG path the member set wins). Until then search reports
  "not installed" unless the member points to an installed `searxng-run`.
- US-5.2 AC2: SearXNG loads only a named list of engines (`agent-search/README.md` in the
  runtime: five general web engines by default, pending owner sign-off), never its own default
  list, and only after the member turns web search on.
- The managed browser (HUP-S5.1) does not call `decide()` yet, and the activity monitor does not
  render the decide metering yet.
- The Jev adapter has been exercised only against a loopback stand-in; no TypeSafe key is held.
