---
created: 2026-11-11T09:00:00Z
branch: ux/v60-ux0-fl-hub-directions
author: Larry Klosowski + Claude Opus 5.5 (MAC lane, V60-UX)
status: active (V60 S0-UX, UX-0; checked by src/styles/kitTokens.test.ts)
planset: citrate-federation .agentile/planset/2026-11-07-v060/03_UX.md §1
sprint: citrate-federation .agentile/sprints/active/2026-11-11-V60-S0.md
---

# Citrate kit tokens → app tokens

The kit is `design/_ds/citrate-design-system-244c4cdf-0fbf-406e-9d19-bc9db7d7b48a/colors_and_type.css`.
The app has one token source (Rule 9):

- `src/styles/foundation.css` is the kit's `:root`, token for token. The only change is the font
  family name: the app self-hosts Geist as "Geist Sans" (BC-7.1). The test checks every kit token.
- `src/styles/tokens.css` maps the kit onto two registers. **Charter** (paper, light) is the
  default; **Instrument** (dark, the ALF evergreen field `#0e1a13`) is set with
  `data-register="instrument"`. Components style against the register tokens (`--srf-*`, `--tx-*`,
  `--line-*`, `--accent*`, `--ok`/`--warn`/`--danger`/`--info`, and the `--fl-*`/`--mesh-*` set
  below), never the kit tokens directly.

The instrument surfaces and text colours come from the ALF handoff, not the kit, so most kit
tokens have no instrument mapping. FL live views default to Instrument (03 §1); forms and
settings stay on Charter.

## Colour map

Each row is read by `src/styles/kitTokens.test.ts`: the kit value must be the kit's, and each
register's app token must resolve to the value given. A value that differs from the kit carries
a note. `none` means the register does not use that kit colour.

| Kit token | Kit value | Charter | Instrument | Note |
|---|---|---|---|---|
| `--citrate-green` | `#8ecc09` | `--accent` = `#8ecc09` | `--accent` = `#8ecc09` | The consequential moment. On paper it is a fill, never text (text uses `--accent-text`). |
| `--citrate-green-deep` | `#5a8205` | `--accent-deep` = `#5a8205` | `--fl-ring-done` = `#5a8205` | Instrument's `--accent-deep` is the lighter hover lime; the deep green marks passed ring segments there. |
| `--citrate-green-dark` | `#2f4502` | `--fl-ring-current` = `#2f4502` | none | The current ring segment on paper (the lime fails 3:1 there). |
| `--citrate-green-tint` | `#e8f3c6` | `--accent-wash` = `#e8f3c6` | none | |
| `--citrate-yellow` | `#ffbd10` | none | `--warn` = `#ffbd10` | Charter uses it only for text selection; charter warnings use a darker yellow for AA. |
| `--ink` | `#0e0f0c` | `--tx-1` = `#0e0f0c` | `--on-accent` = `#0e0f0c` | |
| `--paper` | `#f4f1ea` | `--srf-0` = `#f4f1ea` | none | Also `--fl-on-band` on charter. |
| `--paper-2` | `#faf8f3` | `--srf-1` = `#faf8f3` | none | |
| `--paper-pure` | `#ffffff` | `--srf-2` = `#ffffff` | none | |
| `--stone-50` | `#f1efe8` | `--srf-inset` = `#f1efe8` | none | |
| `--stone-200` | `#d9dad4` | `--line-1` = `#d9dad4` | none | |
| `--stone-300` | `#c3c4be` | `--line-2` = `#c3c4be` | none | |
| `--stone-700` | `#555851` | `--tx-2` = `#555851` | none | |
| `--stone-500` | `#84867f` | `--tx-3` = `#6b6d67` | none | Darkened for AA (HUP-S10.6): the kit value is 3.3:1 on paper. |
| `--deep-evergreen` | `#0f2a1a` | `--fl-band` = `#0f2a1a` | `--fl-band` = `#0f2a1a` | The FL hub header band in both registers. |
| `--semantic-success` | `#4f8a05` | `--ok` = `#467904` | `--ok` = `#8ecc09` | Charter darkened for AA on its wash (HUP-S10.6, pending owner sign-off); instrument uses the lime. |
| `--semantic-success-bg` | `#ecf5d4` | `--ok-bg` = `#ecf5d4` | none | |
| `--semantic-warning` | `#b07b00` | `--warn` = `#8f6400` | `--warn` = `#ffbd10` | Charter darkened for AA (HUP-S10.6); instrument uses the brand yellow. |
| `--semantic-warning-bg` | `#fff1c4` | `--warn-bg` = `#fff1c4` | none | |
| `--semantic-danger` | `#a72414` | `--danger` = `#a72414` | `--danger` = `#dd7259` | The kit red fails 3:1 on the dark field; instrument uses a light coral. |
| `--semantic-danger-bg` | `#f6e1de` | `--danger-bg` = `#f6e1de` | none | |
| `--semantic-info` | `#1b4965` | `--info` = `#1b4965` | `--info` = `#4cc3d5` | Instrument uses the cyan zone hue. |
| `--semantic-info-bg` | `#dbe7ef` | `--info-bg` = `#dbe7ef` | none | |

Type, spacing, radii, shadows and motion are shared by both registers and come straight from the
kit through foundation.css: Space Grotesk display, Geist body, Geist Mono for numbers, hashes and
addresses; 8 pt spacing (`--s-2` = 8px); motion 140/220/420/900 ms (`--dur-fast`, `--dur-base`,
`--dur-slow`, `--dur-narrative`).

## FL and mesh tokens (new in UX-0)

Declared in both register blocks of `tokens.css`, each naming another token. Every state colour
meets 3:1 against `--srf-0`, `--srf-1` and `--srf-2` of its register (WCAG 1.4.11), and colours
inside a group differ. Colour is never the only signal: ring segments carry labels, lease dots
differ by shape (hollow, filled, tick, struck), mesh edges by line style (03 §4).

| Token | Draws | Charter | Instrument |
|---|---|---|---|
| `--fl-band` / `--fl-on-band` | FL hub header band and its text | `--deep-evergreen` / `--paper` | `--deep-evergreen` / `--tx-1` |
| `--fl-ring-track` | ring segments not reached yet | `--line-2` | `--line-2` |
| `--fl-ring-done` | segments passed | `--citrate-green-deep` | `--citrate-green-deep` |
| `--fl-ring-current` | the current segment | `--citrate-green-dark` | `--accent` |
| `--fl-ring-disputed` | Disputed | `--warn` | `--warn` |
| `--fl-ring-expired` | Expired | `--tx-3` | `--tx-3` |
| `--fl-lease-leased` | hollow dot | `--tx-2` | `--tx-2` |
| `--fl-lease-submitted` | filled dot | `--tx-1` | `--tx-1` |
| `--fl-lease-verified` | tick | `--ok` | `--ok` |
| `--fl-lease-rejected` | struck dot | `--danger` | `--danger` |
| `--fl-escrow-paid` / `-reserved` / `-refundable` | escrow bar parts | `--ok` / `--info` / `--tx-3` | `--ok` / `--info` / `--tx-3` |
| `--mesh-edge-direct` / `-relayed` | cluster map edges (solid / dashed) | `--ok` / `--tx-2` | `--ok` / `--tx-2` |
