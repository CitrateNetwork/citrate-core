---
created: 2026-10-04
branch: hup/n6-size-licence (addendum section 8 on hup/n7-size-licence-release-prep, 2026-10-04; sections 9 and 10 on release/v0.5.0-gates, 2026-10-06)
author: Larry Klosowski + Claude Opus 5.5
status: prepared for the v0.5.0 owner signature (section 10), unsigned
---

# Licence review: bundled and first-run components

Gate **g3-licence** of the Hermes upskill planset (`.agentile/planset/2026-09-30-hermes-upskill/`,
gates.yaml: "Licence review for bundled/first-run tools complete; source offer published"), and the
licence half of WP S6.0.

This is an engineering review written for the owner and counsel. It is not legal advice. The gate
closes when the owner (or counsel) signs section 6 and the source offer is published.

The machine-readable inventory is [`release/licences.json`](../release/licences.json). The check
`node scripts/licence-inventory.mjs` fails when anything the app ships has no entry, when an entry
names a licence text that does not exist, when a third-party entry that ships with the app names no
licence text under `src-tauri/licenses/`, or when a shipped copyleft entry has no source offer.
This page explains the findings; the JSON file is the record.

## 1. What was reviewed

Everything Citrate Core puts on a member's machine, in three groups:

| Group | Where it comes from | Inventory keys |
|---|---|---|
| In the installer | `externalBin` and `resources` of every `src-tauri/tauri.bundle-*.conf.json` | `externalBin:`, `resource:` |
| Downloaded on first run | `components/toolchain-bundle.json` tools, `templates/deps.lock.json` libraries | `toolchain:`, `library:` |
| Text the app carries | the reviewed skills (`skills.lock`) and the Hermes knowledge corpus (`manifest.json` of the staged corpus) | `skills:`, `corpus:` |

Two first-run components the web lane added (managed Chromium as Chrome for Testing, and
SearXNG) are listed as `toolchain:` entries since the stack merge; neither installs until its
signed component manifest exists. Citrate's own contract templates (MIT-headed Solidity, bundled
with the installer since the dApp forge lane) have their own entry.

Checked on 2026-10-04 against the staged corpus `9709668…` (15 sources, all included) and the
240-skill `skills.lock`:

```text
$ node scripts/licence-inventory.mjs --corpus src-tauri/knowledge-corpus
licence inventory: OK
  35 entries, 48 shipped keys, corpus checked
  review ok: 19, actions open: 5, owner decisions open: 11
  sign-off: pending owner sign-off
```

## 2. Summary

| Component | Licence | How it ships | Review |
|---|---|---|---|
| citrate node, Hermes sidecar | Apache-2.0 (repo LICENSE) | installer | owner: LICENSE vs NOTICE mismatch (4.6) |
| node-agent | Apache-2.0 | installer | action: crate notices (4.7) |
| mem-mcp, comms-member-daemon, cluster-daemon | BUSL-1.1 | installer | action: crate notices (4.7) |
| llama.cpp (llama-server, dylibs) | MIT | installer | ok |
| Kubo 0.42.0 (ipfs) | MIT OR Apache-2.0 | installer | ok (texts now bundled) |
| bge-base-en-v1.5 | MIT | installer | ok (text now bundled) |
| Gemma 4 E4B-it Q4_0 | Apache-2.0 | installer (release overlay), first-run download (lite) | ok (text now bundled) |
| Citrate skills, capsules, docs corpus | BUSL-1.1 | installer | ok |
| Trail of Bits skills | CC-BY-SA-4.0 | skills bundle, corpus | ok (D-20) |
| frontend-skills | MIT (README) and Apache-2.0 parts | skills bundle, corpus | owner: no LICENSE file upstream |
| agentile-skills | Apache-2.0 | skills bundle, corpus | ok |
| Hermes agent skills | MIT | skills bundle, corpus | ok |
| citrate-docs | Apache-2.0 | corpus | ok |
| Gradient Papers, AGENTILE.md | first party | corpus | action: rebuild from clean checkouts (4.8) |
| OpenZeppelin, Solady | MIT | first-run download, corpus | ok |
| forge-std, Foundry book | MIT OR Apache-2.0 | first-run download, corpus | ok |
| **Medusa docs, Slither docs** | **AGPL-3.0-only** | corpus | owner: source offer publication (4.3) |
| **solc, Aderyn** | **GPL-3.0-only** | first-run download | owner: mirror or upstream (4.2) |
| **Slither, Medusa** | **AGPL-3.0-only** | first-run download | owner: mirror or upstream (4.2) |
| Foundry | MIT OR Apache-2.0 | first-run download | ok |
| CPython (python-build-standalone) | PSF-2.0 | first-run download | ok |
| Node.js | MIT | first-run download | ok |
| Contract templates | MIT (first party) | installer | ok |
| **Managed Chromium (Chrome for Testing)** | **Google Chrome for Testing terms; Chromium BSD-3-Clause and others** | first-run download (not installed before the component signing) | owner: re-host or fetch from Google, notices |
| **SearXNG** | **AGPL-3.0-or-later** | first-run download (not built yet) | owner: distribution approach and source offer |

