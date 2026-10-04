# HUP-S6 US-6.1 (local half), gates g3-gate and g3-e2e (local), HUP-S11.1 (macOS part).
#
# Runs on one machine against a throwaway anvil chain with chain id 40204 and an anvil fork of
# it. Nothing touches the live chain 40204, and no real funds or keys are involved: the member
# wallet is a fresh test vault created for the run.
#
# Run it with scripts/e2e-hello-mint.sh. Every step below maps to exactly one handler in
# src-tauri/src/hello_mint_e2e_tests.rs; an unknown step fails the run (and a plain
# `cargo test` checks that every step is known).
#
# What this does not cover (see the script header): the Hermes interview turn itself (runtime
# interview tests), the Browser pop-out, the faucet (HUP-S6.5, not built: the local chain funds
# the wallet instead), CitrateScan verification (needs the live explorer), and a member clicking
# Approve in the packaged app (the test approves the same ceremony in-process).

Feature: hello mint, end to end on this machine

  Background:
    Given a local anvil chain with chain id 40204 and an anvil fork of it
    And a fresh member wallet in a test vault, funded on the local chain
    And the Dev answers the hello-mint interview with Lemon Drops, LEMON, 500 supply and 5 SALT each
    When the hello-mint template is rendered for tier T1 with the pinned dependencies
    Then the project has a vite + wagmi page and an ERC-721 contract

  Scenario: Without aderyn and medusa the gate is NOT READY and nothing can be deployed
    Given the verifier config of this machine, which has no aderyn or medusa
    When the deploy gate runs forge test, slither, aderyn, medusa and a fork dry run
    Then the verdict is NOT READY
    And the only failing items are Aderyn and Medusa campaign, each because the tool is not installed
    And contract_deploy refuses with the failing items and no ceremony is opened

  Scenario: The READY path deploys through the SignatureCeremony, verifies and mints
    Given the test-only verifier config that adds aderyn and medusa from the measured bundle archives
    When the deploy gate runs forge test, slither, aderyn, medusa and a fork dry run
    Then the verdict is READY
    When the member clicks Deploy
    Then a SignatureCeremony shows a contract creation whose bytecode hash matches the gated artifact
    When the member approves the ceremony
    Then the contract is deployed on the local chain
    And the deployed code matches the compiled artifact and the verifier input is produced
    And the site switches to the deployed contract
    And a test mint of 1 token at 5 SALT succeeds through a ceremony the member acknowledges as raw data
    And the Vercel export is written
    And the page builds and is pinned to IPFS when the page build is enabled

  Scenario: An injected unbounded mint is NOT READY and names the finding
    Given the test-only verifier config that adds aderyn and medusa from the measured bundle archives
    And the supply cap check is removed from the contract
    When the deploy gate runs forge test, slither, aderyn, medusa and a fork dry run
    Then the verdict is NOT READY
    And the Forge tests item fails naming test_mint_stops_at_the_cap
    And contract_deploy refuses with the failing items and no ceremony is opened
