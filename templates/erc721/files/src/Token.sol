// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc721" template (OpenZeppelin Contracts v5.7.0).
pragma solidity ^0.8.24;

import {ERC721} from "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @title {{ct:name}}
/// @notice A paid ERC-721 mint: token ids 1..MAX_SUPPLY, PRICE wei each.
contract {{ct:contract}} is ERC721, Ownable, ReentrancyGuard {
    uint256 public constant MAX_SUPPLY = {{ct:supply}};
    uint256 public constant PRICE = {{ct:price}};
    uint256 public constant MAX_PER_TX = 10;

    /// Tokens minted so far; the next token id is totalMinted + 1.
    uint256 public totalMinted;
    string private _baseUri;

    error BadQuantity(uint256 quantity);
    error SoldOut(uint256 requested, uint256 remaining);
    error WrongPayment(uint256 sent, uint256 expected);
    error WithdrawFailed();

    event BaseURIChanged(string baseURI);
    event Withdrawn(address indexed to, uint256 amount);

    constructor() ERC721("{{ct:name}}", "{{ct:symbol}}") Ownable({{ct:owner}}) {}

    /// Mint `quantity` tokens to the caller for exactly PRICE * quantity wei.
    function mint(uint256 quantity) external payable nonReentrant {
        if (quantity == 0 || quantity > MAX_PER_TX) revert BadQuantity(quantity);
        uint256 minted = totalMinted;
        uint256 remaining = MAX_SUPPLY - minted;
        if (quantity > remaining) revert SoldOut(quantity, remaining);
        uint256 expected = PRICE * quantity;
        if (msg.value != expected) revert WrongPayment(msg.value, expected);
        // Effects before the receiver callbacks in _safeMint.
        totalMinted = minted + quantity;
        for (uint256 i = 1; i <= quantity; ++i) {
            _safeMint(msg.sender, minted + i);
        }
    }

    function setBaseURI(string calldata baseURI) external onlyOwner {
        _baseUri = baseURI;
        emit BaseURIChanged(baseURI);
    }

    /// Send the whole mint balance to the owner.
    function withdraw() external onlyOwner nonReentrant {
        address to = owner();
        uint256 amount = address(this).balance;
        (bool ok,) = to.call{value: amount}("");
        if (!ok) revert WithdrawFailed();
        emit Withdrawn(to, amount);
    }

    function _baseURI() internal view override returns (string memory) {
        return _baseUri;
    }
}
