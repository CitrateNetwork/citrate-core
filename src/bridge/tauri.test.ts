// CORE-A1 A1.5 — Tauri adapter: config round-trip (mocked invoke) + honest
// Unavailable on every unwired seam domain. This is the frontend half of the
// A1.4 proof; the Rust half is `cargo test` (config::tests).
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { AppConfig } from "./types";

// Mock the invoke boundary. We simulate a real on-disk store: writes mutate a
// persisted object, reads return it — proving the read-your-write round-trip
// the config domain relies on.
const persisted: { app: AppConfig } = {
  app: {
    net: "testnet",
    rpc: "local",
    dataDir: "~/.citrate/core",
    cpuCap: 50,
    autolock: 30,
    channel: "stable",
    telemetry: false,
    sigPolicy: "hitl",
  },
};

// B1.2 signing mock state: a single-use ceremony map + a monotonic id.
const signMock: { map: Map<string, { id: string; origin: string; kind: string; chainId: number; decoded: { action: string; cost: string; destination: string }; requiresRawAck: boolean }>; next: number } = {
  map: new Map(),
  next: 1,
};

// C2-F-1: the mocked on-chain claimable `agent_earnings` returns; a test can flip
// it to "0" to exercise the honest "nothing to claim" branch.
const earningsMock = { claimableWei: "9410000000000000000" };

// CORE-AI1: a mocked OS-keyring map of sealed provider configs. The KEY is stored
// here (simulating the keyring), but NO command RESULT ever returns it — the whole
// point of the @rule8 boundary.
const aiMock: { map: Map<string, { baseURL: string; model: string; apiKey: string }>; default: string | null } = {
  map: new Map(),
  default: null,
};

// CORE-BC-3: the mocked model status the `model_status` command returns; a test
// flips it to exercise the notPresent/downloading/ready branches.
const modelMock: { status: unknown } = { status: { state: "notPresent" } };

// W4 — the mocked vault-sealed connection state. A test flips `connected` by
// invoking connection_start; the RESULT never carries a token (I-2 boundary).
const connMock: Record<string, { connected: boolean; scope: string | null; connectedAt: number | null }> = {
  github: { connected: false, scope: null, connectedAt: null },
  gdrive: { connected: false, scope: null, connectedAt: null },
  notion: { connected: false, scope: null, connectedAt: null },
};

