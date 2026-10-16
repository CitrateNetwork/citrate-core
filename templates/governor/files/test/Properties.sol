// SPDX-License-Identifier: MIT
// Generated from the Citrate "governor" template.
//
// Medusa property harness (medusa.json targets this contract). Tokens and votes
// only move between the four actors; voting power can never exceed the supply.
pragma solidity ^0.8.24;

import {{{ct:contract}}} from "../src/Token.sol";
import {{{ct:contract}}Governor} from "../src/Dao.sol";
import {HEVM} from "./utils/Hevm.sol";

contract {{ct:contract}}Properties {
    {{ct:contract}} internal token;
    {{ct:contract}}Governor internal governor;
    address[4] internal actors;

    constructor() {
        token = new {{ct:contract}}();
        governor = new {{ct:contract}}Governor(token);
        actors = [token.INITIAL_HOLDER(), address(0x10000), address(0x20000), address(0x30000)];
    }

    // ---- handlers ----

    function transfer(uint8 from, uint8 to, uint256 amount) external {
        address a = actors[from % 4];
        uint256 balance = token.balanceOf(a);
        if (balance == 0) return;
        HEVM.prank(a);
        require(token.transfer(actors[to % 4], amount % (balance + 1)));
    }

    function delegate(uint8 from, uint8 to) external {
        HEVM.prank(actors[from % 4]);
        token.delegate(actors[to % 4]);
    }

    function advance(uint32 secondsAhead) external {
        HEVM.warp(block.timestamp + (secondsAhead % 30 days) + 1);
    }

    // ---- properties ----

    function property_supply_is_fixed() public view returns (bool) {
        return token.totalSupply() == token.INITIAL_SUPPLY();
    }

    function property_votes_never_exceed_supply() public view returns (bool) {
        uint256 total;
        for (uint256 i; i < actors.length; ++i) {
            if (i > 0 && actors[i] == actors[0]) continue;
            total += token.getVotes(actors[i]);
        }
        return total <= token.totalSupply();
    }

    function property_settings_are_fixed() public view returns (bool) {
        return governor.votingDelay() == 1 days && governor.votingPeriod() == 1 weeks
            && governor.proposalThreshold() == 0 && governor.quorumNumerator() == 4;
    }

    function property_clock_is_timestamp() public view returns (bool) {
        return token.clock() == block.timestamp && governor.clock() == block.timestamp;
    }
}