## 3. What changed in this review

- The app now bundles the third-party licence texts it was missing, as the `licenses/*` resource
  (`src-tauri/licenses/`, 168 KB, added to all four bundle configs): Kubo, bge-base-en-v1.5,
  Gemma 4, the Trail of Bits CC BY-SA 4.0 text, the Hermes agent MIT text, hyperframes (Apache-2.0),
  Medusa and Slither (AGPL-3.0), the Foundry book, OpenZeppelin, Solady and forge-std. Before this,
  Kubo and the BGE model shipped with no notice, which their MIT licences require.
- `release/licences.json` and `scripts/licence-inventory.mjs` (with tests) make a missing entry a
  failing check instead of a review finding.

## 4. Findings

### 4.1 The BUSL app and the copyleft tools are separate programs

Citrate Core never links slither, medusa, aderyn or solc. It downloads them as separate
executables (signed component manifest, verify-then-swap) and runs them as subprocesses with
command-line arguments and files. Under the GPL FAQ's reading, that is aggregation of separate
programs, not a combined work, so the GPL/AGPL does not reach the BUSL-1.1 app. Counsel should
confirm this for the record; the engineering facts are in `docs/COMPONENT_UPDATER.md` and
`components/`.

### 4.2 Upstream fetch versus a Citrate mirror (solc, Aderyn, Slither, Medusa)

Today `components/toolchain-bundle.json` points at upstream URLs and the signed manifest pins the
bytes. Fetched that way, the member downloads from the upstream project and Citrate does not
convey the binary. Two cases change that, and both are open:

- **A Citrate mirror** (an open owner decision in `docs/COMPONENT_UPDATER.md`): Citrate then
  conveys GPL/AGPL object code and must ship the licence text with it and give access to the
  Corresponding Source (GPLv3 section 6). Mirroring the source archives next to the binaries
  (section 6(d)) is the simplest way to satisfy that.
- **The Slither wheelhouse** is `to_be_built`: Citrate would build and host it, which is conveying
  whatever the mirror decision is. It must carry the AGPL/GPL texts and a source offer for slither
  and each dependency in the wheelhouse.

Section 13 (network use) applies only to a modified AGPL program offered over a network. The
members run unmodified slither and medusa on their own machines, so it does not apply today.

### 4.3 AGPL documentation in the knowledge corpus (Medusa, Slither)

Included by owner decision (2026-10-01). The corpus chunks the Markdown into memory nodes and
records upstream, commit, licence, attribution and "Changed: text chunked for search" in its
`NOTICE.md`, and the AGPL text now ships in `licenses/`. Open items:

- **Source offer publication.** The source of the chunked text is the upstream `docs/src` at the
  commit in `manifest.json`. The owner picks between linking upstream at that commit (option A) and
  publishing a copy of those files on a Citrate host (option B, robust if upstream rewrites history).
- **Aggregate.** The `refs` tenant file holds AGPL chunks beside MIT/Apache ones. AGPL section 5
  treats a compilation of separate works as an aggregate when it does not limit the users' rights in
  the parts. Counsel should confirm the tenant file reads that way, or the corpus builder can put the
  AGPL sources in a tenant of their own (a corpus build change, not done here).