const invokeMock = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
  switch (cmd) {
    case "config_read":
      return { ...persisted.app };
    case "config_write": {
      const patch = (args?.patch ?? {}) as Partial<AppConfig>;
      persisted.app = { ...persisted.app, ...patch };
      return { ...persisted.app };
    }
    case "config_keyring_status":
      return "available";
    // CORE-A3 auth commands — claim-derived AuthStatus (never a token).
    case "auth_status":
    case "auth_login":
    case "auth_userinfo":
    case "auth_refresh":
      return {
        signedIn: true,
        sub: "usr_2af4c19e",
        tier: "pilot",
        org: null,
        role: "member",
        kycStatus: "verified",
        walletAddr: "0xabc",
        expiresAt: "2027-07-11",
        email: "dana@example.com",
      };
    case "auth_logout":
    case "kyc_start":
      return undefined;
    // CORE-D3.C — membership_checkout opens the checkout popup and returns void.
    // The money + grant are server-side; the store polls /userinfo for the grant.
    case "membership_checkout":
      return undefined;
    // BC-1.3 — membership_grant_status: the REAL on-chain grant read. The mock
    // returns a granted status keyed by the member address so the adapter's arg
    // shape ({ memberAddress }) + decoded return are asserted.
    case "membership_grant_status":
      return {
        attributedStakeWei: (32000n * 10n ** 18n).toString(),
        attributedSharesWei: (32000n * 10n ** 18n).toString(),
        hasSbt: true,
      };
    // CORE-B1.2 signing — the ceremony commands. request → decoded view (NO
    // signature); approve → a signature hex ONLY (never a key); reject → void.
    // This mock simulates the single-use ceremony map so the adapter's calls are
    // asserted against the real command names + arg shapes.
    case "sign_request": {
      const intent = (args?.intent ?? {}) as { origin: string; kind: string; chainId: number; raw: string };
      const id = String(signMock.next++);
      // Decode: a UTF-8 personal_sign is decodable; everything else is raw-gated.
      let action = "Unrecognized";
      let requiresRawAck = true;
      if (intent.kind === "personal_sign") {
        action = "Sign message: \"hello\"";
        requiresRawAck = false;
      }
      const view = { id, origin: intent.origin, kind: intent.kind, chainId: intent.chainId, decoded: { action, cost: "", destination: "" }, requiresRawAck };
      signMock.map.set(id, view);
      return view;
    }
    case "sign_approve": {
      const id = String(args?.id);
      const view = signMock.map.get(id);
      if (!view) throw "ceremony: unknown or already-consumed id";
      if (view.requiresRawAck && !args?.rawAck) throw "ceremony: undecodable calldata requires an explicit raw-mode ack";
      signMock.map.delete(id); // single-use consume
      return { sigHex: "ab".repeat(64), kind: view.kind };
    }
    case "sign_and_broadcast": {
      // CORE-B1.4 — sign the real EIP-155 tx + broadcast; return the real tx
      // hash + block (never key material). Single-use consume + raw-ack gate.
      const id = String(args?.id);
      const view = signMock.map.get(id);
      if (!view) throw "ceremony: unknown or already-consumed id";
      if (view.requiresRawAck && !args?.rawAck) throw "ceremony: undecodable calldata requires an explicit raw-mode ack";
      signMock.map.delete(id); // single-use consume
      return { txHash: "0x" + "ab".repeat(32), blockNumber: 100 };
    }
    case "sign_reject": {
      const id = String(args?.id);
      if (!signMock.map.delete(id)) throw "ceremony: unknown or already-consumed id";
      return undefined;
    }
    // CORE-C1.1 node — real citrate-node under the SidecarSupervisor. status
    // returns the node's real sync shape; start/stop return void. No secret
    // ever crosses this boundary.
    case "node_status":
      return { state: "running", peers: 3, height: 420, syncPct: 100 };
    case "node_start":
    case "node_stop":
      return undefined;
    // CORE-BC-3 model — the local Gemma download + verify + llama-server sidecar.
    // status returns the honest file-derived state (Ready only after a real
    // verify); download/verify/serveStart return void. `modelMock` lets a test flip
    // the status to exercise the downloading/ready branches.
    case "model_status":
      return modelMock.status;
    case "model_download":
    case "model_verify":
    case "model_serve_start":
      return undefined;
    // CORE-C1.2 node-agent — under the SidecarSupervisor. status returns the
    // supervisor state + whether a bearer session exists (NEVER the token);
    // start/stop return void. No secret ever crosses this boundary.
    case "agent_status":
      return { state: "running", authed: true };
    case "agent_start":
    case "agent_stop":
      return undefined;
    // CORE-C3 memory — the mcp_serve daemon under the SidecarSupervisor. status
    // returns supervisor state + socket path (NEVER the store key); recall/
    // search/neighbors/constellation return REAL parsed nodes from the store.
    case "memory_status":
      return { state: "running", socketPath: "/tmp/memdag.sock", semantic: false };
    case "memory_start":
    case "memory_stop":
      return undefined;
    case "memory_recall":
    case "memory_search":
      return { tenant: (args?.tenant as string) ?? "personal", totalInTenant: 2, hits: [{ id: "0a1b2c3d4e", kind: "ChainContract", title: "LiquidStakingPool" }] };
    case "memory_neighbors":
      return [{ direction: "out", kind: "References", title: "chain 40204 params", proposed: false }];
    case "memory_constellation":
      return [
        { tenant: "personal", totalInTenant: 1, hits: [{ id: "aa11", kind: "Doc", title: "note" }] },
        { tenant: "chain-state", totalInTenant: 1, hits: [{ id: "bb22", kind: "ChainNetwork", title: "chain 40204 params" }] },
      ];
    // CORE-C2 earnings — the REAL claimable from
    // ContributionAccounting.claimable(addr) via eth_call. Returns the single
    // real claimable (wei) + its data source; NO per-source breakdown.
    case "agent_earnings":
      return {
        claimableWei: earningsMock.claimableWei,
        walletAddress: "0x9858effd232b4033e47d90003d41ec34ecaeda94",
        contract: "0xcdd2477387279c7d44a1053f44db5dac0fd8faef",
      };
    // CORE wallet balances — REAL native liquid (eth_getBalance) + claimable +
    // self-stake (LiquidStakingPool.balanceOf). Wei strings; the adapter converts
    // each to a SALT number (no -1 sentinel now that self-stake is grounded).
    case "wallet_balances":
      return {
        liquidWei: "1500000000000000000", // 1.5 SALT
        claimableWei: earningsMock.claimableWei,
        stakedWei: "5000000000000000000", // 5 SALT self-staked
        address: "0x9858effd232b4033e47d90003d41ec34ecaeda94",
      };
    // CORE wallet_ensure_ready — seamless device provisioning + first-run mint.
    // Returns ONLY the public address (never key/seed) + whether it was minted.
    case "wallet_ensure_ready":
      return { address: "0x9858effd232b4033e47d90003d41ec34ecaeda94", created: true };
    // CORE wallet_send — a native transfer bridged into a PENDING ceremony;
    // returns the decoded view (the human approves via sign_and_broadcast).
    case "open_external":
      return undefined; // opener returns void; we assert the invoke args instead
    case "wallet_send": {
      const id = String(signMock.next++);
      const view = {
        id,
        origin: "local-user",
        kind: "transaction",
        chainId: 40204,
        decoded: { action: "Transfer", cost: "1.5 SALT", destination: String((args as { to: string }).to) },
        requiresRawAck: false,
      };
      signMock.map.set(id, view);
      return view;
    }
    // CORE wallet_stake — a LiquidStakingPool deposit() bridged into a PENDING
    // ceremony; returns the decoded view (approved via sign_and_broadcast).
    case "wallet_stake": {
      const id = String(signMock.next++);
      const view = {
        id,
        origin: "local-user",
        kind: "transaction",
        chainId: 40204,
        decoded: { action: "Call LiquidStakingPool with 4 bytes calldata", cost: "deposit", destination: "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e" },
        requiresRawAck: false,
      };
      signMock.map.set(id, view);
      return view;
    }
    // CORE WP2 wallet_request_withdrawal — a LiquidStakingPool requestWithdrawal
    // (shares) bridged into a PENDING ceremony (approved via sign_and_broadcast).
    // The SALT→shares conversion is done in Rust; this returns the decoded view.
    case "wallet_request_withdrawal": {
      const id = String(signMock.next++);
      const view = {
        id,
        origin: "local-user",
        kind: "transaction",
        chainId: 40204,
        decoded: { action: "Call LiquidStakingPool with 36 bytes calldata", cost: "requestWithdrawal", destination: "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e" },
        requiresRawAck: false,
      };
      signMock.map.set(id, view);
      return view;
    }
    // CORE WP2 wallet_claim_withdrawal — a claimWithdrawal(id) bridged into a
    // PENDING ceremony (approved via sign_and_broadcast).
    case "wallet_claim_withdrawal": {
      const id = String(signMock.next++);
      const view = {
        id,
        origin: "local-user",
        kind: "transaction",
        chainId: 40204,
        decoded: { action: "Call LiquidStakingPool with 36 bytes calldata", cost: "claimWithdrawal", destination: "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e" },
        requiresRawAck: false,
      };
      signMock.map.set(id, view);
      return view;
    }
    // CORE WP2 wallet_pending_withdrawals — the wallet's real on-chain queue.
    case "wallet_pending_withdrawals":
      return [
        { id: "1", saltWei: "10000000000000000000", requestBlock: 100, claimableAtBlock: 50500, claimable: true },
        { id: "2", saltWei: "3000000000000000000", requestBlock: 100000, claimableAtBlock: 150400, claimable: false },
      ];
    // CORE item 4 wallet_activity — the REAL indexed 40204 tx history from the
    // CitrateScan `txlist` endpoint (public read). The Rust command returns rows
    // with {id,kind,amount,hash,ts,status,direction}; the adapter maps them down to
    // the Activity shape ({id,kind,amount,hash,ts}) the state model renders.
    case "wallet_activity":
      return [
        { id: "0xaa", kind: "Received", amount: "+12.41 SALT", hash: "0xaa", ts: 1_700_000_500_000, status: 1, direction: "in" },
        { id: "0xbb", kind: "Sent", amount: "−40.00 SALT", hash: "0xbb", ts: 1_700_000_000_000, status: 0, direction: "out" },
      ];
    // CORE-C2-F-1 user_claim — bridges the REAL claimRewards() intent into a
    // PENDING ceremony (a legible tx Call, approvable via sign_and_broadcast).
    // Returns a CeremonyView; NEVER a signature or a local balance mutation.
    case "user_claim": {
      const id = String(signMock.next++);
      const view = {
        id,
        origin: "agent:node-agent",
        kind: "transaction",
        chainId: 40204,
        decoded: { action: "Call claimRewards()", cost: "", destination: "0xcdd2477387279c7d44a1053f44db5dac0fd8faef" },
        requiresRawAck: false,
      };
      signMock.map.set(id, view);
      return view;
    }
    // CORE-AI1 (@rule8) — the AI provider commands. set seals {baseURL,model,
    // apiKey} in the OS keyring (returns void — NEVER the key); status returns
    // metadata ONLY; clear deletes; chat returns the model completion string. This
    // mock simulates a keyring map so the adapter's arg shapes are asserted and the
    // key never appears in any RESULT.
    case "ai_set_provider": {
      const a = (args ?? {}) as { providerId: string; baseUrl: string; model: string; apiKey: string };
      aiMock.map.set(a.providerId, { baseURL: a.baseUrl, model: a.model, apiKey: a.apiKey });
      if (!aiMock.default) aiMock.default = a.providerId;
      return undefined; // NEVER returns the key
    }
    case "ai_provider_status":
      return ["openai", "gateway", "custom"].map((pid) => {
        const cfg = aiMock.map.get(pid);
        return {
          id: pid,
          baseURL: cfg?.baseURL ?? "",
          model: cfg?.model ?? "",
          configured: !!cfg,
          isDefault: aiMock.default === pid,
        };
      });
    case "ai_clear_provider": {
      const pid = String((args as { providerId: string }).providerId);
      aiMock.map.delete(pid);
      if (aiMock.default === pid) aiMock.default = null;
      return undefined;
    }
    case "ai_chat": {
      const a = (args ?? {}) as { providerId: string; messagesJson: string; contextJson: string };
      if (!aiMock.map.has(a.providerId)) throw "ai: no provider configured for that id";
      // The completion echoes the endpoint it would use so a test can assert the
      // STORED baseURL was chosen (the webview never passes a URL).
      const cfg = aiMock.map.get(a.providerId)!;
      return `real-completion from ${cfg.baseURL} model ${cfg.model}`;
    }
    case "ai_chat_local": {
      // BC-3.2 — REAL LOCAL inference: the webview supplies ONLY messages + context.
      // Rust derives the loopback endpoint from the serve manager (no url arg here).
      return "local-completion from the bundled llama-server";
    }
    case "model_inference_state": {
      // BC-3.2 — the honest routing state, computed in Rust. The mock echoes the
      // gatewayConfigured flag so a test can assert it was passed through.
      const a = (args ?? {}) as { gatewayConfigured: boolean };
      return a.gatewayConfigured ? "gateway-only" : "demo";
    }
    // W4 — MCP connections (OAuth). status returns all three; start marks the
    // service connected + returns its status; disconnect clears it. No token.
    case "connection_status":
      return Object.entries(connMock).map(([service, v]) => ({ service, ...v }));
    case "connection_start": {
      const service = String((args ?? {}).service);
      connMock[service] = { connected: true, scope: service === "github" ? "repo" : null, connectedAt: 1_770_000_000 };
      return { service, ...connMock[service] };
    }
    case "connection_disconnect": {
      const service = String((args ?? {}).service);
      connMock[service] = { connected: false, scope: null, connectedAt: null };
      return undefined;
    }
    default:
      throw `unavailable: ${cmd} is not wired in this build`;
  }
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
  isTauri: () => true,
}));

