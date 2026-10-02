---
created: 2026-10-01
branch: hup/n5-corpus-rest
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# knowledge-corpus (bundled app resource, HUP-S3.1)

This directory is the app resource `knowledge-corpus/`. Every build ships this README
so the bundle resource glob (`knowledge-corpus/**/*` in the `tauri.bundle-*.conf.json`
and `tauri.local-run.conf.json` overlays) always matches. A directory without a
`manifest.json` is an honest "no bundle": the first-run import reports
`skipped: no-bundle`.

A release stages the Hermes knowledge corpus here, verified, with:

```bash
node scripts/stage-knowledge-corpus.mjs <corpus-dir | knowledge-corpus.tar.gz> \
  --mem-mcp src-tauri/binaries/mem-mcp-<target-triple>
```

The corpus (format `citrate-corpus/2`) is built in citrate-memories by
`scripts/build-corpus.sh`; the release workflow pulls it as the pinned
`knowledge-corpus.tar.gz` runtime-deps asset. Staged files are git-ignored. See
`docs/KNOWLEDGE_CORPUS_IMPORT.md` and `docs/RELEASE.md` section 3.