### 4.4 CC BY-SA 4.0 (Trail of Bits skills)

Cleared by D-20 (2026-09-30) with attribution and a note of changes; both are in the corpus
`NOTICE.md` and the full licence text now ships. The skills with executables stripped are
adaptations: they stay under CC BY-SA 4.0 (ShareAlike), which the bundle already says. The app is
a collection, not an adaptation, so ShareAlike does not reach it.

### 4.5 Apache-2.0 NOTICE files

`impeccable` ships its `LICENSE` and `NOTICE.md` inside `skills-bundle`; `frontend-design` ships its
`LICENSE.txt`. hyperframes has no NOTICE upstream; its Apache text is now in `licenses/`.
frontend-skills itself has only a README statement of MIT and no LICENSE file (A18, owner).

### 4.6 First-party licence mismatch

`citrate-chain`, `citrate-agent-runtime` and `citrate-node-agent` carry Apache-2.0 `LICENSE` files
and `license = "Apache-2.0"` in Cargo.toml, while the federation `NOTICE` lists citrate-chain and
citrate-agent-runtime as part of the BUSL-1.1 Licensed Work. Both cannot be right. Owner call:
which licence the node and the Hermes sidecar binaries ship under.

### 4.7 Third-party crate and Go module notices

The first-party binaries statically link hundreds of Rust crates (and Kubo many Go modules). Most
are MIT or Apache-2.0, which ask for the notice to travel with the binary. No notice file is
generated today. The fix is mechanical: generate one per binary at release time (for example
`cargo about generate` or `cargo deny check licenses` plus a template) and bundle it under
`licenses/`. Not done here: it needs a tool choice and a release-step change.

### 4.8 Reproducible corpus provenance

The staged corpus records `gradient-papers` and `agentile` at commit `ca5b2e7…-dirty`, because the
build read a working tree with local changes. The release corpus should be built from clean
checkouts so its NOTICE names a commit anyone can fetch.

## 5. Draft source offer (for owner sign-off)

Proposed text for the app's Settings, About, Licences panel and for a public page (location to be
chosen by the owner). Placeholders in angle brackets.

> Citrate Core includes or downloads open-source software under the GNU GPL v3 and GNU AGPL v3:
> solc, Aderyn, Slither and Medusa (developer tools Citrate Core installs when you ask for them),
> and the Medusa and Slither documentation included in Hermes's knowledge library. These programs
> run as separate programs; Citrate Core is licensed separately under BUSL-1.1.
>
> You can get the complete corresponding source code for each, at the exact version Citrate Core
> uses, from <the list below / https://citrate.ai/legal/source>. The licence texts are in the app
> under Resources/licenses. This offer is valid for at least three years from the date you
> received the software and for as long as Citrate offers the software or support for it.
>
> | Component | Licence | Version | Source |
> |---|---|---|---|
> | solc | GPL-3.0 | 0.8.36 | https://github.com/ethereum/solidity/tree/v0.8.36 |
> | Aderyn | GPL-3.0 | 0.6.8 | https://github.com/Cyfrin/aderyn/tree/aderyn-v0.6.8 |
> | Slither | AGPL-3.0 | 0.11.6 | https://github.com/crytic/slither/tree/0.11.6 |
> | Medusa | AGPL-3.0 | 1.5.1 | https://github.com/crytic/medusa/tree/v1.5.1 |
> | Medusa docs | AGPL-3.0 | commit in the corpus manifest | https://github.com/crytic/medusa (docs/src) |
> | Slither docs | AGPL-3.0 | commit in the corpus manifest | https://github.com/crytic/slither (docs/src) |
>
> Questions: <contact address>.

Versions in the table come from `components/toolchain-bundle.json` and the corpus `manifest.json`;
regenerate the table from those files when either changes rather than editing it by hand. The tag
names were checked against each upstream release list on 2026-10-04.

## 6. Owner sign-off checklist

Each item is "pending owner sign-off". When all are decided, set `sign_off` in
`release/licences.json` to `signed` with the name and date; `--require-sign-off` then passes.

1. Section 4.1: separate-programs reading confirmed (owner or counsel).
2. Section 4.2: upstream fetch or Citrate mirror for solc, Aderyn, Slither, Medusa; who builds and
   hosts the Slither wheelhouse.
