// SPDX-License-Identifier: MIT
// Generated from the Citrate "governor" template (OpenZeppelin Contracts v5.7.0).
pragma solidity ^0.8.24;

import {Governor} from "@openzeppelin/contracts/governance/Governor.sol";
import {GovernorSettings} from "@openzeppelin/contracts/governance/extensions/GovernorSettings.sol";
import {GovernorCountingSimple} from "@openzeppelin/contracts/governance/extensions/GovernorCountingSimple.sol";
import {GovernorVotes} from "@openzeppelin/contracts/governance/extensions/GovernorVotes.sol";
import {GovernorVotesQuorumFraction} from
    "@openzeppelin/contracts/governance/extensions/GovernorVotesQuorumFraction.sol";
import {IVotes} from "@openzeppelin/contracts/governance/utils/IVotes.sol";

/// @title {{ct:name}} Governor
/// @notice Voting delay 1 day, voting period 1 week (timestamp clock), proposal
/// threshold 0, quorum 4% of the supply at the proposal snapshot.
contract {{ct:contract}}Governor is
    Governor,
    GovernorSettings,
    GovernorCountingSimple,
    GovernorVotes,
    GovernorVotesQuorumFraction
{
    constructor(IVotes votesToken)
        Governor("{{ct:name}} Governor")
        GovernorSettings(1 days, 1 weeks, 0)
        GovernorVotes(votesToken)
        GovernorVotesQuorumFraction(4)
    {}

    function proposalThreshold() public view override(Governor, GovernorSettings) returns (uint256) {
        return super.proposalThreshold();
    }
}