import { createTauriBridge } from "./tauri";
import { isUnavailable } from "./types";

describe("tauri adapter — config round-trip proof (A1.4, frontend half)", () => {
  beforeEach(() => {
    persisted.app = {
      net: "testnet",
      rpc: "local",
      dataDir: "~/.citrate/core",
      cpuCap: 50,
      autolock: 30,
      channel: "stable",
      telemetry: false,
      sigPolicy: "hitl",
    };
    invokeMock.mockClear();
  });

  it("a value written through the bridge is read back — persists across a fresh read", async () => {
    const bridge = createTauriBridge();
    await bridge.config.write({ net: "local", telemetry: true, autolock: 15 });

    // simulate a restart: a brand-new bridge reading the same persisted store
    const afterRestart = createTauriBridge();
    const cfg = await afterRestart.config.read();
    expect(cfg.net).toBe("local");
    expect(cfg.telemetry).toBe(true);
    expect(cfg.autolock).toBe(15);
    // untouched fields survive
    expect(cfg.channel).toBe("stable");

    expect(invokeMock).toHaveBeenCalledWith("config_write", { patch: { net: "local", telemetry: true, autolock: 15 } });
    expect(invokeMock).toHaveBeenCalledWith("config_read", undefined);
  });

  it("keyring status is read from the real command, not fabricated", async () => {
    const bridge = createTauriBridge();
    expect(await bridge.config.keyringStatus()).toBe("available");
  });
});

