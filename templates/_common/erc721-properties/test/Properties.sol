// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc721" / "erc721-solady" templates.
//
// Medusa property harness (medusa.json targets this contract). Handlers drive the
// token; `property_*` functions must always return true. `test/Invariants.t.sol`
// runs the same harness under `forge test`.
pragma solidity ^0.8.24;

import {{{ct:contract}}} from "../src/Token.sol";
import {HEVM} from "./utils/Hevm.sol";

contract {{ct:contract}}Properties {
    address internal constant OWNER = {{ct:owner}};
    {{ct:contract}} internal token;

    uint256 internal paid;
    uint256 internal withdrawn;
    bool internal wrongPaymentAccepted;
    bool internal capOverrunAccepted;
    bool internal strangerWithdrew;
    bool internal strangerTookOwnership;

    constructor() {
        token = new {{ct:contract}}();
    }

    // ---- handlers ----

    function mint(uint256 quantity) external {
        uint256 remaining = token.MAX_SUPPLY() - token.totalMinted();
        if (remaining == 0) return;
        quantity = 1 + (quantity % token.MAX_PER_TX());
        if (quantity > remaining) quantity = remaining;
        uint256 cost = token.PRICE() * quantity;
        HEVM.deal(address(this), address(this).balance + cost);
        token.mint{value: cost}(quantity);
        paid += cost;
    }

    function mintWithWrongValue(uint256 quantity, uint256 extra) external {
        quantity = 1 + (quantity % token.MAX_PER_TX());
        uint256 value = token.PRICE() * quantity + 1 + (extra % 1 ether);
        HEVM.deal(address(this), address(this).balance + value);
        try token.mint{value: value}(quantity) {
            wrongPaymentAccepted = true;
        } catch {}
    }

    function mintPastTheCap() external {
        uint256 remaining = token.MAX_SUPPLY() - token.totalMinted();
        if (remaining >= token.MAX_PER_TX()) return;
        uint256 quantity = remaining + 1;
        uint256 cost = token.PRICE() * quantity;
        HEVM.deal(address(this), address(this).balance + cost);
        try token.mint{value: cost}(quantity) {
            capOverrunAccepted = true;
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

    function transferOwnershipAsStranger(address to) external {
        try token.transferOwnership(to) {
            strangerTookOwnership = true;
        } catch {}
    }

    function onERC721Received(address, address, uint256, bytes calldata) external pure returns (bytes4) {
        return this.onERC721Received.selector;
    }

    // ---- properties ----

    function property_supply_never_exceeds_cap() public view returns (bool) {
        return !capOverrunAccepted && token.totalMinted() <= token.MAX_SUPPLY();
    }

    function property_payments_are_accounted() public view returns (bool) {
        return address(token).balance == paid - withdrawn;
    }

    function property_minted_tokens_are_held() public view returns (bool) {
        return token.balanceOf(address(this)) == token.totalMinted();
    }

    function property_wrong_payment_never_accepted() public view returns (bool) {
        return !wrongPaymentAccepted;
    }

    function property_only_owner_withdraws() public view returns (bool) {
        return !strangerWithdrew;
    }

    function property_owner_is_stable() public view returns (bool) {
        return !strangerTookOwnership && token.owner() == OWNER;
    }

    function property_price_is_fixed() public view returns (bool) {
        return token.PRICE() == {{ct:price}};
    }
}
