---
created: 2026-09-11
author: Claude Fable 5
status: active
sprint: sprint-hermes-p3-headline
---

# Evidence — Hermes P3 (WP3.1 register half)

## Rule-1: calldata is byte-exact
Ground truth from foundry `cast` (the same tool the on-chain deploy script uses):
```
cast calldata 'registerModel(string,string,string,string,uint256,uint256,(string,string[],string[],uint256,string,string[]))' \
  gemma gguf 1.0 QmTest 1000 0 '(A model,[],[],0,MIT,[nlp,chat])'
```
- selector `0x49dc4e2f`.
- `register_model_calldata_matches_cast_byte_for_byte` asserts the Rust output equals the
  full `cast` hex.

## Rule-3: nothing signs
`models_registry_register` builds the `{from,to,value,data,gas,chainId}` tx JSON and calls
`ceremony.0.request(SignatureIntent{kind: Transaction})` — a PENDING ceremony the human
approves + broadcasts via the signing surface. Identical pattern to `storage_pin`.

## Tests (all green)
- Rust `cargo test --lib model_register`: 4 passed
  (`..._matches_cast_byte_for_byte`, `selector_is_the_live_register_model_selector`,
  `empty_tags_and_metadata_still_encode_...`, `the_registration_fee_is_a_tenth_of_a_salt`).
- Frontend `models.contract.test.ts`: 9 passed (adds `registry` + `register` to the frozen
  method set, and a tauri-invoke arg-shape test for `models_registry_register`).
- Full suites: `cargo test --lib` 403 passed / 5 ignored; `npx vitest run` 482 passed.

## Follow-ons (see SCOPE)
- CID acquisition via `ipfs add` on the pull path; fee funding; WP3.2 template bytecode.