describe("tauri adapter — wallet.balances is a REAL 40204 read", () => {
  it("invokes wallet_balances and converts wei→SALT incl. real self-stake (balanceOf)", async () => {
    const bridge = createTauriBridge();
    // Uses the shared earningsMock default for claimable (do NOT mutate it — other
    // tests assert against the default). liquid is the mock's 1.5 SALT.
    const b = await bridge.wallet.balances();
    expect(invokeMock).toHaveBeenCalledWith("wallet_balances", undefined);
    expect(b.liquid).toBeCloseTo(1.5, 9);
    expect(b.claimable).toBeCloseTo(Number(BigInt(earningsMock.claimableWei)) / 1e18, 9);
    expect(b.staked).toBeCloseTo(5, 9); // real LiquidStakingPool.balanceOf (self-stake), no -1 sentinel
    expect(b.address).toBe("0x9858effd232b4033e47d90003d41ec34ecaeda94");
  });

  it("wallet.ensureReady invokes wallet_ensure_ready and passes the public address + created flag through", async () => {
    const bridge = createTauriBridge();
    const r = await bridge.wallet.ensureReady();
    expect(invokeMock).toHaveBeenCalledWith("wallet_ensure_ready", undefined);
    expect(r.address).toBe("0x9858effd232b4033e47d90003d41ec34ecaeda94");
    expect(r.created).toBe(true);
    // @rule8 / I-2: the boundary carries ONLY the address + a flag — no key/seed.
    expect(Object.keys(r).sort()).toEqual(["address", "created"]);
  });

  it("wallet.stake invokes wallet_stake with {amountWei} and returns a pending CeremonyView", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.wallet.stake("3000000000000000000");
    expect(invokeMock).toHaveBeenCalledWith("wallet_stake", { amountWei: "3000000000000000000" });
    expect(view.id).toBeTruthy();
    expect(view.origin).toBe("local-user");
    expect(view.kind).toBe("transaction");
    expect(view.requiresRawAck).toBe(false);
  });

  it("shell.openExternal invokes open_external with the url", async () => {
    const bridge = createTauriBridge();
    await bridge.shell.openExternal("https://explorer.citrate.ai/tx/0xabc");
    expect(invokeMock).toHaveBeenCalledWith("open_external", { url: "https://explorer.citrate.ai/tx/0xabc" });
  });

  it("wallet.send invokes wallet_send with {to, amountWei} and returns a pending CeremonyView", async () => {
    const bridge = createTauriBridge();
    const to = "0x1111111111111111111111111111111111111111";
    const view = await bridge.wallet.send(to, "1500000000000000000");
    expect(invokeMock).toHaveBeenCalledWith("wallet_send", { to, amountWei: "1500000000000000000" });
    expect(view.id).toBeTruthy();
    expect(view.decoded.destination).toBe(to); // signs nothing; human approves via broadcast
  });

  // CORE WP2 (@rule8) — the withdraw path. requestWithdrawal builds a pending
  // ceremony from the SALT amount (shares conversion done in Rust); claimWithdrawal
  // builds a pending ceremony for a matured id; pendingWithdrawals reads the real
  // on-chain queue. None settle here — the human approves via sign_and_broadcast.
  it("wallet.requestWithdrawal invokes wallet_request_withdrawal with {amountWei}", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.wallet.requestWithdrawal("2500000000000000000");
    expect(invokeMock).toHaveBeenCalledWith("wallet_request_withdrawal", { amountWei: "2500000000000000000" });
    expect(view.id).toBeTruthy();
    expect(view.decoded.destination).toBe("0xfd272195b55cb4f5a240a5be75aabab0d1c5685e");
    expect(view.requiresRawAck).toBe(false);
  });

  it("wallet.claimWithdrawal invokes wallet_claim_withdrawal with {requestId}", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.wallet.claimWithdrawal("7");
    expect(invokeMock).toHaveBeenCalledWith("wallet_claim_withdrawal", { requestId: "7" });
    expect(view.id).toBeTruthy();
    expect(view.decoded.destination).toBe("0xfd272195b55cb4f5a240a5be75aabab0d1c5685e");
  });

  it("wallet.pendingWithdrawals invokes wallet_pending_withdrawals and returns the real queue", async () => {
    const bridge = createTauriBridge();
    const list = await bridge.wallet.pendingWithdrawals();
    expect(invokeMock).toHaveBeenCalledWith("wallet_pending_withdrawals", undefined);
    expect(list).toHaveLength(2);
    expect(list[0].id).toBe("1");
    expect(list[0].claimable).toBe(true);
    expect(list[1].claimable).toBe(false);
    expect(list[0].saltWei).toBe("10000000000000000000");
  });
});

// CORE item 4 — wallet.activity now invokes the REAL `wallet_activity` command
// (the CitrateScan `txlist` read), no longer a seam stub. The adapter maps the
// Rust rows down to the Activity shape the state model renders.
describe("tauri adapter — wallet.activity invokes the real CitrateScan read (item 4)", () => {
  it("invokes wallet_activity and returns the Activity-shaped indexed history", async () => {
    const bridge = createTauriBridge();
    const rows = await bridge.wallet.activity();
    expect(invokeMock).toHaveBeenCalledWith("wallet_activity", undefined);
    expect(rows).toHaveLength(2);
    // Q-E.2 (C-5) — the receipt `status` (and `direction`) are now PASSED THROUGH
    // the boundary so a reverted tx can render a failed marker (it used to strip
    // both, making a failed tx look identical to a success).
    expect(Object.keys(rows[0]).sort()).toEqual(["amount", "direction", "hash", "id", "kind", "status", "ts"]);
    expect(rows[0].kind).toBe("Received");
    expect(rows[0].amount).toBe("+12.41 SALT");
    expect(rows[0].hash).toBe("0xaa");
    expect(rows[0].ts).toBe(1_700_000_500_000);
    expect(rows[0].status).toBe(1);
    expect(rows[0].direction).toBe("in");
    // Newest-first order preserved from the indexer (desc timestamp).
    expect(rows[1].hash).toBe("0xbb");
    // A reverted tx carries status=0 through the boundary (the failed marker).
    expect(rows[1].status).toBe(0);
  });
});

describe("tauri adapter — unwired domains are honestly Unavailable (Rule 1)", () => {
  it("every still-unwired seam domain rejects with Unavailable", async () => {
    const bridge = createTauriBridge();
    // NOTE: `auth` (CORE-A3) and `node` (CORE-C1.1) are now genuinely wired and
    // are asserted separately.
    const calls = [
      // memory.assert (a signed WRITE) stays a seam stub until it routes through
      // the ceremony; recall/search/neighbors are now genuinely wired (C3).
      () => bridge.memory.assert("x"),
      () => bridge.membership.entitlement(),
      () => bridge.commissary.catalog(),
      () => bridge.comms.connections(),
      () => bridge.chat.backend(),
    ];
    for (const c of calls) {
      await expect(c()).rejects.toSatisfy((e: unknown) => isUnavailable(e));
    }
  });
});

