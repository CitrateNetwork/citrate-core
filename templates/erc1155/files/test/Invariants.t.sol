// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc1155" template.
//
// Runs the Medusa property harness under Foundry's invariant fuzzer.
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {{{ct:contract}}Properties} from "./Properties.sol";

contract {{ct:contract}}InvariantTest is Test {
    {{ct:contract}}Properties internal props;

    function setUp() public {
        props = new {{ct:contract}}Properties();
        bytes4[] memory selectors = new bytes4[](5);
        selectors[0] = props.mint.selector;
        selectors[1] = props.mintPastTheCap.selector;
        selectors[2] = props.mintWithWrongValue.selector;
        selectors[3] = props.withdrawAsOwner.selector;
        selectors[4] = props.withdrawAsStranger.selector;
        targetSelector(FuzzSelector({addr: address(props), selectors: selectors}));
        targetContract(address(props));
    }

    function invariant_per_id_supply_never_exceeds_cap() public view {
        assertTrue(props.property_per_id_supply_never_exceeds_cap());
    }

    function invariant_payments_are_accounted() public view {
        assertTrue(props.property_payments_are_accounted());
    }

    function invariant_wrong_payment_never_accepted() public view {
        assertTrue(props.property_wrong_payment_never_accepted());
    }

    function invariant_only_owner_withdraws() public view {
        assertTrue(props.property_only_owner_withdraws());
    }
}
