---
created: 2026-07-11
branch: main
author: Claude Fable 5 (CORE-S0 scaffold agent), directed by @SaulBuilds
status: active
---

# citrate-core

The federation desktop house: a Tauri full-node app (T1). Scaffold in progress, see federation planset `citrate-federation/.agentile/planset/2026-07-11-citrate-core/`.

## Building — read [BUILDING.md](BUILDING.md) first

`git clone && npm run tauri build` is **not** enough. The bundled `citrate` node
sidecar is ~33 MB and gitignored, so every build machine produces its own:

```bash
scripts/build-sidecar.sh --target aarch64-apple-darwin   # Apple Silicon
npx tauri build --config src-tauri/tauri.bundle-node.conf.json
```

Do not copy a `citrate` binary from elsewhere. Chain 40204 was re-rolled
2026-08-04 with both consensus activation heights set to 0; a sidecar from an
older citrate-chain **silently forks** — it connects, syncs and looks healthy
while computing different state roots. `build-sidecar.sh` checks the source
constants and refuses to build a forking binary. See [BUILDING.md](BUILDING.md)
for macOS targets, verification (state-root parity with the fleet), and the
node-config/bootnode details.
