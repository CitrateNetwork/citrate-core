// HUP-S6.2: the hello-mint template's page configuration (templates/hello-mint/
// files/app/src/config.ts). The page switches between a local anvil fork of
// chain 40204 and chain 40204 itself by configuration only; these tests pin that
// switch and the refusals. The template file has no placeholders, so it is
// imported directly.
import { describe, expect, it } from "vitest";
import { CITRATE_CHAIN_ID, readConfig } from "../../templates/hello-mint/files/app/src/config";

const ADDR = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";

describe("hello-mint readConfig", () => {
  it("defaults to the local fork with no contract yet", () => {
    const r = readConfig({});
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.deployment.target).toBe("fork");
    expect(r.deployment.chain.id).toBe(CITRATE_CHAIN_ID);
    expect(r.deployment.rpcUrl).toBe("http://127.0.0.1:8545");
    expect(r.deployment.contract).toBeNull();
  });

  it("switches to chain 40204 and its public RPC by config", () => {
    const r = readConfig({ VITE_TARGET: "citrate", VITE_CONTRACT_ADDRESS: ADDR.toLowerCase() });
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.deployment.target).toBe("citrate");
    expect(r.deployment.chain.id).toBe(40204);
    expect(r.deployment.rpcUrl).toBe("https://rpc.citrate.ai");
    expect(r.deployment.chain.nativeCurrency.symbol).toBe("SALT");
    // Normalized to the checksummed form.
    expect(r.deployment.contract).toBe(ADDR);
  });

  it("ignores VITE_FORK_RPC_URL when targeting chain 40204", () => {
    const r = readConfig({ VITE_TARGET: "citrate", VITE_FORK_RPC_URL: "http://example.com" });
    expect(r.ok && r.deployment.rpcUrl).toBe("https://rpc.citrate.ai");
  });

  it("accepts loopback fork URLs only", () => {
    for (const url of ["http://127.0.0.1:8545", "http://localhost:9545", "http://[::1]:8545"]) {
      const r = readConfig({ VITE_FORK_RPC_URL: url });
      expect(r.ok, url).toBe(true);
    }
    for (const url of ["http://10.0.0.5:8545", "https://127.0.0.1:8545", "http://rpc.citrate.ai", "not a url", "file:///etc/hosts"]) {
      const r = readConfig({ VITE_FORK_RPC_URL: url });
      expect(r.ok, url).toBe(false);
    }
  });

  it("refuses an unknown target and a malformed address", () => {
    expect(readConfig({ VITE_TARGET: "mainnet" }).ok).toBe(false);
    expect(readConfig({ VITE_TARGET: "Fork" }).ok).toBe(false);
    expect(readConfig({ VITE_CONTRACT_ADDRESS: "0x1234" }).ok).toBe(false);
    expect(readConfig({ VITE_CONTRACT_ADDRESS: "0x5AAeb6053F3E94C9b9A09f33669435E7Ef1BeAed" }).ok).toBe(false);
    const r = readConfig({ VITE_TARGET: "mainnet" });
    expect(!r.ok && r.error).toContain("VITE_TARGET");
  });
});
