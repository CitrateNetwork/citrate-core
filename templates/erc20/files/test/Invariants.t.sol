// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc20" template.
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
        selectors[1] = props.approveAndTransferFrom.selector;
        selectors[2] = props.transferMoreThanBalance.selector;
        targetSelector(FuzzSelector({addr: address(props), selectors: selectors}));
        targetContract(address(props));
    }

    function invariant_supply_is_fixed() public view {
        assertTrue(props.property_supply_is_fixed());
    }

    function invariant_balances_sum_to_supply() public view {
        assertTrue(props.property_balances_sum_to_supply());
    }

    function invariant_no_overdraft() public view {
        assertTrue(props.property_no_overdraft());
    }
}
