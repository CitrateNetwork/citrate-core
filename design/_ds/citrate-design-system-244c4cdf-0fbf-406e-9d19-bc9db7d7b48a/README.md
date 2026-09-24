# Citrate Network — Design System

> **The Blockchain That Learns.** An AI-native Layer 1 where reaching consensus and training a decentralized AI are the same process. Every block makes the network smarter. Citrate sells **privacy with auditability across an entire organization, bottom-up and at scale** — custom-fit onto each customer's on-premise infrastructure, with optional posting to the public substrate for transparency under their own compliance constraints.

This design system equips designers and engineers to produce work that feels native to Citrate: deliberate, sturdy, expensive, and government-document precise on the surface, with a quiet biological-cryptographic undercurrent. The audience is procurement and operations leaders at defense_prime, DuPont, Toyota, Aramco, state and federal governments — buyers who must feel the work would survive scrutiny in a contractor's vault, a minister's briefing book, and a SOC.

---

## What we sell (and what we don't)

We do **not** sell a public ledger. We offer one — customers may opt to post to it on cadences and within compliance envelopes they define, when transparency serves them. What we sell is:

1. **Privacy with auditability, bottom-up.** Every action across an organization is cryptographically witnessed and queryable by the auditors that organization names — and *only* by them. The substrate provides mathematical proof without surrendering the data.
2. **A custom-fit on-premise stack.** Citrate's Lattice Virtual Machine, MCP orchestration layer, and federated-learning consensus deploy directly onto a customer's hardware, behind their firewall, on their compliance plane. No two installations are identical; every deployment is engineered to the customer's existing on-prem topology.
3. **The path to becoming a Tier 2 data producer.** A customer's global facilities — refineries, ports, factory floors, ground stations, server rooms — can become validating nodes that contribute verifiable inference to the larger network and earn for it. Their compute footprint stops being a cost center and starts producing.

The selling motion is consultative and bespoke. The website and outbound materials carry **deliberate mystery** — enough technical credibility to be respected by a CISO and a regulator, never enough specificity to commoditize what we deliver.

---

## What's in this design system

| File / folder | Contents |
|---|---|
| `README.md` | This file — brand, voice, visual rules, index |
| `styles.css` | **Entry stylesheet** — consumers link this one file; it `@import`s the foundation |
| `colors_and_type.css` | Design tokens — color, type (Space Grotesk display · Geist body · Geist Mono), spacing, radii, shadow, motion, lattice utilities |
| `SKILL.md` | Cross-compatible skill manifest (Claude Code / Agent Skills) |
| `components/` | Exported, consumable React components — `Button` (window.&lt;NS&gt;.Button) |
| `templates/citrate-loader/` | **Citrate Loader** — the animated logo loader (Design Component) |
| `assets/` | Logo marquees, C-mark glyphs, lattice motifs |
| `fonts/` | Webfont notes & substitution log |
| `preview/` | Token & component cards for the Design System tab |
| `ui_kits/marketing/` | Marketing landing — restrained, mystery-forward |
| `ui_kits/console/` | Citrate Operator Console — desktop product UI |
| `ui_kits/mobile/` | Mobile companion — auth, attest-on-the-go, alerts |
| `ui_kits/tablet/` | Tablet briefing surface — reading & approval |
| `pitch_deck/` | 12-slide sales deck template |
| `emails/` | Transactional & briefing email templates |

### Consuming the system

- **Styles:** link `styles.css` (pulls in `colors_and_type.css`). Use the tokens (`var(--citrate-green)`, `var(--paper)`, `var(--font-display)` = Space Grotesk) and semantic classes (`.t-display`, `.t-eyebrow`, `.t-compliance`, `.lattice-dots`).
- **Components:** load `_ds_bundle.js`, then `const { Button } = window.CitrateDesignSystem_244c4c`.
- **Templates:** the **Citrate Loader** is the canonical loading / processing state — copy `templates/citrate-loader/` and tweak its props (`speed`, `cycle`, `turns`, `ringRadius`, `arcSweep`, `thickness`).

