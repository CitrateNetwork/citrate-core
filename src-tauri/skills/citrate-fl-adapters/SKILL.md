---
name: citrate-fl-adapters
description: How LoRA adapters reach the member's local model: the eval gate, the checks before it, what ACCEPT and REJECT mean, and what core refuses. Use when a member asks whether an adapter is safe to load, why one was rejected, or how to load or unload one.
license: Apache-2.0
metadata:
  created: 2026-11-16
  branch: feat/054-hermes-hd-fl-skills
  author: Larry Klosowski + Claude Opus 5.5
  status: active
  wp: Hermes x FL WP-H11
  reserved: citrate-fl names are reserved; skill_write and learned skills cannot shadow them
  citrate-core: f29eedcee183cc56cadbbca92e11518a09b8a000
  citrate-chain: b2e91167bda2b81d52658c58f6327063a25cbedc
---

# Adapters and the eval gate

A LoRA adapter from a round changes how the member's local model answers, so core loads one only
after the eval gate accepted it on the base model being served
(`citrate-core:src-tauri/src/fl_rounds.rs#authorize_load`). Hermes cannot run the gate or override it.

## The gate

The member picks the adapter file and the base and candidate scorecards on Rounds > Adapters, and
core runs the gate (`citrate-core:src-tauri/src/fl_rounds.rs#fl_adapter_gate`). Before it, core
checks the file's SHA-256 and, when the member gave the round result file, that the round says
Accepted, that at least three devices took part, that the digests agree and that the replay found
no mismatch.

- ACCEPT means the candidate did at least as well on the shared metrics as the base.
- REJECT names the metric that got worse. Report it; never call a rejected adapter safe.
- Scorecards that measured different models are not comparable.

## Loading and unloading

Loading is the member's decision on a card, and core re-checks the gate and re-hashes the file.
Unloading only removes the adapter, so it needs no card. Read the records before you describe an
adapter; never invent a verdict or a score.

Downloading adapters from the network is not in this build.
