import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { WagmiProvider, createConfig, http } from "wagmi";
import { injected } from "wagmi/connectors";
import { App, ConfigError } from "./App";
import { readConfig } from "./config";
import "./styles.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html is missing #root");
}

const config = readConfig(import.meta.env);

if (!config.ok) {
  createRoot(root).render(
    <StrictMode>
      <ConfigError message={config.error} />
    </StrictMode>,
  );
} else {
  const { deployment } = config;
  const wagmiConfig = createConfig({
    chains: [deployment.chain],
    connectors: [injected()],
    transports: { [deployment.chain.id]: http(deployment.rpcUrl) },
  });
  const queryClient = new QueryClient();
  createRoot(root).render(
    <StrictMode>
      <WagmiProvider config={wagmiConfig}>
        <QueryClientProvider client={queryClient}>
          <App deployment={deployment} />
        </QueryClientProvider>
      </WagmiProvider>
    </StrictMode>,
  );
}