3. Section 4.3: source offer publication for the AGPL docs (option A upstream links, option B
   Citrate copy), and the aggregate reading of the `refs` tenant.
4. Section 4.5: frontend-skills LICENSE file upstream, or confirm the README grant (A18).
5. Section 4.6: licence of the citrate node and Hermes sidecar binaries.
6. Section 4.7: approve a crate-notice generator for the release step.
7. Section 5: the offer text, its URL and the contact address.

## 7. Running the check

```sh
node scripts/licence-inventory.mjs                                    # repo-only check
node scripts/licence-inventory.mjs --corpus src-tauri/knowledge-corpus  # plus the staged corpus
node scripts/licence-inventory.mjs --require-sign-off                 # release checklist: fails until signed
```

Tests: `scripts/licence-inventory.test.mjs` (committed inventory, fixture repos with one defect
each, corpus manifest coverage, CLI exit codes).

## 8. Addendum, 2026-10-04: release prep (pending owner sign-off)

Sections 1 to 7 stand as written. This addendum records what the release-prep lane built for 4.7
and 4.8; nothing in it changes `sign_off`, and the generator choice is still checklist item 6.

**4.7, crate and Go module notices: generator built.** `scripts/third-party-notices.mjs` runs
cargo-about (0.9.2, config `release/about.toml`) on the app and the six Rust sidecars and
go-licenses (v2.0.1) on Kubo's `cmd/ipfs` (plus the Go standard library), for
`aarch64-apple-darwin`, as configured in `release/notices.json`. It writes one file,
`src-tauri/licenses/THIRD-PARTY-NOTICES.txt`, which ships through the existing `licenses/*`
resource: every third-party package with its licence and the full text, each distinct text once
with the copyright lines of the packages it applies to. First run: 2,231 packages across 8
components, 107 distinct licence bodies, 656,022 bytes, generated from the federation main
checkouts (revisions in the file header). Every package resolved to a licence on the generator's
permissive list; a package outside it fails the run, so a new copyleft dependency is a reviewed
change. Each covered inventory entry now names the file (`third_party_notices`), and
`licence-inventory --require-notices` (also run by release.yml) fails while the app or a
first-party sidecar has none. Re-run it at release from the revisions the bundled sidecars are
built from (docs/RELEASE.md, step 7).

For the owner: 14 packages are MPL-2.0 (file-level copyleft), used unmodified: cssparser,
cssparser-macros, dtoa-short and selectors (app), option-ext, colored, priority-queue (MPL-2.0
chosen over LGPL-3.0), hpke-rs, hpke-rs-crypto and hpke-rs-rust-crypto (comms), and HashiCorp
go-version, golang-lru (v1, v2) and go-yamux (Kubo). The notices list each with its upstream URL;
whether that URL is enough as the "how to get the source" statement MPL-2.0 section 3.2 asks for is
an owner or counsel call, recommended yes for unmodified files.

**4.8, corpus provenance: gate built, clean rebuild blocked.** `stage-knowledge-corpus.mjs` now
refuses a corpus whose included sources record a `-dirty` commit (`--allow-dirty` for dev builds,
warned). It refuses the current staged corpus (`gradient-papers` and `agentile` at
`ca5b2e7…-dirty`). A clean rebuild cannot remove those two today: `gradient-papers/` is not under
version control (it is untracked inside the citrate-labs metarepo, with no repository of its own),
and the `agentile` source reads the metarepo root, which any checkout with untracked child repos
makes dirty. Owner decision needed: put `gradient-papers` under version control (recommended: its
own private repository), after which mem-corpus records a clean commit for it. Follow-up for
citrate-memories: scope mem-corpus's dirty check to the files a source includes (for `agentile`,
only `AGENTILE.md`), so untracked child checkouts beside it do not mark it dirty.

## 9. v0.5.0 review, 2026-10-06 (prepared for the owner's signature)

Sections 1 to 8 stand as written. This section re-checks the inventory against what v0.5.0
ships, records the owner's decisions of 2026-10-04 on the open items, and lists what is still
open. Owner decision 2026-10-06: the owner signs this review for 0.5.0 (it is not waived).

