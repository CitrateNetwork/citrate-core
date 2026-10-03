---
created: 2026-10-03T00:00:00-07:00
branch: test/windows-hermes-refusal-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-HERMES-REFUSAL-FIXTURE
---

# Windows Hermes refusal fixture portability

## Authority and tracking

- Issue: `CitrateNetwork/citrate-core#186`.
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`.
- Operating direction: tactical Windows bug discovery and bounded fixes under normal
  repository governance.

## Reproduction at `main` (`b8d26e84cb12954d027b35bfadf5f63659e784ef`)

Two consecutive `cargo test --lib` runs failed
`hermes::brief_tests::the_ureq_transport_keeps_the_body_of_a_refusal` with Windows
socket errors 10053 and 10054. The exact test and all 11 Hermes brief tests pass when
run without full-suite load.

The one-shot test server reads an arbitrary socket chunk and closes after writing its
response without proving it consumed the POST body. Windows may reset a TCP connection
closed with unread request data.

## Scope

1. Make the local POST fixture consume the complete advertised request body.
2. Preserve the real `UreqControl` assertion for status 422 and the refusal body.
3. Leave production Hermes transport behavior unchanged.
4. Preserve the test count.

## Acceptance criteria

- The targeted POST refusal test passes on Windows.
- All Hermes brief tests pass on Windows.
- The test remains green under the full parallel library suite.
- Formatting and Clippy pass.
