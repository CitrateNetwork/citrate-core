import { WagmiProvider, useBlockNumber } from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig } from "./wagmi";
import { citrate } from "./chain";
import "./App.css";

const queryClient = new QueryClient();

/**
 * Chain-read proof (CORE-S0, WP 0.3 partial): renders the live block height
 * from https://rpc.citrate.ai (chain 40204) via wagmi's useBlockNumber, which
 * drives viem's publicClient underneath. Live data or an honest error — never
 * a mock (Rule 1).
 */
function BlockHeight() {
  const { data: blockNumber, isPending, error } = useBlockNumber({
    watch: true,
    chainId: citrate.id,
  });

  if (isPending) {
    return <p className="status">Connecting to {citrate.rpcUrls.default.http[0]}…</p>;
  }
  if (error) {
    return (
      <p className="status error">
        Could not reach {citrate.rpcUrls.default.http[0]}: {error.message}
      </p>
    );
  }
  return (
    <p className="block-height">
      Block height <strong>{blockNumber.toString()}</strong>
    </p>
  );
}

function App() {
  return (
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={queryClient}>
        <main className="container">
          <h1>Citrate Core</h1>
          <p className="subtitle">
            CORE-S0 scaffold — live read from {citrate.name} (chain id {citrate.id})
          </p>
          <BlockHeight />
        </main>
      </QueryClientProvider>
    </WagmiProvider>
  );
}

export default App;
