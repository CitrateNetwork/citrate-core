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
    default:
      // seam domains reject with the honest unavailable message
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

  it("every seam domain rejects with Unavailable", async () => {
    const bridge = createTauriBridge();
    const calls = [
      () => bridge.auth.userinfo(),
      () => bridge.node.status(),
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
