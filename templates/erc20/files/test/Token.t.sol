// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc20" template.
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {IERC20Errors} from "@openzeppelin/contracts/interfaces/draft-IERC6093.sol";
import {{{ct:contract}}} from "../src/Token.sol";

contract {{ct:contract}}Test is Test {
    address internal constant HOLDER = {{ct:owner}};
    {{ct:contract}} internal token;
    address internal alice = makeAddr("alice");

    function setUp() public {
        token = new {{ct:contract}}();
    }

    function test_parameters_are_baked_in() public view {
        assertEq(token.name(), "{{ct:name}}");
        assertEq(token.symbol(), "{{ct:symbol}}");
        assertEq(token.decimals(), 18);
        assertEq(token.totalSupply(), uint256({{ct:supply}}) * 1e18);
        assertEq(token.balanceOf(HOLDER), token.totalSupply());
    }

    function test_transfer_moves_balance() public {
        vm.prank(HOLDER);
        assertTrue(token.transfer(alice, 1e18));
        assertEq(token.balanceOf(alice), 1e18);
        assertEq(token.balanceOf(HOLDER), token.totalSupply() - 1e18);
    }

    function test_transfer_over_balance_reverts() public {
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(IERC20Errors.ERC20InsufficientBalance.selector, alice, 0, 1));
        // forge-lint: disable-next-line(erc20-unchecked-transfer)
        token.transfer(HOLDER, 1);
    }

    function test_permit_sets_allowance_from_a_signature() public {
        (address signer, uint256 key) = makeAddrAndKey("signer");
        vm.prank(HOLDER);
        assertTrue(token.transfer(signer, 5e18));
        uint256 deadline = block.timestamp + 1 hours;
        bytes32 structHash = keccak256(
            abi.encode(
                keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)"),
                signer,
                alice,
                5e18,
                token.nonces(signer),
                deadline
            )
        );
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", token.DOMAIN_SEPARATOR(), structHash));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, digest);
        token.permit(signer, alice, 5e18, deadline, v, r, s);
        assertEq(token.allowance(signer, alice), 5e18);
        vm.prank(alice);
        assertTrue(token.transferFrom(signer, alice, 5e18));
        assertEq(token.balanceOf(alice), 5e18);
    }

    function testFuzz_supply_is_conserved_by_transfers(uint256 amount) public {
        amount = bound(amount, 0, token.balanceOf(HOLDER));
        vm.prank(HOLDER);
        assertTrue(token.transfer(alice, amount));
        assertEq(token.balanceOf(alice) + token.balanceOf(HOLDER), token.totalSupply());
    }
}
