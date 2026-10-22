// GROW-S1 task 10 — the v2 invite-link bridge calls the Rust commands with the argument names
// Tauri maps to the Rust parameters, and gives the join commands deadlines above the Rust bounds.
import { describe, it, expect, vi, beforeEach } from "vitest";

const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
vi.mock("./invoke", async (orig) => {
  const real = await orig<typeof import("./invoke")>();
  return {
    ...real,
    invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return null;
    }),
  };
});

import { tauriJoin } from "./join";
import { deadlineFor, INVOKE_TIMEOUT_MS } from "./invoke";

describe("GROW-S1 join bridge", () => {
  beforeEach(() => {
    calls.length = 0;
  });

  it("maps each method to its join_* command and argument names", async () => {
    const req = { kind: "direct" as const, clusterName: "Garden club", goal: "storage" as const };
    await tauriJoin.mint(req, "citrate://invite?g=a&t=b&k=c");
    await tauriJoin.mint({ ...req, kind: "referral" });
    await tauriJoin.attach("Ab3dE7gh");
    await tauriJoin.redeem("Ab3dE7gh");
    await tauriJoin.revoke("Ab3dE7gh");
    await tauriJoin.list();
    expect(calls).toEqual([
      { cmd: "join_mint", args: { request: req, inviteLink: "citrate://invite?g=a&t=b&k=c" } },
      { cmd: "join_mint", args: { request: { ...req, kind: "referral" }, inviteLink: null } },
      { cmd: "join_attach", args: { code: "Ab3dE7gh" } },
      { cmd: "join_redeem", args: { code: "Ab3dE7gh" } },
      { cmd: "join_revoke", args: { code: "Ab3dE7gh" } },
      { cmd: "join_list", args: undefined },
    ]);
  });

  it("task 12: the pending-invite commands and the share QR map to their Rust commands", async () => {
    await tauriJoin.pendingSet("citrate://join/Ab3dE7gh#AAECAwQFBgcICQoLDA0ODw");
    await tauriJoin.pendingGet();
    await tauriJoin.pendingResolve();
    await tauriJoin.pendingClear();
    await tauriJoin.shareQr("https://citrate.ai/join/Ab3dE7gh");
    expect(calls).toEqual([
      { cmd: "pending_join_set", args: { link: "citrate://join/Ab3dE7gh#AAECAwQFBgcICQoLDA0ODw" } },
      { cmd: "pending_join_get", args: undefined },
      { cmd: "pending_join_resolve", args: undefined },
      { cmd: "pending_join_clear", args: undefined },
      { cmd: "join_share_qr", args: { link: "https://citrate.ai/join/Ab3dE7gh" } },
    ]);
    expect(deadlineFor("pending_join_resolve")).toBeGreaterThan(30_000);
  });

  it("mint and resolve outlive two 15 s Rust-side HTTP calls", () => {
    expect(deadlineFor("join_mint")).toBeGreaterThan(30_000);
    expect(deadlineFor("pending_join_resolve")).toBeGreaterThan(30_000);
    expect(deadlineFor("join_attach")).toBeGreaterThan(15_000);
    expect(deadlineFor("join_redeem")).toBe(INVOKE_TIMEOUT_MS);
  });

  it("only the held invite is resolved: there is no direct resolve call or deadline", () => {
    expect("resolve" in tauriJoin).toBe(false);
    expect(deadlineFor("join_resolve")).toBe(INVOKE_TIMEOUT_MS);
  });
});
