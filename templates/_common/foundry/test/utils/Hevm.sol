// SPDX-License-Identifier: MIT
// Generated from a Citrate template.
pragma solidity ^0.8.24;

/// The cheatcode subset that both Medusa and Foundry implement at the standard
/// cheatcode address. The property harnesses use only these, so the same harness
/// runs under `medusa fuzz` and under `forge test` (as invariant targets).
interface IHevm {
    function deal(address who, uint256 amount) external;
    function prank(address sender) external;
    function warp(uint256 timestamp) external;
    function roll(uint256 blockNumber) external;
}

IHevm constant HEVM = IHevm(0x7109709ECfa91a80626fF3989D68f67F5b1DD12D);
