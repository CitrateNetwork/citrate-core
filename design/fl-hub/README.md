---
created: 2026-11-11T09:30:00Z
branch: ux/v60-ux0-fl-hub-directions
author: Larry Klosowski + Claude Opus 5.5 (MAC lane, V60-UX)
status: review (V60 S0-UX, UX-0; awaiting the owner's pick of one direction, gate g0-ux)
planset: citrate-federation .agentile/planset/2026-11-07-v060/03_UX.md (§2 operations, §3.1 FL hub, §5 UX-0)
sprint: citrate-federation .agentile/sprints/active/2026-11-11-V60-S0.md
---

# FL hub: three directions (UX-0)

Static design mocks for the 0.6.0 FL hub (Work › Rounds), built on the Citrate kit and the
explorer's direction B, for the owner to pick one. The pick is recorded in planset 03 §5 and
closes g0-ux; UX-1 then builds the picked direction in `src/` against live sources.

| Direction | Leads with | Register | Screenshot |
|---|---|---|---|
| A · Instrument | the live round as the hero, matches beside it | Instrument (dark) end to end | [screens/direction-a.png](screens/direction-a.png) |
| B · Ledger | explorer direction B: headline, stat receipts with sources, offers as a table; the live round as one dark strip | Charter (paper) with an Instrument strip | [screens/direction-b.png](screens/direction-b.png) |
| C · Lattice | groups and the cluster first; rounds as lanes across the lifecycle; the selected round opens as a dark panel | Charter with an Instrument panel | [screens/direction-c.png](screens/direction-c.png) |

All three draw the same fixture round and offers with the same parts, so the choice is about
layout and register, not content: round ring with lease dots, challenge clock, Belnap counts,
escrow bar, your contribution timeline, matches with "why this match", the join sheet (what is
signed), payout receipts, Advanced (hidden offers with why-not, ids, raw JSON, CSV), the three
empty and error states, and the tokenomics notice. Each figure names its source as in 02 §11.

## What these are not

- **Not app code and not app data (Rule 1).** Every figure is fixture data drawn for layout, and
  each page says so in a banner. Nothing under `design/` is in the Vite build: the only entry is
  `index.html`, and `scripts/design-fl-hub.test.mjs` fails if the entry, the Vite config or any
  module in `src/` references `design/`.
- **Not a second token source (Rule 9).** The pages link `src/styles/foundation.css` and
  `src/styles/tokens.css` and contain no colour literal (the test checks the pages, `hub.css` and
  the index). The kit-to-app map is [../KIT_TOKEN_MAP.md](../KIT_TOKEN_MAP.md).

## Proposed, needs the owner

- A **Work** place in the sidebar, as 03 §3.1 names it ("Work › Rounds"). The 0.5.2 nav has five
  places (Hermes, Groups, Wallet, Node, Store) plus Settings, and group rounds sit under Groups ›
  Rounds today. The mocks draw Work as a sixth place.
- The colour of the current ring segment on paper is the kit's `--citrate-green-dark`, because
  the lime fails 3:1 on paper. On Instrument it is the lime.

## View and regenerate the screenshots

From the citrate-core root, after `npm ci` (fonts load from node_modules):

```sh
python3 -m http.server 8741 --bind 127.0.0.1
# open http://127.0.0.1:8741/design/fl-hub/
```

The screenshots were taken with Playwright's cached headless Chromium (1440 px wide), no app
binary involved:

```sh
chrome-headless-shell --headless --hide-scrollbars --window-size=1440,2200 \
  --screenshot=screens/direction-a.png http://127.0.0.1:8741/design/fl-hub/direction-a.html
```
