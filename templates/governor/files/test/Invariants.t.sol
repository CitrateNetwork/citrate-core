// SPDX-License-Identifier: MIT
// Generated from the Citrate "governor" template.
//
// Runs the Medusa property harness under Foundry's invariant fuzzer.
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {{{ct:contract}}Properties} from "./Properties.sol";

contract {{ct:contract}}InvariantTest is Test {
    {{ct:contract}}Properties internal props;

    function setUp() public {
        props = new {{ct:contract}}Properties();
        bytes4[] memory selectors = new bytes4[](3);
        selectors[0] = props.transfer.selector;
        selectors[1] = props.delegate.selector;
        selectors[2] = props.advance.selector;
        targetSelector(FuzzSelector({addr: address(props), selectors: selectors}));
        targetContract(address(props));
    }

    function invariant_supply_is_fixed() public view {
        assertTrue(props.property_supply_is_fixed());
    }

    function invariant_votes_never_exceed_supply() public view {
        assertTrue(props.property_votes_never_exceed_supply());
    }

    function invariant_settings_are_fixed() public view {
        assertTrue(props.property_settings_are_fixed());
    }

    function invariant_clock_is_timestamp() public view {
        assertTrue(props.property_clock_is_timestamp());
    }
}
