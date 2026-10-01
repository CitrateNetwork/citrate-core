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

## After the deploy

Citrate's **After deploy** panel (Agent, Contracts) takes the project from here:

1. finds the deployed address from the deploy receipt on chain 40204;
2. submits the contract source to CitrateScan's verifier (it needs `forge`);
3. switches the page to chain 40204 by writing `VITE_TARGET=citrate` and the
   address into `app/.env.local`;
4. pins the built page (`app/dist`, from `npm run build`) to the app's IPFS
   node and shows the CID with gateway links;
5. writes `vercel-export/`, a Vercel-ready copy of `app/` with `vercel.json`
   and a `.env.production` for chain 40204. Deploy it with your own Vercel
   account:

```sh
cd vercel-export
npx vercel deploy --prod
```

Citrate never signs in to Vercel or deploys there for you.
