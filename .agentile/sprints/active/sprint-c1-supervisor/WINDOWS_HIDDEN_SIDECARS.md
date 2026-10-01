---
created: 2026-10-01T00:00:00Z
branch: fix/windows-sidecar-exe-suffix-and-updater
author: Sisyphus, directed by Kurt
status: completed
sprint: CORE-C1 Windows sidecar process UX
---

# Windows sidecar process UX

## Gate

The packaged Windows application runs every supervised sidecar without opening
a console window. macOS and Linux process behavior remains unchanged.

## Evidence

- Build and install the NSIS package from the PR 149 v0.4.3 baseline.
- Verify `citrate.exe`, `ipfs.exe`, and `mem-mcp.exe` remain live children of
  `citrate-core.exe` without visible top-level windows.
- Verify the local node answers chain-40204 JSON-RPC with live peers.

## Status

Verified in the installed v0.4.3 NSIS build on Windows: `citrate.exe`,
`ipfs.exe`, and `mem-mcp.exe` remained live and responsive with no top-level
window titles. The app remained responsive, the node reported chain `40204`
with four peers, and no startup panic was recorded.
