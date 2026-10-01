---
created: 2026-10-01T14:00:00Z
branch: hup/n4-browser
author: Larry Klosowski + Claude Opus 5.5
status: implemented behind CITRATE_HERMES_BROWSER (default off); managed Chromium install is HUP-S5.5
wp: HUP-S5.1, HUP-S5.6
---

# Hermes's browser: the Browser pop-out and attach-to-Chrome

US-5.1 "Watch it browse" (planset `2026-09-30-hermes-upskill`, epic E5). The browser runs in the
Hermes sidecar (citrate-agent-runtime crate `citrate-agent-browser`; its README describes the
worker, the tools and the sidecar routes). This page covers citrate-core's side.

## Off by default

The sidecar offers the browser only when it starts with `CITRATE_HERMES_BROWSER=1` (the sidecar
inherits citrate-core's environment). Unset, the sidecar answers `enabled: false`, the main
window's browser controls render nothing, and nothing about a member's app changes. The managed
Chromium path is `CITRATE_BROWSER_CHROMIUM`; without it the sidecar uses a Chromium-family browser
already installed, and says "not installed" when there is none.

## What the member sees

- **Browser controls** (`src/components/BrowserControls.tsx`, in the Agent pane, only while the
  browser is on): the mode, Stop or Resume, Watch (opens the pop-out), the action that is waiting
  for a decision (Allow once / Deny), an origin waiting for consent, attach and detach.
- **The Browser pop-out** (`src/popout/BrowserPopout.tsx`): the screencast with the element
  Hermes is about to act on outlined (amber while it waits for the member, green once done), the
  page address, the mode, and an always-visible Stop. It holds no app commands: decisions and
  consent are taken in the main window only, and its Stop goes over the bridge.

## The safety model

| Rule | Where |
|---|---|
| Every page is untrusted; its text reaches the model fenced as data and taints the session | sidecar (`tools.rs`), loop taint downgrade (HUP-S2.7) |
| After taint, opening an address, clicking or typing waits for the member's explicit decision; no decision within 120 s, Stop, or a session stop means no action | sidecar (`approvals.rs`), main-window controls |
| Stop closes the browser at once, denies any waiting action, and latches until Resume; the global e-stop also stops the browser | sidecar (`service.rs`, `/stop`) |
| Attach needs the member's consent for this session and an unprivileged loopback port | `browser.rs` here, then the sidecar again |
| In attach mode every origin needs consent; banking, email and health origins are excluded by default and need a separate include tick for that one site | sidecar (`scope.rs`, `data/sensitive-origins.toml`), main-window controls |
| Frames of an origin without consent are withheld | sidecar |
| Hermes drives only the tab it opened; detach closes that tab, forgets every consent and never closes the member's browser | sidecar |
| Only http(s) pages; `file:`, `chrome:`, `javascript:`, `data:` are refused | sidecar, and origin checks here |
| No key, no signature: nothing here signs | n/a |

## Commands (main window only)

`hermes_browser_status`, `hermes_browser_frame`, `hermes_browser_stop`, `hermes_browser_resume`,
`hermes_browser_attach`, `hermes_browser_detach`, `hermes_browser_origin`, `hermes_browser_decide`
(`src-tauri/src/browser.rs`). Each runs off the main thread and is listed in
`permissions/main-window.toml`; pop-outs cannot call any of them.

## Data sources (Rule 7)

| Field | Source |
|---|---|
| Status, mode, consent, waiting action | sidecar `GET /browser/status` |
| Screencast frame, element outline | sidecar `GET /browser/frame?after=N` (CDP `Page.screencastFrame` in the sidecar) |
| Chromium found or not | sidecar, from the configured managed path and the usual install locations |

## Pending owner sign-off (shipped with conservative defaults)

- The sensitive-origins list (`data/sensitive-origins.toml` in the runtime crate).
- The 120 s wait for a decision before an action is denied.
- One decision per action after taint (no per-origin budget); a HIC-2 style budget for low-risk
  browsing is the owner's call.
- 9222 as the suggested attach port.

## Not done here

- Downloading and updating the managed Chromium (HUP-S5.5 component updater).
- `console`, `network` and `siwe_sign` browser tools (SIWE is HUP-S2.3).
- The `decide()` element picker (HUP-S5.3) and private search (HUP-S5.2).
- Turning the browser on from Settings: today it is an environment switch for the sidecar.