---

## Citrate, technically

| | |
|---|---|
| **Consensus** | GhostDAG with k=18 anticone tolerance |
| **Finality** | BFT committee of 100 validators · 67% threshold |
| **Checkpoint** | 50 blocks · ~25 seconds |
| **VM** | Lattice Virtual Machine (LVM) — REVM + 10 precompiles (7 AI + 3 x402) |
| **Orchestration** | MCP (Model Context Protocol) layer |
| **Tokenomics** | SALT |
| **Compatibility** | EVM compatible · Chain ID 40204 (testnet) |
| **Learning** | Mentor-mentee protocol at every checkpoint · LoRA rank 16 · embedding dim 768 |
| **Logic** | Belnap FOUR-valued (handles contradictory outputs without discard) |
| **Governance** | Wyoming DAO LLC · 50/67/14 amendment process (quorum / approval / timelock) · BR1J Constitution |
| **Design lineage** | Cnidarian biology — nerve net consensus (Aurelia), colonial modularity (Physalia), state reversal (Turritopsis), distributed observability (Tripedalia), symbiotic compute (Cassiopea), bloom dynamics, strobilation, nematocyst defense |

In enterprise-deployment language: an on-prem GhostDAG cluster, gated by your IAM, posting summary attestations to whichever auditor (or the public substrate) you authorize, while contributing optional verifiable inference back to the larger network for revenue.

---

## Content fundamentals

### Voice principles

- **Quiet authority.** The brand never raises its voice. Headlines are observations, not promises. "The audit is the system." "Every block makes the network smarter." "Witnesses, not watchers."
- **Mystery before specifics.** Marketing surfaces describe *outcomes* (privacy, auditability, revenue from idle compute) and *invariants* (cryptographic proof, on-prem deployment, bottom-up coverage). Implementation detail — GhostDAG, LVM, mentor-mentee, FOUR-valued logic — appears in the technical paper and in conversation, not on the landing page.
- **You speak with operators and regulators, not consumers.** Address auditors, CISOs, CFOs, procurement chiefs, ministry CIOs. Never "users." Never "customers" in marketing copy (it diminishes the buyer); say "members," "operators," "primes," "participants."
- **Plainspoken classical English.** Plain Anglo-Saxon verbs over Latinate jargon when both work. "Run" not "execute." "Witness" not "validate." "Settle" not "finalize."
- **Receipts, not promises.** Concrete numbers, named jurisdictions, audit lineages, mathematical claims.
- **No emoji. No exclamation marks. No "we believe / we built / our mission."**

### Casing & punctuation

- Headlines: **sentence case**. Always.
- Eyebrows, status pills, table headers, form section labels: **UPPERCASE** with `+0.14em` tracking — government-document signature, used sparingly.
- ISO dates in product (`2026‑05‑17`); long-form in prose (`17 May 2026`).
- **Tabular lining figures** for all numerals.
- Em-dashes for asides. Oxford comma. Standard typographic quotes.

### Vocabulary

| Use | Avoid |
|---|---|
| substrate, lattice, ledger, instrument, filing | platform, engine, rocket, magic, supercharged |
| witness, attest, route, custody, mentor | sync, push, blast, validate (in product UI) |
| member, operator, participant, prime, custodian | user, customer (in marketing), client |
| audit, lineage, attestation, checkpoint, finality | log, history, record (alone) |
| privacy, auditability, sovereignty, on-prem | cloud-native, multi-tenant |
| Tier 2 data producer, verifiable inference | mining, staking |

### Sample copy

**Hero (marketing)**
> The audit is the system.
>
> Citrate is the substrate that witnesses every action across your organization — cryptographically, bottom-up, on your hardware. Your auditors see what your operators do. The world sees only what you choose to publish.