// CORE-A3 A3.3 — the auth domain now invokes the real OIDC commands. This is the
// frontend half of the boundary: every auth invoke returns claim-derived flags
// (AuthStatus) and NEVER a token (ADV-8). The Rust half is `cargo test`
// (oidc::tests). Here we assert the adapter calls the right commands and passes
// the claim-derived status straight through — no token field is even present.
describe("tauri adapter — auth domain invokes real OIDC commands (A3.3)", () => {
  it("status/login/userinfo/refresh return claim-derived flags, never a token", async () => {
    const bridge = createTauriBridge();
    for (const call of [
      () => bridge.auth.status(),
      () => bridge.auth.login(),
      () => bridge.auth.userinfo(),
      () => bridge.auth.refresh(),
    ]) {
      const st = await call();
      expect(st.signedIn).toBe(true);
      expect(st.tier).toBe("pilot");
      // The AuthStatus shape has no token field at all (ADV-8 boundary).
      expect(Object.keys(st)).not.toContain("access_token");
      expect(Object.keys(st)).not.toContain("refresh_token");
      expect(JSON.stringify(st)).not.toContain("token");
    }
  });

  it("logout + kycStart invoke their commands and return void", async () => {
    const bridge = createTauriBridge();
    await expect(bridge.auth.logout()).resolves.toBeUndefined();
    await expect(bridge.auth.kycStart()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("auth_logout", undefined);
    expect(invokeMock).toHaveBeenCalledWith("kyc_start", undefined);
  });
});

// CORE-B1.2 B1.2.1 — the signing domain invokes the real SignatureCeremony
// commands. Frontend half: request returns a decoded view (NO signature), approve
// returns a signature hex ONLY (never a key), the ceremony is single-use, and the
// raw-ack gate + explicit-id binding are honored across the invoke boundary. The
// Rust half (state machine, ecrecover, negative controls) is `cargo test`.
describe("tauri adapter — signing domain invokes the SignatureCeremony (B1.2.1)", () => {
  beforeEach(() => {
    signMock.map.clear();
    signMock.next = 1;
    invokeMock.mockClear();
  });

  it("request returns a decoded PENDING view with NO signature field", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.signing.request({ origin: "https://app.citrate.ai", kind: "personal_sign", chainId: 40204, raw: "0x68656c6c6f" });
    expect(view.id).toBeTruthy();
    expect(view.origin).toBe("https://app.citrate.ai"); // TRUE origin surfaced
    expect(view.decoded.action).toContain("Sign message");
    expect(view.requiresRawAck).toBe(false);
    // A request NEVER carries a signature (the type has no sig field).
    expect(JSON.stringify(view)).not.toContain("sigHex");
    expect(invokeMock).toHaveBeenCalledWith("sign_request", { intent: { origin: "https://app.citrate.ai", kind: "personal_sign", chainId: 40204, raw: "0x68656c6c6f" } });
  });

  it("approve on a specific id returns a signature (hex) and consumes the ceremony (single-use)", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.signing.request({ origin: "o", kind: "personal_sign", chainId: 40204, raw: "0x68656c6c6f" });
    const sig = await bridge.signing.approve(view.id, false);
    expect(sig.sigHex).toHaveLength(128);
    expect(invokeMock).toHaveBeenCalledWith("sign_approve", { id: view.id, rawAck: false });
    // The signature carries no key material — only a signature + kind.
    expect(Object.keys(sig).sort()).toEqual(["kind", "sigHex"]);
    // Single-use: a second approve on the same id is rejected.
    await expect(bridge.signing.approve(view.id, false)).rejects.toBeTruthy();
  });

  it("undecodable calldata (transaction) is raw-ack gated across the boundary", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.signing.request({ origin: "agent", kind: "transaction", chainId: 40204, raw: "0x02f86b" });
    expect(view.requiresRawAck).toBe(true);
    // approve WITHOUT the ack → rejected.
    await expect(bridge.signing.approve(view.id, false)).rejects.toBeTruthy();
    // approve WITH the explicit ack → signs.
    const view2 = await bridge.signing.request({ origin: "agent", kind: "transaction", chainId: 40204, raw: "0x02f86b" });
    const sig = await bridge.signing.approve(view2.id, true);
    expect(sig.sigHex).toHaveLength(128);
  });

  it("reject consumes without a signature; unknown id errors", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.signing.request({ origin: "o", kind: "personal_sign", chainId: 40204, raw: "0x68656c6c6f" });
    await expect(bridge.signing.reject(view.id)).resolves.toBeUndefined();
    // The rejected id cannot then be approved.
    await expect(bridge.signing.approve(view.id, false)).rejects.toBeTruthy();
    // An unknown id rejects.
    await expect(bridge.signing.reject("does-not-exist")).rejects.toBeTruthy();
  });

  // CORE-B1.4 — broadcast invokes sign_and_broadcast and returns the real tx
  // hash + block (never key material); single-use consume holds across invoke.
  it("broadcast invokes sign_and_broadcast, returns the real tx hash + block, single-use", async () => {
    const bridge = createTauriBridge();
    const view = await bridge.signing.request({ origin: "app", kind: "transaction", chainId: 40204, raw: '{"to":"0x35","value":"0x1"}' });
    // The raw JSON tx here is raw-gated by this mock's decode (non-personal_sign);
    // broadcast WITH the explicit ack signs + broadcasts.
    const result = await bridge.signing.broadcast(view.id, true);
    expect(result.txHash).toMatch(/^0x[0-9a-fA-F]{64}$/);
    expect(result.blockNumber).toBe(100);
    expect(invokeMock).toHaveBeenCalledWith("sign_and_broadcast", { id: view.id, rawAck: true });
    // No key material in the result — only public tx facts.
    expect(Object.keys(result).sort()).toEqual(["blockNumber", "txHash"]);
    // Single-use: a second broadcast on the same id is rejected.
    await expect(bridge.signing.broadcast(view.id, true)).rejects.toBeTruthy();
  });
});

