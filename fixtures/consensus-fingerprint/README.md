---
created: 2026-11-24
branch: feat/059-windows
author: Larry Klosowski + Claude Opus 5.5 (Windows lane, 0.5.9)
status: active
---

# consensus-fingerprint fixture

`windows-98c513ec.json` is the output of `citrate --config <rendered r1007 node.toml> consensus --json`
on the Windows node built from citrate-chain 98c513ec (68434069 + #494), captured on a GitHub-hosted
windows-2022 runner by `.github/workflows/release-windows.yml` (run 45472309246, smoke job). Its
`fingerprint` is 0x2b0f96ee2bb3ff72d69370dc19a7ffd4. `scripts/ci/consensus-fingerprint.test.mjs` uses it
to show that `scripts/ci/consensus-fingerprint.py` reproduces that fingerprint, gives the r1007 cut's
0xa2f16115438794f0410b187c99a14e34 with only git_sha set to 68434069de70, and refuses a change to any
other input.
