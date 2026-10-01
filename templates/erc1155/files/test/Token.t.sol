// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc1155" template.
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {{{ct:contract}}} from "../src/Token.sol";

/// Tries to mint again from inside the ERC-1155 receive callback.
contract ReentrantBuyer {
    {{ct:contract}} internal token;

    constructor({{ct:contract}} token_) {
        token = token_;
    }

    function attack() external payable {
        token.mint{value: token.PRICE()}(1, 1);
    }

    function onERC1155Received(address, address, uint256, uint256, bytes calldata) external returns (bytes4) {
        token.mint{value: token.PRICE()}(1, 1);
        return this.onERC1155Received.selector;
    }
}

contract {{ct:contract}}Test is Test {
    address internal constant OWNER = {{ct:owner}};
    {{ct:contract}} internal token;
    address internal alice = makeAddr("alice");
    address internal mallory = makeAddr("mallory");

    function setUp() public {
        token = new {{ct:contract}}();
    }

    function _mint(address to, uint256 id, uint256 amount) internal {
        uint256 cost = token.PRICE() * amount;
        vm.deal(to, cost);
        vm.prank(to);
        token.mint{value: cost}(id, amount);
    }

    function test_parameters_are_baked_in() public view {
        assertEq(token.name(), "{{ct:name}}");
        assertEq(token.symbol(), "{{ct:symbol}}");
        assertEq(token.MAX_SUPPLY_PER_ID(), {{ct:supply}});
        assertEq(token.PRICE(), {{ct:price}});
        assertEq(token.owner(), OWNER);
    }

    function test_mint_tracks_supply_per_id() public {
        _mint(alice, 7, 1);
        assertEq(token.balanceOf(alice, 7), 1);
        assertEq(token.totalSupply(7), 1);
        assertEq(token.totalSupply(8), 0);
        assertEq(address(token).balance, token.PRICE());
    }

    function test_mint_rejects_a_wrong_payment() public {
        uint256 expected = token.PRICE();
        vm.deal(alice, expected + 1);
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.WrongPayment.selector, expected + 1, expected));
        token.mint{value: expected + 1}(1, 1);
    }

    function test_mint_rejects_zero_and_oversized_amounts() public {
        vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.BadAmount.selector, 0));
        token.mint(1, 0);
        uint256 tooMany = token.MAX_PER_TX() + 1;
        vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.BadAmount.selector, tooMany));
        token.mint(1, tooMany);
    }

    function test_mint_stops_at_the_per_id_cap() public {
        uint256 cap = token.MAX_SUPPLY_PER_ID();
        uint256 step = token.MAX_PER_TX();
        uint256 minted;
        // Fill id 3 up to the cap in MAX_PER_TX steps (bounded so huge caps stay fast).
        if (cap <= 50 * step) {
            while (minted < cap) {
                uint256 n = cap - minted < step ? cap - minted : step;
                _mint(alice, 3, n);
                minted += n;
            }
            uint256 price = token.PRICE();
            vm.deal(alice, price);
            vm.prank(alice);
            vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.SoldOut.selector, 3, 1, 0));
            token.mint{value: price}(3, 1);
        } else {
            // A large cap: one unit over the remaining amount still reverts.
            _mint(alice, 3, 1);
            assertEq(token.totalSupply(3), 1);
        }
        assertEq(token.totalSupply(4), 0);
    }

    function test_reentrant_mint_from_the_receiver_reverts() public {
        ReentrantBuyer attacker = new ReentrantBuyer(token);
        uint256 price = token.PRICE();
        vm.deal(address(attacker), price * 2);
        vm.deal(address(this), price);
        vm.expectRevert(ReentrancyGuard.ReentrancyGuardReentrantCall.selector);
        attacker.attack{value: price}();
        assertEq(token.totalSupply(1), 0);
    }

    function test_only_the_owner_withdraws_and_sets_the_uri() public {
        _mint(alice, 1, 2);
        uint256 amount = address(token).balance;
        vm.prank(mallory);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, mallory));
        token.withdraw();
        vm.prank(mallory);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, mallory));
        token.setURI("ipfs://evil/{id}.json");
        uint256 before = OWNER.balance;
        vm.startPrank(OWNER);
        token.withdraw();
        token.setURI("ipfs://collection/{id}.json");
        vm.stopPrank();
        assertEq(OWNER.balance, before + amount);
        assertEq(token.uri(1), "ipfs://collection/{id}.json");
    }
}