// CORE-C1.1 — the node domain now invokes the real citrate-node commands under
// the SidecarSupervisor. status returns the node's real sync shape
// ({state,peers,height,syncPct}); start/stop invoke the spawn/release commands.
// No secret ever crosses this boundary.
describe("tauri adapter — node domain is wired to the real citrate-node (C1.1)", () => {
  it("status invokes node_status and passes the real sync shape through", async () => {
    const bridge = createTauriBridge();
    const st = await bridge.node.status();
    expect(invokeMock).toHaveBeenCalledWith("node_status", undefined);
    expect(st).toEqual({ state: "running", peers: 3, height: 420, syncPct: 100 });
    // No token / secret field is even present in the shape.
    expect(Object.keys(st).sort()).toEqual(["height", "peers", "state", "syncPct"]);
  });

  it("start invokes node_start and stop invokes node_stop", async () => {
    const bridge = createTauriBridge();
    await bridge.node.start();
    expect(invokeMock).toHaveBeenCalledWith("node_start", undefined);
    await bridge.node.stop();
    expect(invokeMock).toHaveBeenCalledWith("node_stop", undefined);
  });
});

// CORE-C1.2 — the agent domain invokes the real node-agent commands under the
// SidecarSupervisor. status returns {state, authed} — the bearer token is NEVER
// exposed across the bridge. start/stop invoke the spawn/release commands.
describe("tauri adapter — agent domain is wired to the real node-agent (C1.2)", () => {
  it("status invokes agent_status and exposes NO bearer token", async () => {
    const bridge = createTauriBridge();
    const st = await bridge.agent.status();
    expect(invokeMock).toHaveBeenCalledWith("agent_status", undefined);
    expect(st).toEqual({ state: "running", authed: true });
    // The shape carries only {state, authed} — no token/bearer field exists.
    expect(Object.keys(st).sort()).toEqual(["authed", "state"]);
    expect(JSON.stringify(st)).not.toContain("bearer");
    expect(JSON.stringify(st)).not.toContain("token");
  });

  it("start invokes agent_start and stop invokes agent_stop", async () => {
    const bridge = createTauriBridge();
    await bridge.agent.start();
    expect(invokeMock).toHaveBeenCalledWith("agent_start", undefined);
    await bridge.agent.stop();
    expect(invokeMock).toHaveBeenCalledWith("agent_stop", undefined);
  });

  // CORE-C2 — earnings invokes the real agent_earnings command; the single real
  // claimable (wei) + its data source cross the boundary, NO fabricated split.
  it("earnings invokes agent_earnings and returns the real claimable + data source", async () => {
    const bridge = createTauriBridge();
    const e = await bridge.agent.earnings();
    expect(invokeMock).toHaveBeenCalledWith("agent_earnings", undefined);
    expect(e.claimableWei).toBe("9410000000000000000");
    expect(e.contract).toBe("0xcdd2477387279c7d44a1053f44db5dac0fd8faef");
    // The response carries ONLY the single claimable — no per-source breakdown.
    expect(Object.keys(e).sort()).toEqual(["claimableWei", "contract", "walletAddress"]);
  });

  // CORE-C2-F-1 (@rule8) — the Claim button drives the REAL command. With a
  // non-zero claimable, claim() reads the on-chain value then invokes `user_claim`,
  // returning a REAL pending ceremony (approvable via sign_and_broadcast). It NEVER
  // fabricates a settlement or mutates a balance across the bridge.
  it("claim reads the on-chain claimable then invokes the REAL user_claim ceremony", async () => {
    earningsMock.claimableWei = "9410000000000000000";
    const bridge = createTauriBridge();
    const res = await bridge.agent.claim();
    // It routed to the REAL commands: agent_earnings (the read) + user_claim.
    expect(invokeMock).toHaveBeenCalledWith("agent_earnings", undefined);
    expect(invokeMock).toHaveBeenCalledWith("user_claim", undefined);
    expect(res.kind).toBe("ceremony");
    if (res.kind !== "ceremony") throw new Error("expected a real ceremony");
    // The returned ceremony is a legible claimRewards() Call — NOT a signature.
    expect(res.view.decoded.action).toContain("claimRewards");
    expect("sigHex" in res.view).toBe(false);
    // And that ceremony is approvable through the ONE real broadcast path (B1.4).
    const result = await bridge.signing.broadcast(res.view.id, false);
    expect(invokeMock).toHaveBeenCalledWith("sign_and_broadcast", { id: res.view.id, rawAck: false });
    expect(result.txHash).toMatch(/^0x[0-9a-f]{64}$/);
  });

  // CORE-C2-F-1 — an honest ZERO: nothing to claim → no ceremony, no user_claim
  // invoke, no faked settlement (Rule 1). The button surfaces "nothing to claim".
  it("claim with 0 on-chain claimable returns honest 'nothing' and does NOT invoke user_claim", async () => {
    earningsMock.claimableWei = "0";
    invokeMock.mockClear(); // isolate this assertion from prior tests' calls
    const bridge = createTauriBridge();
    const res = await bridge.agent.claim();
    expect(res.kind).toBe("nothing");
    expect(invokeMock).toHaveBeenCalledWith("agent_earnings", undefined);
    expect(invokeMock).not.toHaveBeenCalledWith("user_claim", undefined);
    earningsMock.claimableWei = "9410000000000000000"; // restore for other tests
  });
});

