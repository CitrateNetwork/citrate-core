# {{ct:name}}

Generated from the Citrate **hello-mint** template.

| Directory | What it is |
|---|---|
| `contracts/` | Foundry project: the `{{ct:contract}}` ERC-721 (OpenZeppelin v5.7.0), unit tests, and the Medusa property harness (also run as Foundry invariants). |
| `app/` | vite + React + wagmi + viem mint page for the contract. |

Each directory has a `citrate-template.lock.json` recording the template digest,
the parameters and the pinned dependencies.

## Contracts

The dependencies are not included. Install the pinned versions listed in
`contracts/citrate-template.lock.json` into `contracts/lib/` (Citrate's bundled
toolchain does this for you), then:

```sh
cd contracts
forge build
forge test
medusa fuzz   # when Medusa is installed; budget is in medusa.json
```

## App

```sh
cd app
cp .env.example .env.local   # then edit
npm install
npm run dev
```

`VITE_TARGET=fork` points the page at a local anvil fork of chain 40204
(`VITE_FORK_RPC_URL`, loopback only). `VITE_TARGET=citrate` points it at chain
40204. `VITE_CONTRACT_ADDRESS` is the deployed contract; until it is set, the page
says the contract is not deployed yet.

The fork keeps chain id 40204, so a wallet cannot tell the fork and chain 40204
apart by chain id. The page reads from the RPC set here, but the mint
transaction goes wherever the wallet's own 40204 network points. In fork mode,
point the wallet's network at the fork RPC first, or the mint is sent to
chain 40204 itself.

This template contains no deploy step. Deploying goes through Citrate's deploy
gate and a signature you approve in the app.
