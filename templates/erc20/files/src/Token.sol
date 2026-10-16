// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc20" template (OpenZeppelin Contracts v5.7.0).
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {ERC20Permit} from "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";

/// @title {{ct:name}}
/// @notice Fixed supply: {{ct:supply}} whole tokens minted once to the initial holder.
contract {{ct:contract}} is ERC20, ERC20Permit {
    uint256 public constant INITIAL_SUPPLY = {{ct:supply}} * 10 ** 18;
    address public constant INITIAL_HOLDER = {{ct:owner}};

    constructor() ERC20("{{ct:name}}", "{{ct:symbol}}") ERC20Permit("{{ct:name}}") {
        _mint(INITIAL_HOLDER, INITIAL_SUPPLY);
    }
}
