// The subset of the hello-mint ERC-721 ABI this page uses. It matches both the
// "erc721" (OpenZeppelin) and "erc721-solady" contract templates; the template
// test suite checks every entry against both sources.
export const mintAbi = [
  {
    type: "function",
    name: "mint",
    stateMutability: "payable",
    inputs: [{ name: "quantity", type: "uint256" }],
    outputs: [],
  },
  {
    type: "function",
    name: "totalMinted",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "uint256" }],
  },
  {
    type: "function",
    name: "MAX_SUPPLY",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "uint256" }],
  },
  {
    type: "function",
    name: "PRICE",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "uint256" }],
  },
  {
    type: "function",
    name: "MAX_PER_TX",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "uint256" }],
  },
  {
    type: "function",
    name: "balanceOf",
    stateMutability: "view",
    inputs: [{ name: "owner", type: "address" }],
    outputs: [{ name: "", type: "uint256" }],
  },
  {
    type: "error",
    name: "BadQuantity",
    inputs: [{ name: "quantity", type: "uint256" }],
  },
  {
    type: "error",
    name: "SoldOut",
    inputs: [{ name: "requested", type: "uint256" }, { name: "remaining", type: "uint256" }],
  },
  {
    type: "error",
    name: "WrongPayment",
    inputs: [{ name: "sent", type: "uint256" }, { name: "expected", type: "uint256" }],
  },
] as const;
