---
created: 2026-10-04
branch: hup/n6-size-licence
author: Larry Klosowski + Claude Opus 5.5
status: draft, pending owner sign-off
---

# Licence review: bundled and first-run components

Gate **g3-licence** of the Hermes upskill planset (`.agentile/planset/2026-09-30-hermes-upskill/`,
gates.yaml: "Licence review for bundled/first-run tools complete; source offer published"), and the
licence half of WP S6.0.

This is an engineering review written for the owner and counsel. It is not legal advice. The gate
closes when the owner (or counsel) signs section 6 and the source offer is published.

The machine-readable inventory is [`release/licences.json`](../release/licences.json). The check
`node scripts/licence-inventory.mjs` fails when anything the app ships has no entry, when an entry
names a licence text that does not exist, or when a shipped copyleft entry has no source offer.
This page explains the findings; the JSON file is the record.

## 1. What was reviewed

Everything Citrate Core puts on a member's machine, in three groups:

| Group | Where it comes from | Inventory keys |
|---|---|---|
| In the installer | `externalBin` and `resources` of every `src-tauri/tauri.bundle-*.conf.json` | `externalBin:`, `resource:` |
| Downloaded on first run | `components/toolchain-bundle.json` tools, `templates/deps.lock.json` libraries | `toolchain:`, `library:` |
| Text the app carries | the reviewed skills (`skills.lock`) and the Hermes knowledge corpus (`manifest.json` of the staged corpus) | `skills:`, `corpus:` |

Two components that are planned but not shipped (managed Chromium, SearXNG) are listed as
`planned:` so the review is ready when they arrive.

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
| Managed Chromium | BSD-3-Clause and others | not shipped | owner, when added |
| SearXNG | AGPL-3.0-or-later | not shipped | owner, when added |

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
