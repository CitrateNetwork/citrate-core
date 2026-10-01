// SPDX-License-Identifier: MIT
// Generated from the Citrate "governor" template (OpenZeppelin Contracts v5.7.0).
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {ERC20Permit} from "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import {ERC20Votes} from "@openzeppelin/contracts/token/ERC20/extensions/ERC20Votes.sol";
import {Nonces} from "@openzeppelin/contracts/utils/Nonces.sol";

/// @title {{ct:name}}
/// @notice Fixed-supply votes token. Holders must delegate (to themselves or
/// another address) before their balance counts as votes.
contract {{ct:contract}} is ERC20, ERC20Permit, ERC20Votes {
    uint256 public constant INITIAL_SUPPLY = {{ct:supply}} * 10 ** 18;
    address public constant INITIAL_HOLDER = {{ct:owner}};

    constructor() ERC20("{{ct:name}}", "{{ct:symbol}}") ERC20Permit("{{ct:name}}") {
        _mint(INITIAL_HOLDER, INITIAL_SUPPLY);
    }

    /// Timestamp clock (EIP-6372): governance timings are in seconds.
    function clock() public view override returns (uint48) {
        return uint48(block.timestamp);
    }

    function CLOCK_MODE() public pure override returns (string memory) {
        return "mode=timestamp";
    }

    function _update(address from, address to, uint256 value) internal override(ERC20, ERC20Votes) {
        super._update(from, to, value);
    }

    function nonces(address owner) public view override(ERC20Permit, Nonces) returns (uint256) {
        return super.nonces(owner);
    }
}
