// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc1155" template.
//
// Medusa property harness (medusa.json targets this contract). The harness mints
// ids 0..3 only, so the per-id cap is checked over that set.
pragma solidity ^0.8.24;

import {{{ct:contract}}} from "../src/Token.sol";
import {HEVM} from "./utils/Hevm.sol";

contract {{ct:contract}}Properties {
    address internal constant OWNER = {{ct:owner}};
    uint256 internal constant IDS = 4;
    {{ct:contract}} internal token;

    uint256 internal paid;
    uint256 internal withdrawn;
    bool internal wrongPaymentAccepted;
    bool internal capOverrunAccepted;
    bool internal strangerWithdrew;

    constructor() {
        token = new {{ct:contract}}();
    }

    // ---- handlers ----

    function mint(uint256 id, uint256 amount) external {
        id = id % IDS;
        uint256 remaining = token.MAX_SUPPLY_PER_ID() - token.totalSupply(id);
        if (remaining == 0) return;
        amount = 1 + (amount % token.MAX_PER_TX());
        if (amount > remaining) amount = remaining;
        uint256 cost = token.PRICE() * amount;
        HEVM.deal(address(this), address(this).balance + cost);
        token.mint{value: cost}(id, amount);
        paid += cost;
    }

    function mintPastTheCap(uint256 id) external {
        id = id % IDS;
        uint256 remaining = token.MAX_SUPPLY_PER_ID() - token.totalSupply(id);
        if (remaining >= token.MAX_PER_TX()) return;
        uint256 amount = remaining + 1;
        uint256 cost = token.PRICE() * amount;
        HEVM.deal(address(this), address(this).balance + cost);
        try token.mint{value: cost}(id, amount) {
            capOverrunAccepted = true;
        } catch {}
    }

    function mintWithWrongValue(uint256 id, uint256 amount, uint256 extra) external {
        amount = 1 + (amount % token.MAX_PER_TX());
        uint256 value = token.PRICE() * amount + 1 + (extra % 1 ether);
        HEVM.deal(address(this), address(this).balance + value);
        try token.mint{value: value}(id % IDS, amount) {
            wrongPaymentAccepted = true;
        } catch {}
    }

    function withdrawAsOwner() external {
        uint256 before = OWNER.balance;
        HEVM.prank(OWNER);
        token.withdraw();
        withdrawn += OWNER.balance - before;
    }

    function withdrawAsStranger() external {
        try token.withdraw() {
            strangerWithdrew = true;
        } catch {}
    }

    function onERC1155Received(address, address, uint256, uint256, bytes calldata) external pure returns (bytes4) {
        return this.onERC1155Received.selector;
    }

    // ---- properties ----

    function property_per_id_supply_never_exceeds_cap() public view returns (bool) {
        if (capOverrunAccepted) return false;
        for (uint256 id; id < IDS; ++id) {
            if (token.totalSupply(id) > token.MAX_SUPPLY_PER_ID()) return false;
        }
        return true;
    }

    function property_payments_are_accounted() public view returns (bool) {
        return address(token).balance == paid - withdrawn;
    }

    function property_wrong_payment_never_accepted() public view returns (bool) {
        return !wrongPaymentAccepted;
    }

    function property_only_owner_withdraws() public view returns (bool) {
        return !strangerWithdrew && token.owner() == OWNER;
    }
}
