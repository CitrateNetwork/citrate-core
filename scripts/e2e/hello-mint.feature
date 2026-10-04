# ---
# created: 2026-10-01T00:00:00Z
# branch: hup/n5-hellomint-e2e
# author: Larry Klosowski + Claude Opus 5.5
# status: active
# wp: HUP-S6 g3-gate, g3-e2e (local half), HUP-S11.1 (macOS part)
# ---
#
# US-6.1 (planset 04_FEATURES_BDD) as the local end-to-end run. Executed by
# scripts/e2e-hello-mint.sh; the Rust steps are
# src-tauri/src/hello_mint_e2e_tests.rs::e2e_hello_mint_on_an_anvil_fork.
#
# What differs from the planset scenario, on purpose:
# - the chain is a local anvil fork of 40204, never 40204 itself (agents never sign or send
#   on 40204; the 40204 run is a person on a T1 machine with the packaged app);
# - the member's wallet is a vault opened in-process with anvil's public development mnemonic,
#   and the approval is the ceremony's own approve_and_broadcast call (no approval UI);
# - "verified" is a local check (deployed runtime code equals the compiled artifact, and the
#   constructor parameters read back). CitrateScan cannot verify a contract on a local fork;
# - the faucet top-up step is not exercised (faucet_request is not built; ADR proposed);
# - the site is not built or pinned here (scripts/e2e-postdeploy-reader.sh covers the pin path
#   against a local kubo) and the Browser pop-out is not driven.

Feature: hello mint on a local fork of 40204

  Background:
    Given forge, anvil, cast, slither and jq are installed
    And the verifier configuration is "default" or the test-only "test-full"

  Scenario: Dev builds and deploys an NFT mint from a prompt (READY path, test-full verifiers)
    When the Dev says "help me make an NFT project called Lemon Drops, 500 supply, 5 SALT each"
    Then the sidecar's full-project track asks at most 5 interview questions, each with a default
    And the brief names the hello-mint workflow and the D-4 gates
    When the brief is accepted
    Then the hello-mint template renders an ERC-721 contract and a vite+wagmi app
    And forge test, slither, aderyn and medusa run on the contract
    And a dry run deploys the exact init code on an anvil fork of 40204
    And the deploy gate verdict is READY because every item passes
    When the Dev deploys
    Then a SignatureCeremony opens for a contract creation bound to the gated init-code hash
    And after approval the vault wallet signs and the contract is deployed to the fork
    And the deployed runtime code equals the compiled artifact and the parameters read back
    And a test mint through a second ceremony mints token 1 to the Dev for the set price
    And a Vercel export folder is produced from the project

  Scenario: A machine without aderyn and medusa (default verifiers)
    When the same run uses the default verifier configuration
    Then the verdict is NOT READY and the only failing items are Aderyn and Medusa, "not installed"
    And the deploy is refused naming them, and no ceremony is opened

  Scenario: An injected bug yields NOT READY (US-6.1 AC2, US-6.2)
    Given the rendered contract with its supply-cap check removed (an unbounded mint)
    When the same verifiers and dry run run on it
    Then the verdict is NOT READY with the failing forge test named
    And with the test-full verifiers the Medusa supply invariant fails too
    And the deploy is refused naming the failing items, and no ceremony is opened