**Tier-2 framing**
> Your global facilities are already producing data. With Citrate, they begin producing value — contributing verifiable inference to the network and earning for the compute they were going to spend anyway.

**Product empty state**
> No instruments yet. When a member files within your tenancy, it appears here — witnessed, lineage attached, queryable by the auditors you've authorized.

**Email subject (alert)**
> A filing is awaiting your witness · CTR‑9F4A

---

## Visual foundations

### Mood in one paragraph

Citrate looks like a 21st-century reissue of a Treasury document carrying a quiet undercurrent of cryptographic and biological intelligence. Generous warm paper. A geometric grotesque display — Space Grotesk — that reads as engineered precision rather than ornament. A sturdy geometric humanist sans for the operating surface. Citric green and citric yellow used like a wax seal — to mark the consequential moment, never to decorate. Hairlines, not drop shadows. Solid borders, not gradients. A **lattice motif** — a sparse grid of dots and rules suggesting GhostDAG topology — used at very low contrast as a watermark on hero surfaces and on the back of the pitch deck. Motion is slow, considered, and physical — pages settle, marks witness themselves into place, status changes feel earned.

### Color

- **Citrate Green `#8ecc09`** — primary accent. The moment of action and proof. Witness, settle, sign, confirm. Never a fill larger than a button or a single highlight.
- **Citrate Yellow `#ffbd10`** — secondary accent. Evidence, attention, pending state. Pending witnesses, warnings.
- **Deep Evergreen `#0f2a1a`** — trust anchor. Dark navigation, footer, pitch-deck dark slides, data-viz fills.
- **Ink `#0e0f0c`** — primary text. Warm, slightly green-leaning to harmonize with the citrus.
- **Paper `#f4f1ea` / `#faf8f3` / `#ffffff`** — page, raised card, document. Warm cream over cold white; pure white reserved for instruments and dialogs.
- **Stone scale** — warm neutrals for type, dividers, surfaces. No cool greys.
- **Semantic** — success (deeper green), warning (deeper yellow), danger (brick `#a72414`), info (navy `#1b4965`). All with paired flat tints.

**Hard rules.** No gradients. No colored shadows. Accents are reserved — if brand colors cover more than 8 % of a screen's surface, the design is wrong.

### Typography

| Role | Family | Weights | Notes |
|---|---|---|---|
| Display | **Space Grotesk** (variable wght) | 360–500 | Sentence-case headlines, never bold. Engineered, precise, faintly technical — sits naturally over the lattice motif. |
| UI sans | **Geist** | 400 / 500 / 600 | Sturdy humanist geometric. All product chrome and body. |
| Mono | **Geist Mono** | 400 / 500 | The compliance label — uppercase, +0.14em tracking — is the brand's visual signature. |

Long-form prose (`.t-prose`) is set in **Geist**, not the display face — Space Grotesk is for headlines, ledes, and big numerals, never for paragraphs.

⚠ The wordmark's closest commercial match is **Söhne**. Geist ships free, sturdy, and harmonizes; if a Söhne license is acquired, swap `--font-sans` and everything else absorbs cleanly.

### The lattice motif

Citrate's one decorative element is a **lattice** — a sparse grid of small dots and hairline rules at the corners, suggesting GhostDAG topology without literal illustration. The lattice is:

- **Always single-color** (stone-200, citrate-green at 8 % opacity, or evergreen on dark surfaces)
- **Never the focal point** — a watermark, a corner cap, a hairline through dead space
- **Geometric, never organic** — no curves, no flourishes, no "data flow" swooshes
- **Used at most once per surface** — hero only, deck cover only, footer or sign-in panel only

Defined as utility classes in `colors_and_type.css` (`.lattice-grid`, `.lattice-dots`).

### Borders, corners, surfaces

- Hairlines, not drop shadows. Cards: `1px solid var(--border-1)`. Shadows only on modals and popovers.
- Restrained radii: default `0`; buttons `6px`; cards `8px`; modals `12px`. Pills `999px`, used only for status.
- Cards sit on paper, with a paper-2 fill and a 1px stone-200 border. Hover darkens the border one step.

