// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc20" template.
//
// Medusa property harness (medusa.json targets this contract). Tokens only move
// between the four actors, so their balances must always sum to the supply.
pragma solidity ^0.8.24;

import {{{ct:contract}}} from "../src/Token.sol";
import {HEVM} from "./utils/Hevm.sol";

contract {{ct:contract}}Properties {
    {{ct:contract}} internal token;
    address[4] internal actors;
    bool internal overdraftAccepted;

    constructor() {
        token = new {{ct:contract}}();
        actors = [token.INITIAL_HOLDER(), address(0x10000), address(0x20000), address(0x30000)];
    }

    // ---- handlers ----

    function transfer(uint8 from, uint8 to, uint256 amount) external {
        address a = actors[from % 4];
        address b = actors[to % 4];
        uint256 balance = token.balanceOf(a);
        if (balance == 0) return;
        HEVM.prank(a);
        require(token.transfer(b, amount % (balance + 1)));
    }

    function approveAndTransferFrom(uint8 owner, uint8 spender, uint8 to, uint256 amount) external {
        address o = actors[owner % 4];
        address s = actors[spender % 4];
        address t = actors[to % 4];
        uint256 balance = token.balanceOf(o);
        amount = amount % (balance + 1);
        HEVM.prank(o);
        token.approve(s, amount);
        HEVM.prank(s);
        require(token.transferFrom(o, t, amount));
    }

    function transferMoreThanBalance(uint8 from, uint8 to) external {
        address a = actors[from % 4];
        uint256 balance = token.balanceOf(a);
        HEVM.prank(a);
        try token.transfer(actors[to % 4], balance + 1) {
            overdraftAccepted = true;
        } catch {}
    }

    // ---- properties ----

    function property_supply_is_fixed() public view returns (bool) {
        return token.totalSupply() == token.INITIAL_SUPPLY();
    }

    function property_balances_sum_to_supply() public view returns (bool) {
        uint256 sum;
        for (uint256 i; i < actors.length; ++i) {
            // The holder may coincide with a fuzzer sender; count it once.
            if (i > 0 && actors[i] == actors[0]) continue;
            sum += token.balanceOf(actors[i]);
        }
        return sum == token.totalSupply();
    }

    function property_no_overdraft() public view returns (bool) {
        return !overdraftAccepted;
    }
}
