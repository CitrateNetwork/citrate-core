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
    // CORE-C1.2 node-agent — under the SidecarSupervisor. status returns the
    // supervisor state + whether a bearer session exists (NEVER the token);
    // start/stop return void. No secret ever crosses this boundary.
    case "agent_status":
      return { state: "running", authed: true };
    case "agent_start":
    case "agent_stop":
      return undefined;
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

describe("tauri adapter — unwired domains are honestly Unavailable (Rule 1)", () => {
  it("wallet.balances rejects with Unavailable, never fabricated data", async () => {
    const bridge = createTauriBridge();
    await expect(bridge.wallet.balances()).rejects.toSatisfy((e: unknown) => isUnavailable(e));
  });

  it("every still-unwired seam domain rejects with Unavailable", async () => {
    const bridge = createTauriBridge();
    // NOTE: `auth` (CORE-A3) and `node` (CORE-C1.1) are now genuinely wired and
    // are asserted separately.
    const calls = [
      () => bridge.memory.recall("x"),
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
});
