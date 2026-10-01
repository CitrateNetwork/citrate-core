import { useState } from "react";
import { BaseError, formatEther, type Address } from "viem";
import {
  useAccount,
  useConnect,
  useDisconnect,
  useReadContracts,
  useSwitchChain,
  useWaitForTransactionReceipt,
  useWriteContract,
} from "wagmi";
import { injected } from "wagmi/connectors";
import { mintAbi } from "./abi";
import { COLLECTION } from "./collection";
import type { Deployment } from "./config";

function errorText(error: unknown): string {
  if (error instanceof BaseError) return error.shortMessage;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}

export function MintCard({ deployment }: { deployment: Deployment }) {
  if (deployment.contract === null) {
    return (
      <section className="card">
        <h2>Not deployed yet</h2>
        <p>
          {COLLECTION.name} ({COLLECTION.symbol}): {COLLECTION.maxSupply.toString()} tokens at{" "}
          {formatEther(COLLECTION.priceWei)} SALT each.
        </p>
        <p className="hint">The page connects to the contract once VITE_CONTRACT_ADDRESS is set after deploy.</p>
      </section>
    );
  }
  return <LiveMintCard deployment={deployment} address={deployment.contract} />;
}

function LiveMintCard({ deployment, address }: { deployment: Deployment; address: Address }) {
  const chainId = deployment.chain.id;
  const account = useAccount();
  const { connect, isPending: connecting, error: connectError } = useConnect();
  const { disconnect } = useDisconnect();
  const { switchChain, isPending: switching } = useSwitchChain();
  const [quantity, setQuantity] = useState(1);

  const contract = { address, abi: mintAbi, chainId } as const;
  const reads = useReadContracts({
    contracts: [
      { ...contract, functionName: "totalMinted" },
      { ...contract, functionName: "MAX_SUPPLY" },
      { ...contract, functionName: "PRICE" },
      { ...contract, functionName: "MAX_PER_TX" },
    ],
    allowFailure: false,
  });

  const { writeContract, data: hash, isPending: signing, error: writeError, reset } = useWriteContract();
  const receipt = useWaitForTransactionReceipt({ hash, chainId });

  if (reads.isError) {
    return (
      <section className="card">
        <h2>Cannot read the contract</h2>
        <p>{errorText(reads.error)}</p>
        <p className="hint">Is the RPC reachable and is {address} the deployed contract?</p>
      </section>
    );
  }
  if (!reads.data) {
    return (
      <section className="card">
        <h2>Loading</h2>
        <p>Reading {COLLECTION.name} from the chain.</p>
      </section>
    );
  }

  const [minted, maxSupply, price, maxPerTx] = reads.data;
  const remaining = maxSupply - minted;
  const limit = Number(remaining < maxPerTx ? remaining : maxPerTx);
  const qty = Math.min(Math.max(quantity, 1), Math.max(limit, 1));
  const cost = price * BigInt(qty);
  const soldOut = remaining === 0n;
  const wrongChain = account.isConnected && account.chainId !== chainId;
  const busy = signing || receipt.isLoading;

  const onMint = () => {
    reset();
    writeContract(
      { ...contract, functionName: "mint", args: [BigInt(qty)], value: cost },
      { onSuccess: () => void reads.refetch() },
    );
  };

  return (
    <section className="card">
      <h2>
        {COLLECTION.name} <span className="symbol">{COLLECTION.symbol}</span>
      </h2>
      <p className="supply">
        {minted.toString()} / {maxSupply.toString()} minted
      </p>
      <p className="price">{price === 0n ? "Free mint" : `${formatEther(price)} SALT each`}</p>

      {!account.isConnected ? (
        <button type="button" disabled={connecting} onClick={() => connect({ connector: injected(), chainId })}>
          {connecting ? "Connecting" : "Connect wallet"}
        </button>
      ) : wrongChain ? (
        <button type="button" disabled={switching} onClick={() => switchChain({ chainId })}>
          {switching ? "Switching" : "Switch to Citrate"}
        </button>
      ) : soldOut ? (
        <p className="status">Sold out.</p>
      ) : (
        <div className="mint">
          <label>
            Quantity
            <input
              type="number"
              min={1}
              max={limit}
              value={qty}
              onChange={(e) => setQuantity(Number.parseInt(e.target.value, 10) || 1)}
            />
          </label>
          <button type="button" disabled={busy} onClick={onMint}>
            {busy ? "Minting" : `Mint ${qty} for ${formatEther(cost)} SALT`}
          </button>
        </div>
      )}

      {account.isConnected && (
        <p className="account">
          {account.address}{" "}
          <button type="button" className="link" onClick={() => disconnect()}>
            Disconnect
          </button>
        </p>
      )}
      {connectError && <p className="error">{errorText(connectError)}</p>}
      {writeError && <p className="error">{errorText(writeError)}</p>}
      {receipt.isSuccess && <p className="status">Minted. Transaction {hash}</p>}
      {receipt.isError && <p className="error">{errorText(receipt.error)}</p>}
    </section>
  );
}
