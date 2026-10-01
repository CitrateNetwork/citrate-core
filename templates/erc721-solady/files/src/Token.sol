// SPDX-License-Identifier: MIT
// Generated from the Citrate "erc721-solady" template (Solady v0.1.26).
pragma solidity ^0.8.24;

import {ERC721} from "solady/tokens/ERC721.sol";
import {Ownable} from "solady/auth/Ownable.sol";
import {ReentrancyGuard} from "solady/utils/ReentrancyGuard.sol";
import {LibString} from "solady/utils/LibString.sol";
import {SafeTransferLib} from "solady/utils/SafeTransferLib.sol";

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

    event BaseURIChanged(string baseURI);
    event Withdrawn(address indexed to, uint256 amount);

    constructor() {
        _initializeOwner({{ct:owner}});
    }

    function name() public pure override returns (string memory) {
        return "{{ct:name}}";
    }

    function symbol() public pure override returns (string memory) {
        return "{{ct:symbol}}";
    }

    function tokenURI(uint256 id) public view override returns (string memory) {
        if (!_exists(id)) revert TokenDoesNotExist();
        string memory base = _baseUri;
        return bytes(base).length == 0 ? "" : string.concat(base, LibString.toString(id));
    }

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
        SafeTransferLib.safeTransferETH(to, amount);
        emit Withdrawn(to, amount);
    }
}
