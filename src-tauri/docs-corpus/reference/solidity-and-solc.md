# Solidity and the solc Compiler

Reference knowledge for writing and compiling smart contracts. Procedural how-tos
(deploying a contract, registering a model) are delivered as executable skills, not
this reference — see the agent's contract-deploy skill.

## What Solidity is

Solidity is a statically typed, contract-oriented language that compiles to EVM
bytecode. A contract resembles a class: it has persistent state variables (stored in
contract storage), functions, events, and modifiers. Contracts run on any
EVM-compatible chain, including Citrate (chain 40204).

## The solc compiler

`solc` turns Solidity source into two main artifacts: the deployment bytecode
(what a deploy transaction carries) and the ABI (a JSON description of the contract's
functions and events that clients use to encode calls and decode results). Pin the
compiler version with a pragma such as `pragma solidity 0.8.24;` and pin the exact
version in your build config so a build is reproducible. The 0.8.x series adds
checked arithmetic by default: overflow and underflow revert instead of wrapping.

## Value types and storage

Common value types are `uint256` (the default word size), `address`, `bool`,
`bytes32`, and fixed/dynamic `bytes`/`string`. Reference types (`struct`, arrays,
`mapping`) live in `storage`, `memory`, or `calldata`; choosing the right data
location matters for both correctness and gas. `storage` is persistent and expensive;
`memory` is transient within a call; `calldata` is read-only input.

## Visibility and mutability

Functions are `external`, `public`, `internal`, or `private`, and are annotated
`view` (reads state, no writes), `pure` (touches no state), or `payable` (accepts
value). A function with none of these may modify state. Prefer the narrowest
visibility that works, and mark reads `view` so callers can call them for free.

## The ABI and function selectors

A function is identified on-chain by its selector: the first four bytes of the
keccak-256 hash of its canonical signature, e.g. `getModel(bytes32)`. Calldata is the
selector followed by ABI-encoded arguments. Dynamic types (strings, bytes, arrays)
are encoded head/tail: the head holds a 32-byte offset to the tail, and the tail holds
a length word followed by padded data. Understanding this layout is what lets a client
decode a contract return without a full ABI library.

## Events and logs

`event` declarations define structured logs a contract emits with `emit`. Indexed
event parameters become searchable topics; non-indexed parameters go in the log data.
Off-chain indexers and explorers read these logs to reconstruct state changes.

## Safety patterns

Follow checks-effects-interactions: validate inputs, update state, then make external
calls, so a reentrant callee cannot observe stale state. Use a reentrancy guard on
functions that move value. Prefer pull-over-push for payouts. Never trust an external
call's success blindly; check return values. Access control (owner/role checks) gates
privileged functions.