### 9.1 What v0.5.0 ships, checked against the inventory

Read from the four `src-tauri/tauri.bundle-*.conf.json` files, `src-tauri/tauri.conf.json`,
`components/toolchain-bundle.json`, `templates/deps.lock.json`, `skills.lock` and the corpus spec
(citrate-memories `corpus/hermes-knowledge.toml`):

| What | Inventory entry | Result |
|---|---|---|
| Eight sidecars: citrate node, node-agent, mem-mcp, llama-server, ipfs (Kubo), comms-member-daemon, cluster-daemon, Hermes | one entry each (`externalBin:`) | covered; Rust and Go notices regenerated (9.2) |
| llama runtime libraries (`llama/*`) | `llama.cpp` | covered; vendored-library texts added (9.2), web UI open (9.3) |
| BGE model and its GGUF, Gemma 4 GGUF (node flavour only) | `bge-base-en-v1.5`, `gemma-4-e4b` | covered |
| skills, skills bundle, docs corpus, knowledge corpus, capsules, contract templates, `licenses/*` | first-party and per-source entries | covered; the 15 corpus source ids in the spec equal the inventory's `corpus:` covers |
| **The webview** (Vite build of `src/`, `frontendDist`) | **none before this review** | **gap, fixed** (9.2) |
| First-run components: solc, Foundry, Python, Slither, Aderyn, Medusa, Node.js, Chrome for Testing, SearXNG | `toolchain:` entries | covered. None of them can install in 0.5.0: the production component key slot is empty, so the updater refuses before any download (`components/src/key.rs`, `docs/COMPONENT_UPDATER.md`) |
| Solidity libraries for new projects (OpenZeppelin, Solady, forge-std) | `library:` entries | covered |

`node scripts/licence-inventory.mjs --require-notices` on this branch: OK, 37 entries, 45 shipped
keys, review ok 24, actions open 2, owner decisions open 11 (closed by section 10), sign-off pending.
The staged knowledge corpus is not in this checkout; the release corpus (checklist A9) must pass
`--corpus` before the cut (9.3).

### 9.2 Gaps found and fixed in this review

1. **The webview's npm packages and fonts had no notices.** The installer's webview carries the
   npm packages Vite bundles and the font files the stylesheet loads. The licence check did not
   know the webview ships and the notices generator scanned only Rust and Go. Fixed:
   `licence-inventory` now requires an entry for `frontendDist` (`app-webview`) and, with
   `--require-notices`, its notices; `third-party-notices` gained an npm collector that builds the
   webview with Vite (nothing written) and lists only packages whose modules or assets are in the
   output. Result: 24 packages (React, viem, wagmi, TanStack Query, jose, the Tauri JS API and
   plugins, and their dependencies) and the three `@fontsource` font packages (Geist Sans,
   Geist Mono, Space Grotesk, OFL-1.1, unmodified). Installed packages that are not in the output,
   such as the MetaMask SDK (a proprietary licence) and the WalletConnect stack that wagmi's
   optional connectors pull in, are not shipped; the generator re-checks this on every run, so a
   new import that pulls one in fails the release step. OFL-1.1 is allowed for this component
   only (`release/notices.json`), as a reviewed licence (attestation 10.6).
2. **llama.cpp's vendored libraries had no notices.** llama-server and libmtmd compile
   cpp-httplib (MIT), nlohmann/json (MIT) and stb_image (MIT or public domain), plus miniaudio
   (MIT-0 or public domain) and subprocess.h (Unlicense). Their texts are now in
   `src-tauri/licenses/llama.cpp-vendored.LICENSE` (from tag b8640).
3. **The notices were generated from older revisions.** Regenerated on 2026-10-06 from the release
   head and the current federation mains: citrate-chain `2979a15`, citrate-memories `0e9d488`,
   citrate-cluster `755cf21`, citrate-agent-runtime `397a6b1` (node-agent, comms and Kubo
   unchanged). Every package still resolves to the permissive list (MPL-2.0 the only file-level
   copyleft, 14 packages, all unmodified) or to the webview's reviewed OFL-1.1.

