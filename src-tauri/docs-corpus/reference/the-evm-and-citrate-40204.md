# The EVM and Citrate 40204

Reference knowledge for how the Ethereum Virtual Machine executes, and where Citrate
(chain ID 40204, native token SALT) differs.

## The execution model

The EVM is a stack machine. A transaction targets an account: an externally owned
account (controlled by a key) or a contract account (controlled by code). Contract
execution reads and writes a key-value storage trie, can call other contracts, emits
logs, and either commits or reverts atomically. A revert rolls back all state changes
from that call but still consumes the gas spent up to the revert.

## Gas

Every operation costs gas; the transaction supplies a gas limit and the sender pays for
gas used at the effective gas price. Gas exists to price computation and storage and to
bound execution. Storage writes are among the most expensive operations; reading is
cheaper than writing; a fresh storage slot costs more than updating an existing one.

## Accounts, nonces, and addresses

An externally owned account has a nonce that increments with each sent transaction,
preventing replay. A contract's address is deterministic: `CREATE` derives it from the
deployer address and nonce, while `CREATE2` derives it from the deployer, a salt, and
the init-code hash, so a contract's address can be known before deployment. Citrate's
core contracts that must keep a stable address across a chain re-roll are deployed with
CREATE2; nonce-based contracts move on each re-roll.

## Citrate specifics

Citrate is an AI-native BlockDAG network. It uses GhostDAG consensus: a block may
reference multiple parents, forming a directed acyclic graph rather than a single linear
chain, which raises throughput while keeping a total order. The chain ID is 40204 and
the native token is SALT. The public RPC endpoint is https://rpc.citrate.ai.

## Gasless UX via a relayer

Citrate has no native paymaster at the protocol level, so sponsored (gasless)
transactions are implemented with an EIP-2771 trusted-forwarder relayer: the user signs
a meta-transaction, and a forwarder submits it on-chain and pays the gas, appending the
original sender so the target contract can recover it via `_msgSender()`. Account
abstraction flows use an EntryPoint with a paymaster depositing gas on the user's
behalf.

## On-chain registries

Citrate exposes registries that node software reads directly with `eth_call`: a
ModelRegistry (registered models: owner, name, framework, version, IPFS CID, price,
inference count, active flag) and a SkillRegistry (published agent skills: owner, name,
version, manifest CID, description, active flag). Clients enumerate hashes, then read
each record, decoding the ABI return by hand when a full ABI library is not available.

## Reading state without a full node

`eth_call` runs a contract function against the current state without sending a
transaction, so it costs no gas and changes nothing. It is how a client reads a
registry, a balance, or any view function. `eth_getLogs` with a filter retrieves the
events a contract emitted, which is how indexers and explorers reconstruct history.
