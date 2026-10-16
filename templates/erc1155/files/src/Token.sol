// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc1155" template (OpenZeppelin Contracts v5.7.0).
pragma solidity ^0.8.24;

import {ERC1155} from "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import {ERC1155Supply} from "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Supply.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @title {{ct:name}}
/// @notice Paid ERC-1155: any id, PRICE wei per unit, at most MAX_SUPPLY_PER_ID units per id.
contract {{ct:contract}} is ERC1155, ERC1155Supply, Ownable, ReentrancyGuard {
    uint256 public constant MAX_SUPPLY_PER_ID = {{ct:supply}};
    uint256 public constant PRICE = {{ct:price}};
    uint256 public constant MAX_PER_TX = 100;

    error BadAmount(uint256 amount);
    error SoldOut(uint256 id, uint256 requested, uint256 remaining);
    error WrongPayment(uint256 sent, uint256 expected);
    error WithdrawFailed();

    event Withdrawn(address indexed to, uint256 amount);

    constructor() ERC1155("") Ownable({{ct:owner}}) {}

    /// Collection name (not part of ERC-1155; read by wallets and marketplaces).
    function name() external pure returns (string memory) {
        return "{{ct:name}}";
    }

    /// Collection symbol (not part of ERC-1155; read by wallets and marketplaces).
    function symbol() external pure returns (string memory) {
        return "{{ct:symbol}}";
    }

    /// Mint `amount` units of `id` to the caller for exactly PRICE * amount wei.
    function mint(uint256 id, uint256 amount) external payable nonReentrant {
        if (amount == 0 || amount > MAX_PER_TX) revert BadAmount(amount);
        uint256 remaining = MAX_SUPPLY_PER_ID - totalSupply(id);
        if (amount > remaining) revert SoldOut(id, amount, remaining);
        uint256 expected = PRICE * amount;
        if (msg.value != expected) revert WrongPayment(msg.value, expected);
        _mint(msg.sender, id, amount, "");
    }

    function setURI(string calldata newURI) external onlyOwner {
        _setURI(newURI);
    }

    /// Send the whole mint balance to the owner.
    function withdraw() external onlyOwner nonReentrant {
        address to = owner();
        uint256 amount = address(this).balance;
        (bool ok,) = to.call{value: amount}("");
        if (!ok) revert WithdrawFailed();
        emit Withdrawn(to, amount);
    }

    function _update(address from, address to, uint256[] memory ids, uint256[] memory values)
        internal
        override(ERC1155, ERC1155Supply)
    {
        super._update(from, to, ids, values);
    }
}
