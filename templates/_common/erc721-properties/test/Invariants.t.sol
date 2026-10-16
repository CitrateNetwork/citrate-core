// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc721" / "erc721-solady" templates.
//
// Runs the Medusa property harness under Foundry's invariant fuzzer, so the
// template invariants are checked even where Medusa is not installed.
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {{{ct:contract}}Properties} from "./Properties.sol";

contract {{ct:contract}}InvariantTest is Test {
    {{ct:contract}}Properties internal props;

    function setUp() public {
        props = new {{ct:contract}}Properties();
        bytes4[] memory selectors = new bytes4[](6);
        selectors[0] = props.mint.selector;
        selectors[1] = props.mintWithWrongValue.selector;
        selectors[2] = props.mintPastTheCap.selector;
        selectors[3] = props.withdrawAsOwner.selector;
        selectors[4] = props.withdrawAsStranger.selector;
        selectors[5] = props.transferOwnershipAsStranger.selector;
        targetSelector(FuzzSelector({addr: address(props), selectors: selectors}));
        targetContract(address(props));
    }

    function invariant_supply_never_exceeds_cap() public view {
        assertTrue(props.property_supply_never_exceeds_cap());
    }

    function invariant_payments_are_accounted() public view {
        assertTrue(props.property_payments_are_accounted());
    }

    function invariant_minted_tokens_are_held() public view {
        assertTrue(props.property_minted_tokens_are_held());
    }

    function invariant_wrong_payment_never_accepted() public view {
        assertTrue(props.property_wrong_payment_never_accepted());
    }

    function invariant_only_owner_withdraws() public view {
        assertTrue(props.property_only_owner_withdraws());
    }

    function invariant_owner_is_stable() public view {
        assertTrue(props.property_owner_is_stable());
    }

    function invariant_price_is_fixed() public view {
        assertTrue(props.property_price_is_fixed());
    }
}