### Motion

- Hover: color shift only. No scale, no shadow flare.
- Press: 1px translate or inset color step.
- Page transitions: 220 ms cross-fade with subtle shift.
- **Filing choreography** (the brand's one showy moment): a row strokes itself in green, then the witness mark stamps in with a faint stagger. Used once per signed action.
- The animated C-mark on the hero is the brand's *one* spring easing. Everywhere else, motion is single-property and unhurried.

### Layout

- Marketing: 1280 px container, 96 px section rhythm, 12-col / 24-gutter grid.
- Product: 8-col data grid with collapsing right panel; dense but never cramped.
- Mobile: single column, generous 24px gutters. Mobile keeps the same type/color tokens — only spacing and grid contract.

### Backgrounds, photography, illustration

- Backgrounds are solid paper. **Never** gradient, image-backed, or noise-textured.
- Photography (when used) is duotone (ink + paper) with a single citric accent overlay. Wide-angle documentary subjects — port cranes, refineries, courthouse façades, ground stations, server halls. Never stock-photo handshakes, never glowing-server long exposures.
- **No hand-drawn illustration. No flat-vector character illustration.** Diagrams are geometric, monochrome, vector — the lattice motif is the closest thing we permit to illustration.

---

## Iconography

⚠ Until bespoke marks are commissioned, Citrate uses **Lucide** at `1.5px` stroke, augmented by bespoke replacements for operations-critical concepts.

- Open, geometric, single-stroke. No filled icons.
- 24 × 24 canvas, 18–22 px optical. Inline at 16 × 16, `vertical-align: -2px`.
- `currentColor` always. The citric accents are reserved for status, never decoration.
- **No emoji anywhere.** Not in marketing, not in product, not in email.

Operations-critical icons that need bespoke production: `witness`, `attest`, `route`, `custody`, `instrument`, `lattice-node`, `tier2-producer`.

### Logos & marks

In `assets/`:

- `citrate_marquee_{black,green,yellow,white}.svg` — the canonical wordmark + mark lockup, four colorways.
- `citrate_mark_{black,green,yellow,white}.svg` — the standalone C-mark glyph (favicon, badge, social profile). All four normalized to identical viewBox dimensions so they render at the same size in any container.

The mark itself is a stylized C made of overlapping curved segments — it reads simultaneously as citrus fruit cross-section, as orbital arcs, and as a knotted ledger seal. Never deform, never recolor outside the four canonical variants, never enclose in a container shape.

---

## Surfaces in this system

- **Marketing** (`ui_kits/marketing/`) — landing site, restrained and mystery-forward
- **Console** (`ui_kits/console/`) — desktop operator product (auth, dashboard, ledger, filing detail)
- **Mobile** (`ui_kits/mobile/`) — mobile companion (auth, attestations, alerts, quick-witness)
- **Tablet** (`ui_kits/tablet/`) — tablet briefing surface (filing approval, witness queue, daily brief)
- **Pitch deck** (`pitch_deck/`) — 12-slide consultative sales deck
- **Emails** (`emails/`) — welcome, attest-required alert, weekly briefing

Each surface has its own README and renders in the Design System tab as a registered preview card.

---

## Open questions for the customer

1. **Söhne vs Geist** — confirm the canonical sans. Geist ships; Söhne is closer to the wordmark.
2. **Bespoke icon set** — commission the seven operations-critical icons before launch.
3. **Photography library** — supply or commission a duotone documentary set (port, refinery, courthouse, ground station, server hall).
4. **Customer logos** — currently typographic placeholders; replace with permissioned marks.
5. **Confidential variant of the deck** — if you want, we can produce a NDA-only "Annex" variant of the pitch deck with the technical specifics (GhostDAG, LVM, mentor-mentee, FOUR-valued logic) that the public deck deliberately withholds.
