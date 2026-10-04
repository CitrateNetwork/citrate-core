---
created: 2026-10-04T00:00:00Z
branch: hup/n7-docs-almanac-retro
author: Larry Klosowski + Claude Opus 5.5
status: evidence (gate g0-formal)
planset: 2026-09-30-hermes-upskill
---

# Evidence: g0-formal, the eight gate-0 TLA+ specs

Gate g0-formal asks for AgentLoop, FolderGrant, WebSigningBudget, SkillPersistence, DeployGate,
SpendBudget, AnchorBatch and DeviceLink to be written and TLC-checked at small bounds. All eight
exist with a `.cfg` in the commits below. TLC 2.19 (rev 5a47802), OpenJDK 27, macOS arm64.

| Spec | Repo, path, commit | Run | Result |
|---|---|---|---|
| AgentLoop | citrate-core `src-tauri/formal/AgentLoop.tla`, `81ef7a0` | re-run 2026-10-04 (`AgentLoop.cfg`) | no error; 2,342 generated, 1,258 distinct, depth 27 |
| FolderGrant | citrate-agent-runtime `agent-grants/formal/FolderGrant.tla`, main `01a32ed` | re-run 2026-10-04 (`FolderGrant.cfg`) | no error; 270,585 generated, 153,484 distinct, depth 8, 3 min 25 s |
| WebSigningBudget | citrate-core `src-tauri/formal/WebSigningBudget.tla` (5 cfgs), `81ef7a0` | recorded in `src-tauri/formal/README.md` | no error on all 5 cfgs (7,828,872 distinct on the base cfg); 23 of 23 mutants caught. A 2026-10-04 re-run was started and stopped unfinished at a load average near 30 |
| SkillPersistence | citrate-agent-runtime `agent-learn/formal/SkillPersistence.tla`, main `01a32ed` | recorded in `agent-learn/formal/README.md` | no error, 480,480 distinct |
| DeployGate | citrate-core `src-tauri/formal/DeployGate.tla` (+ `DeployGate_Reach.cfg`), `81ef7a0` | recorded in `EVIDENCE-n6-hellomint-e2e.md` (S6 sprint) | no error, 3,378 distinct; Reach config violates NeverSigns as intended |
| SpendBudget | citrate-core `src-tauri/formal/SpendBudget.tla` (+ `SpendBudget_TwoEndpoints.cfg`), `81ef7a0` | recorded in `src-tauri/formal/README.md` and `SpendBudget_mutants.py` | no error |
| AnchorBatch | citrate-agent-runtime `agent-anchor/formal/AnchorBatch.tla`, main `01a32ed` | recorded in `agent-anchor/formal/README.md` | no error, 5,813 distinct |
| DeviceLink | citrate-cluster `formal/DeviceLink.tla`, main `35a4caf` | re-run in fan-out 6 (RT-other lane) | no error, 21,456 distinct; 6 mutants each caught |

Reproduce: `scripts/run-tlc.sh <Spec> [cfg|all]` in citrate-core; in the other repos run
`java -cp ~/.tla/tla2tools.jar tlc2.TLC <Spec>.tla -config <Spec>.cfg` from the `formal/` folder.

Not claimed: bounds are small; these check designs, not code. Full proofs close with their WPs.