// CORE-C3 — the memory domain invokes the real mcp_serve daemon commands under
// the SidecarSupervisor. recall/search/neighbors/constellation return REAL
// parsed nodes from the per-user encrypted store; status carries the socket path
// but NEVER the store wrapping key. No sim graph is presented as live (Rule 1).
describe("tauri adapter — memory domain is wired to the real mcp_serve daemon (C3)", () => {
  it("status invokes memory_status, carries the socket path, NEVER the store key", async () => {
    const bridge = createTauriBridge();
    const st = await bridge.memory.status();
    expect(invokeMock).toHaveBeenCalledWith("memory_status", undefined);
    expect(st.socketPath).toBe("/tmp/memdag.sock");
    expect(st.semantic).toBe(false);
    // No key material ever crosses the bridge.
    expect(JSON.stringify(st)).not.toContain("key");
    expect(JSON.stringify(st)).not.toMatch(/[0-9a-f]{64}/i);
  });

  it("recall/search invoke the real tools and pass the tenant + parsed hits through", async () => {
    const bridge = createTauriBridge();
    const r = await bridge.memory.recall("chain-state", 15);
    expect(invokeMock).toHaveBeenCalledWith("memory_recall", { tenant: "chain-state", budget: 15 });
    expect(r.tenant).toBe("chain-state");
    expect(r.hits[0].title).toBe("LiquidStakingPool");
    await bridge.memory.search("personal", "telemetry");
    expect(invokeMock).toHaveBeenCalledWith("memory_search", { tenant: "personal", query: "telemetry", budget: undefined });
  });

  it("neighbors + constellation invoke real commands and never fabricate a graph", async () => {
    const bridge = createTauriBridge();
    const nbs = await bridge.memory.neighbors("personal", "0a1b2c3d4e");
    expect(invokeMock).toHaveBeenCalledWith("memory_neighbors", { tenant: "personal", idPrefix: "0a1b2c3d4e", budget: undefined });
    expect(nbs[0].direction).toBe("out");
    const graph = await bridge.memory.constellation();
    expect(invokeMock).toHaveBeenCalledWith("memory_constellation", { budget: undefined });
    // The real graph carries both tenants — personal + the REAL chain-state name.
    expect(graph.map((t) => t.tenant).sort()).toEqual(["chain-state", "personal"]);
  });

  it("memory.assert (a signed WRITE) is still honestly Unavailable until the ceremony WP", async () => {
    const bridge = createTauriBridge();
    await expect(bridge.memory.assert("x")).rejects.toSatisfy((e: unknown) => isUnavailable(e));
  });
});