### 9.3 Still open (engineering actions, not legal judgments)

| # | Item | Blocks the 0.5.0 signature? | Action |
|---|---|---|---|
| a | llama-server embeds llama.cpp's web UI (default build option `LLAMA_BUILD_WEBUI=ON`). Core never serves it (`--no-webui`), but its npm packages ship inside the binary without notices | yes, unless fixed or accepted | Build the bundled llama-server with `-DLLAMA_BUILD_WEBUI=OFF` on every OS (recommended), or generate the web UI's notices from its lockfile |
| b | The notices must match the binaries actually bundled. The node is rebuilt at the new genesis commit and Hermes after the pending runtime changes (checklist B4, A7, B5) | yes | Re-run docs/RELEASE.md step 7 from those revisions after B5; the file header records them |
| c | The release knowledge corpus records `gradient-papers` and `agentile` at a `-dirty` commit (4.8); `gradient-papers` is still not its own repository | yes, if the corpus ships | Build the release corpus from clean checkouts and pass `licence-inventory --corpus`; if `gradient-papers` cannot be clean in time, leave it out of the 0.5.0 corpus |
| d | frontend-skills has no LICENSE file upstream (owner decision: add it) | no (the README grant stands meanwhile) | Add the MIT LICENSE to `saulbuilds/frontend-skills` |
| e | The source-offer page at citrate.ai/source-offer is not published yet; the gate says "source offer published" | yes (gate text) | Publish the text in 9.5 on citrate-landing and link it from Settings > About > Licences |
| f | Slither and SearXNG are described as Citrate-packed wheelhouses (`to_be_built`), which would be Citrate conveying the GPL/AGPL code, while the owner chose upstream fetch with no re-host | no (neither can install in 0.5.0) | Before either installs (0.5.1, after the component key ceremony): fetch each pinned wheel from upstream by URL and sha256, or record a decision to host them with the source offer |
| g | Licence of the citrate node and Hermes sidecar binaries (4.6: Apache-2.0 files vs the BUSL list in the federation NOTICE) | no; the owner signs with the item named as open | Resolve with counsel (owner decision 2026-10-04); attestation 10.8 records it |

### 9.4 What v0.5.0 conveys, by licence family

- **Permissive** (MIT, Apache-2.0, BSD, ISC and similar): the bulk; notices in
  `THIRD-PARTY-NOTICES.txt` and the per-component texts in `licenses/`.
- **MPL-2.0, file-level copyleft, unmodified:** 14 Rust and Go packages, each with its upstream
  URL in the notices.
- **OFL-1.1, unmodified fonts:** three @fontsource packages; their licence and copyright lines are
  in the notices (OFL-1.1 permits bundling fonts with software when the licence goes with them).
- **CC BY-SA 4.0:** the Trail of Bits skills and their corpus text, with attribution and a note of
  changes (D-20).
- **AGPL-3.0 text:** the Medusa and Slither documentation chunked into the knowledge corpus.
- **GPL-3.0 and AGPL-3.0 programs (solc, Aderyn, Slither, Medusa, SearXNG):** not conveyed by
  0.5.0; they cannot install until the component key exists, and they will then come from the
  upstream URLs pinned by sha256. Chrome for Testing likewise.

### 9.5 Source offer (owner decision 2026-10-04: option A, upstream links at pinned commits)

To publish at `https://citrate.ai/source-offer` and in Settings > About > Licences. Supersedes
the placeholders of section 5; the contact address is the one placeholder left for the owner.

