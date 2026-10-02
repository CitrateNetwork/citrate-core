---
created: 2026-10-01
branch: hup/m2-knowledge
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S3.2
---

# Reviewed third-party skills (staged at build time)

This folder is an app resource. A release build stages the third-party skills that
`skills.lock` admits here, as `<source>/<path>/SKILL.md` plus the files the lock pins, and a
copy of `skills.lock`. Nothing but this README is committed; the staged files are ignored by git.

The Hermes sidecar loads the folder as one locked skill source (`CITRATE_HERMES_SKILLS_LOCK` and
`CITRATE_HERMES_SKILLS_THIRD_PARTY`, set by `src-tauri/src/hermes.rs` when both exist). It checks
every file against the sha256 the lock pins when it loads and again on every read, so a file that
is not pinned is never offered. Scripts are never staged.

- Stage from the source checkouts: `node scripts/stage-skills-bundle.mjs build --sources-base <dir>`
- Check a staged tree: `node scripts/stage-skills-bundle.mjs verify src-tauri/skills-bundle`
- Release: `.github/workflows/release.yml` stages the pinned `skills-bundle.tar.gz` asset with
  `node scripts/stage-skills-bundle.mjs from-tarball`, which refuses an asset that does not match
  this repo's `skills.lock` exactly.

A build without a staged bundle still packages (this README matches the resource glob); the
sidecar then offers no reviewed third-party skills.