// CORE-D3.C — the membership domain's `checkout()` invokes the real
// `membership_checkout` command (opens the in-app core-membership popup). It
// returns void — it does NOT report payment success; the money + grant are
// server-side and the store polls /userinfo. `entitlement()` stays Unavailable.
describe("tauri adapter — membership.checkout opens the real checkout popup (D3.C)", () => {
  it("checkout invokes membership_checkout and resolves to void (no settlement reported)", async () => {
    const bridge = createTauriBridge();
    invokeMock.mockClear();
    await expect(bridge.membership.checkout()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("membership_checkout", undefined);
  });

  it("membership.entitlement is still honestly Unavailable (only checkout is wired)", async () => {
    const bridge = createTauriBridge();
    await expect(bridge.membership.entitlement()).rejects.toSatisfy((e: unknown) => isUnavailable(e));
  });

  // BC-1.3 (@rule8) — grantStatus invokes the real membership_grant_status command
  // with the member address (the OIDC wallet_address claim) and decodes the REAL
  // on-chain grant. A PURE READ — no signing. The store settles S5 ONLY from this.
  it("grantStatus invokes membership_grant_status with { memberAddress } and decodes the real read", async () => {
    const bridge = createTauriBridge();
    invokeMock.mockClear();
    const status = await bridge.membership.grantStatus("0xabc");
    expect(invokeMock).toHaveBeenCalledWith("membership_grant_status", { memberAddress: "0xabc" });
    expect(status.attributedStakeWei).toBe((32000n * 10n ** 18n).toString());
    expect(status.hasSbt).toBe(true);
  });
});

// CORE-AI1 (@rule8) — the chat domain now invokes the real AI provider commands.
// Frontend half of the exfiltration boundary: setProvider hands the key to Rust
// ONCE (sealed in the OS keyring) and returns void — NO command result carries the
// key or the Authorization header; providerStatus returns metadata ONLY; infer
// returns the model completion (the STORED baseURL is used — the webview passes NO
// url). The Rust half (keyring/http seams, exfil-binding negative control) is
// `cargo test` (ai::tests).
describe("tauri adapter — chat domain wires real AI inference, key never leaks (AI1)", () => {
  beforeEach(() => {
    aiMock.map.clear();
    aiMock.default = null;
    invokeMock.mockClear();
  });

  it("setProvider invokes ai_set_provider with {providerId,baseUrl,model,apiKey} and returns void", async () => {
    const bridge = createTauriBridge();
    await expect(
      bridge.chat.setProvider("openai", "https://api.openai.com/v1", "gpt-4o-mini", "sk-live-KEY"),
    ).resolves.toBeUndefined();
    // The apiKey rides ONLY on the invoke ARGS (going INTO Rust) — never in a result.
    expect(invokeMock).toHaveBeenCalledWith("ai_set_provider", {
      providerId: "openai",
      baseUrl: "https://api.openai.com/v1",
      model: "gpt-4o-mini",
      apiKey: "sk-live-KEY",
    });
  });

  it("providerStatus returns metadata ONLY — never the key or an Authorization field", async () => {
    const bridge = createTauriBridge();
    await bridge.chat.setProvider("openai", "https://api.openai.com/v1", "gpt-4o", "sk-super-secret");
    const statuses = await bridge.chat.providerStatus();
    expect(invokeMock).toHaveBeenCalledWith("ai_provider_status", undefined);
    const openai = statuses.find((p) => p.id === "openai")!;
    expect(openai.configured).toBe(true);
    expect(openai.baseURL).toBe("https://api.openai.com/v1");
    expect(openai.model).toBe("gpt-4o");
    expect(openai.isDefault).toBe(true);
    // CRITICAL: the whole status payload carries NO key material (invariant 1).
    const json = JSON.stringify(statuses);
    expect(json).not.toContain("sk-super-secret");
    expect(json.toLowerCase()).not.toContain("apikey");
    expect(json).not.toContain("Authorization");
    // An unconfigured preset is honestly reported (never fabricated).
    expect(statuses.find((p) => p.id === "gateway")!.configured).toBe(false);
  });

  it("infer invokes ai_chat with {providerId,messagesJson,contextJson} and returns the completion (no url arg)", async () => {
    const bridge = createTauriBridge();
    await bridge.chat.setProvider("gateway", "https://infer.citrate.ai/v1", "gemma", "cgk_key");
    const messagesJson = JSON.stringify([{ role: "user", content: "hi" }]);
    const contextJson = JSON.stringify({ height: 1 });
    const out = await bridge.chat.infer("gateway", messagesJson, contextJson);
    // The command args have NO url field — the webview cannot supply an endpoint
    // (exfil-binding). The completion reflects the STORED baseURL.
    expect(invokeMock).toHaveBeenCalledWith("ai_chat", { providerId: "gateway", messagesJson, contextJson });
    const callArgs = invokeMock.mock.calls.find((c) => c[0] === "ai_chat")![1] as Record<string, unknown>;
    expect(Object.keys(callArgs).sort()).toEqual(["contextJson", "messagesJson", "providerId"]);
    expect(out).toContain("https://infer.citrate.ai/v1");
  });

  it("clearProvider invokes ai_clear_provider and an infer on the cleared id fails closed", async () => {
    const bridge = createTauriBridge();
    await bridge.chat.setProvider("openai", "https://api.openai.com/v1", "gpt-4o", "sk-abc12345");
    await bridge.chat.clearProvider("openai");
    expect(invokeMock).toHaveBeenCalledWith("ai_clear_provider", { providerId: "openai" });
    await expect(bridge.chat.infer("openai", "[]", "{}")).rejects.toBeTruthy();
  });

  it("chat.backend stays honestly Unavailable (demo/real selection lives in the store)", async () => {
    const bridge = createTauriBridge();
    await expect(bridge.chat.backend()).rejects.toSatisfy((e: unknown) => isUnavailable(e));
  });

  // Q-A.4a item 6 — the DEAD local-model wire is now live: bridge.chat.inferLocal
  // exists and invokes the REAL Rust `ai_chat_local` command with ONLY {messagesJson,
  // contextJson} (NO providerId, NO url — Rust owns the loopback endpoint, exfil-bound).
  it("chat.inferLocal exists and invokes ai_chat_local with ONLY {messagesJson,contextJson} (no url/provider)", async () => {
    const bridge = createTauriBridge();
    expect(typeof bridge.chat.inferLocal).toBe("function");
    const messagesJson = JSON.stringify([{ role: "user", content: "hi" }]);
    const contextJson = JSON.stringify({ height: 1 });
    const out = await bridge.chat.inferLocal(messagesJson, contextJson);
    expect(invokeMock).toHaveBeenCalledWith("ai_chat_local", { messagesJson, contextJson });
    const callArgs = invokeMock.mock.calls.find((c) => c[0] === "ai_chat_local")![1] as Record<string, unknown>;
    // Exfil-binding: NO providerId and NO url on the local path.
    expect(Object.keys(callArgs).sort()).toEqual(["contextJson", "messagesJson"]);
    expect(out).toContain("llama-server");
  });

  // Q-A.4a item 6 — the honest routing state passes the gateway-key presence to
  // Rust (model_inference_state), which decides local/gateway/demo. The webview
  // never fabricates "ready"; only Rust returns it (from real serve health).
  it("chat.inferenceState invokes model_inference_state with {gatewayConfigured}", async () => {
    const bridge = createTauriBridge();
    expect(await bridge.chat.inferenceState(true)).toBe("gateway-only");
    expect(invokeMock).toHaveBeenCalledWith("model_inference_state", { gatewayConfigured: true });
    expect(await bridge.chat.inferenceState(false)).toBe("demo");
  });
});

// CORE-BC-3 — the model domain invokes the real BC-3.1/3.2 commands. status
// returns the honest file-derived state (Ready ONLY after a real verify);
// download/verify/serveStart invoke their commands and return void. No secret
// ever crosses this boundary.
describe("tauri adapter — model domain is wired to the real download+verify+serve (BC-3)", () => {
  beforeEach(() => {
    modelMock.status = { state: "notPresent" };
    invokeMock.mockClear();
  });

  it("status invokes model_status and passes the honest file-derived state through", async () => {
    modelMock.status = { state: "downloading", downloadedBytes: 100, totalBytes: 5_335_289_824, pct: 0.000002 };
    const bridge = createTauriBridge();
    const st = await bridge.model.status();
    expect(invokeMock).toHaveBeenCalledWith("model_status", undefined);
    expect(st.state).toBe("downloading");
    if (st.state !== "downloading") throw new Error("narrow");
    expect(st.totalBytes).toBe(5_335_289_824);
    // No key/secret field is ever present.
    expect(JSON.stringify(st)).not.toContain("key");
  });

  it("status reports Ready only when the Rust side has EARNED it (a real verify)", async () => {
    // Presence-derived downloading is NOT ready; the ready state comes from Rust.
    modelMock.status = { state: "ready" };
    const bridge = createTauriBridge();
    const st = await bridge.model.status();
    expect(st.state).toBe("ready");
  });

  it("download/verify/serveStart invoke their commands and return void", async () => {
    const bridge = createTauriBridge();
    await expect(bridge.model.download()).resolves.toBeUndefined();
    await expect(bridge.model.verify()).resolves.toBeUndefined();
    await expect(bridge.model.serveStart()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("model_download", undefined);
    expect(invokeMock).toHaveBeenCalledWith("model_verify", undefined);
    expect(invokeMock).toHaveBeenCalledWith("model_serve_start", undefined);
  });
});

// W4 — the connections domain runs the real OAuth loopback flow through the Rust
// commands; the token is sealed in the vault and NEVER returned to the frontend.
describe("tauri adapter — connections domain runs real OAuth, token never leaks (W4)", () => {
  it("status invokes connection_status and returns all three services", async () => {
    const bridge = createTauriBridge();
    const st = await bridge.connections.status();
    expect(invokeMock).toHaveBeenCalledWith("connection_status", undefined);
    expect(st.map((c) => c.service).sort()).toEqual(["gdrive", "github", "notion"]);
  });

  it("start passes { service } and returns a connected status with NO token field", async () => {
    const bridge = createTauriBridge();
    const info = await bridge.connections.start("github");
    expect(invokeMock).toHaveBeenCalledWith("connection_start", { service: "github" });
    expect(info.connected).toBe(true);
    expect(info.scope).toBe("repo");
    // The whole result carries only connect facts — never a token/secret.
    expect(JSON.stringify(info)).not.toMatch(/token|secret|access/i);
  });

  it("disconnect invokes connection_disconnect with { service } and forgets it", async () => {
    const bridge = createTauriBridge();
    await bridge.connections.start("notion");
    await bridge.connections.disconnect("notion");
    expect(invokeMock).toHaveBeenCalledWith("connection_disconnect", { service: "notion" });
    const after = await bridge.connections.status();
    expect(after.find((c) => c.service === "notion")?.connected).toBe(false);
  });
});