> Citrate Core includes or can download open-source software licensed under the GNU GPL v3 and
> the GNU AGPL v3: the Medusa and Slither documentation in Hermes's knowledge library, and the
> developer tools solc, Aderyn, Slither and Medusa and the private search engine SearXNG, which
> Citrate Core downloads from their upstream projects when you turn them on. These run as separate
> programs; Citrate Core is licensed separately. Citrate Core runs SearXNG unmodified, on your own
> machine only, reachable from this machine only.
>
> The complete corresponding source for each is published by its upstream project at the exact
> version Citrate Core uses:
>
> | Component | Licence | Version | Source |
> |---|---|---|---|
> | solc | GPL-3.0 | 0.8.36 | https://github.com/ethereum/solidity/tree/v0.8.36 |
> | Aderyn | GPL-3.0 | 0.6.8 | https://github.com/Cyfrin/aderyn/tree/aderyn-v0.6.8 |
> | Slither | AGPL-3.0 | 0.11.6 | https://github.com/crytic/slither/tree/0.11.6 |
> | Medusa | AGPL-3.0 | 1.5.1 | https://github.com/crytic/medusa/tree/v1.5.1 |
> | SearXNG | AGPL-3.0-or-later | 2026.10.4+d48c4b555 | https://github.com/searxng/searxng/tree/d48c4b555421e824342c51d68482dd0898e54d0f |
> | Medusa docs | AGPL-3.0 | commit in the corpus manifest | https://github.com/crytic/medusa/tree/&lt;commit&gt;/docs/src |
> | Slither docs | AGPL-3.0 | commit in the corpus manifest | https://github.com/crytic/slither/tree/&lt;commit&gt;/docs/src |
>
> Every download is checked against the SHA-256 recorded in the signed component manifest, so
> what you run is byte for byte the upstream release named above. The licence texts are in the
> app under Resources/licenses. If you cannot get the source from these links, write to
> &lt;contact address&gt; and we will send it to you, for at least three years from the date you
> received the software.

Fill the two docs commits from the release corpus `manifest.json` when it is staged (A9).

## 10. Owner signature: v0.5.0 licence review

The gate (`g3-licence`) closes when the owner signs below and the source offer is published
(9.3 e). Signing attests to the statements 10.1 to 10.9 for the v0.5.0 release and nothing else;
each names the record it rests on. It is not legal advice and does not replace counsel's review
of 10.8.

**I, Larry Klosowski, for Citrate Inc., attest that for Citrate Core v0.5.0:**

- 10.1 **Inventory.** `release/licences.json` names a licence for everything the 0.5.0 installer
  ships, downloads on first run or carries as text, as checked by
  `node scripts/licence-inventory.mjs --require-notices` (and `--corpus` on the staged release
  corpus), with the items in 9.3 a, b, c and e done.
- 10.2 **Notices.** `src-tauri/licenses/THIRD-PARTY-NOTICES.txt` is regenerated from the
  revisions the 0.5.0 binaries are built from (9.3 b) and ships in the `licenses/*` resource with
  the per-component texts.
- 10.3 **Separate programs (4.1).** The BUSL-1.1 app does not link the GPL/AGPL tools; it runs
  them as separate programs.
- 10.4 **Source offer (4.2, 4.3).** Option A: the offer in 9.5, at `citrate.ai/source-offer`, with
  upstream links at the pinned commits. Third-party tools are fetched from upstream, pinned by
  sha256, with no Citrate re-hosting; SearXNG runs unmodified and loopback only. Any later change
  to a Citrate mirror or a Citrate-built wheelhouse (9.3 f) is a new decision with its own offer.
- 10.5 **MPL-2.0 (section 8).** The 14 MPL-2.0 packages are used unmodified, and the upstream URL
  listed with each in the notices is the statement of where to get their source.
- 10.6 **OFL-1.1 (9.2).** The three bundled font packages are unmodified and ship with their
  licence and copyright lines.
- 10.7 **CC BY-SA 4.0 (4.4).** The Trail of Bits skills ship with attribution and a note of
  changes, under CC BY-SA 4.0.
- 10.8 **Open, not attested:** the licence of the citrate node and Hermes sidecar binaries (4.6,
  9.3 g) stays with counsel; frontend-skills' upstream LICENSE file (9.3 d) is to be added.
- 10.9 **Record.** On signing, `release/licences.json` `sign_off` becomes
  `{"status": "signed", "by": "Larry Klosowski", "date": "<date>"}`, and the `owner` review states
  the 2026-10-04 decisions settled (solc, Aderyn, Slither, Medusa, Chrome for Testing, SearXNG,
  the Medusa and Slither docs) become `ok` with the decision noted; `--require-sign-off` then
  passes.

Signature: ______________________________ (Larry Klosowski, owner, Citrate Inc.)

Date: ____________

Status: **UNSIGNED**, prepared 2026-10-06.
