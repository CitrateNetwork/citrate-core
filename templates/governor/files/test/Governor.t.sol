// SPDX-License-Identifier: MIT
// Generated from the Citrate "governor" template.
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {IGovernor} from "@openzeppelin/contracts/governance/IGovernor.sol";
import {{{ct:contract}}} from "../src/Token.sol";
import {{{ct:contract}}Governor} from "../src/Dao.sol";

contract {{ct:contract}}GovernorTest is Test {
    address internal constant HOLDER = {{ct:owner}};
    {{ct:contract}} internal token;
    {{ct:contract}}Governor internal governor;
    address internal alice = makeAddr("alice");

    function setUp() public {
        token = new {{ct:contract}}();
        governor = new {{ct:contract}}Governor(token);
    }

    function test_parameters_are_baked_in() public view {
        assertEq(token.name(), "{{ct:name}}");
        assertEq(token.symbol(), "{{ct:symbol}}");
        assertEq(token.totalSupply(), uint256({{ct:supply}}) * 1e18);
        assertEq(token.balanceOf(HOLDER), token.totalSupply());
        assertEq(governor.name(), "{{ct:name}} Governor");
        assertEq(token.CLOCK_MODE(), "mode=timestamp");
        assertEq(governor.CLOCK_MODE(), "mode=timestamp");
        assertEq(governor.votingDelay(), 1 days);
        assertEq(governor.votingPeriod(), 1 weeks);
    }

    function test_balance_counts_only_after_delegation() public {
        assertEq(token.getVotes(HOLDER), 0);
        vm.prank(HOLDER);
        token.delegate(HOLDER);
        assertEq(token.getVotes(HOLDER), token.totalSupply());
    }

    /// Full lifecycle: propose a transfer of treasury tokens, vote, execute.
    function test_a_passed_proposal_executes() public {
        uint256 grant = 10e18 > token.totalSupply() / 2 ? token.totalSupply() / 2 : 10e18;
        vm.startPrank(HOLDER);
        token.delegate(HOLDER);
        assertTrue(token.transfer(address(governor), grant));
        vm.stopPrank();
        vm.warp(block.timestamp + 1);

        address[] memory targets = new address[](1);
        uint256[] memory values = new uint256[](1);
        bytes[] memory calldatas = new bytes[](1);
        targets[0] = address(token);
        calldatas[0] = abi.encodeCall(token.transfer, (alice, grant));
        string memory description = "Grant to alice";

        vm.prank(HOLDER);
        uint256 id = governor.propose(targets, values, calldatas, description);
        assertEq(uint256(governor.state(id)), uint256(IGovernor.ProposalState.Pending));

        vm.warp(block.timestamp + governor.votingDelay() + 1);
        assertEq(uint256(governor.state(id)), uint256(IGovernor.ProposalState.Active));
        vm.prank(HOLDER);
        governor.castVote(id, 1);

        vm.warp(block.timestamp + governor.votingPeriod() + 1);
        assertEq(uint256(governor.state(id)), uint256(IGovernor.ProposalState.Succeeded));

        governor.execute(targets, values, calldatas, keccak256(bytes(description)));
        assertEq(token.balanceOf(alice), grant);
        assertEq(uint256(governor.state(id)), uint256(IGovernor.ProposalState.Executed));
    }

    function test_a_proposal_without_quorum_is_defeated() public {
        vm.prank(HOLDER);
        token.delegate(HOLDER);
        vm.warp(block.timestamp + 1);
        address[] memory targets = new address[](1);
        uint256[] memory values = new uint256[](1);
        bytes[] memory calldatas = new bytes[](1);
        targets[0] = address(token);
        calldatas[0] = abi.encodeCall(token.transfer, (alice, 0));
        vm.prank(HOLDER);
        uint256 id = governor.propose(targets, values, calldatas, "No votes");
        vm.warp(block.timestamp + governor.votingDelay() + governor.votingPeriod() + 2);
        assertEq(uint256(governor.state(id)), uint256(IGovernor.ProposalState.Defeated));
    }
}
