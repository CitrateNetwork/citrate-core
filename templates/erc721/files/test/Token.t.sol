// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc721" template.
pragma solidity ^0.8.24;

import {Test, stdStorage, StdStorage} from "forge-std/Test.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {{{ct:contract}}} from "../src/Token.sol";

/// Tries to mint again from inside the ERC-721 receive callback.
contract ReentrantMinter {
    {{ct:contract}} internal token;

    constructor({{ct:contract}} token_) {
        token = token_;
    }

    function attack() external payable {
        token.mint{value: token.PRICE()}(1);
    }

    function onERC721Received(address, address, uint256, bytes calldata) external returns (bytes4) {
        token.mint{value: token.PRICE()}(1);
        return this.onERC721Received.selector;
    }
}

contract {{ct:contract}}Test is Test {
    using stdStorage for StdStorage;

    address internal constant OWNER = {{ct:owner}};
    {{ct:contract}} internal token;
    address internal alice = makeAddr("alice");
    address internal mallory = makeAddr("mallory");

    function setUp() public {
        token = new {{ct:contract}}();
    }

    function _mint(address to, uint256 quantity) internal {
        uint256 cost = token.PRICE() * quantity;
        vm.deal(to, cost);
        vm.prank(to);
        token.mint{value: cost}(quantity);
    }

    function test_parameters_are_baked_in() public view {
        assertEq(token.name(), "{{ct:name}}");
        assertEq(token.symbol(), "{{ct:symbol}}");
        assertEq(token.MAX_SUPPLY(), {{ct:supply}});
        assertEq(token.PRICE(), {{ct:price}});
        assertEq(token.owner(), OWNER);
        assertEq(token.totalMinted(), 0);
    }

    function test_mint_assigns_sequential_ids_from_one() public {
        uint256 quantity = token.MAX_SUPPLY() < 3 ? token.MAX_SUPPLY() : 3;
        _mint(alice, quantity);
        assertEq(token.balanceOf(alice), quantity);
        assertEq(token.totalMinted(), quantity);
        for (uint256 id = 1; id <= quantity; ++id) {
            assertEq(token.ownerOf(id), alice);
        }
        assertEq(address(token).balance, token.PRICE() * quantity);
    }

    function test_mint_rejects_a_wrong_payment() public {
        uint256 expected = token.PRICE();
        vm.deal(alice, expected + 1);
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.WrongPayment.selector, expected + 1, expected));
        token.mint{value: expected + 1}(1);
    }

    function test_mint_rejects_zero_and_oversized_quantities() public {
        vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.BadQuantity.selector, 0));
        token.mint(0);
        uint256 tooMany = token.MAX_PER_TX() + 1;
        vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.BadQuantity.selector, tooMany));
        token.mint(tooMany);
    }

    function test_mint_stops_at_the_cap() public {
        stdstore.target(address(token)).sig("totalMinted()").checked_write(token.MAX_SUPPLY() - 1);
        _mint(alice, 1);
        assertEq(token.ownerOf(token.MAX_SUPPLY()), alice);
        uint256 cost = token.PRICE();
        vm.deal(alice, cost);
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector({{ct:contract}}.SoldOut.selector, 1, 0));
        token.mint{value: cost}(1);
    }

    function test_reentrant_mint_from_the_receiver_reverts() public {
        ReentrantMinter attacker = new ReentrantMinter(token);
        uint256 price = token.PRICE();
        vm.deal(address(attacker), price * 2);
        vm.deal(address(this), price);
        // `price` is read first: a call inside the expression would be the one
        // vm.expectRevert checks.
        vm.expectRevert(ReentrancyGuard.ReentrancyGuardReentrantCall.selector);
        attacker.attack{value: price}();
        assertEq(token.totalMinted(), 0);
    }

    function test_only_the_owner_withdraws_the_balance() public {
        _mint(alice, 1);
        uint256 amount = address(token).balance;
        vm.prank(mallory);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, mallory));
        token.withdraw();
        uint256 before = OWNER.balance;
        vm.prank(OWNER);
        token.withdraw();
        assertEq(OWNER.balance, before + amount);
        assertEq(address(token).balance, 0);
    }

    function test_only_the_owner_sets_the_base_uri() public {
        _mint(alice, 1);
        assertEq(token.tokenURI(1), "");
        vm.prank(mallory);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, mallory));
        token.setBaseURI("ipfs://evil/");
        vm.prank(OWNER);
        token.setBaseURI("ipfs://collection/");
        assertEq(token.tokenURI(1), "ipfs://collection/1");
    }

    function testFuzz_mint_charges_exactly_price_times_quantity(uint8 raw) public {
        uint256 cap = token.MAX_SUPPLY() < token.MAX_PER_TX() ? token.MAX_SUPPLY() : token.MAX_PER_TX();
        uint256 quantity = 1 + (uint256(raw) % cap);
        _mint(alice, quantity);
        assertEq(address(token).balance, token.PRICE() * quantity);
        assertEq(token.balanceOf(alice), quantity);
    }
}
