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
      if (cmd === "join_redeem") return { path: "rendezvous", join: { group: "g", name: "n", state: "waiting", reason: null, expiresAt: 1, graceUntil: 2, updatedAt: 3 } };
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
    await tauriJoin.resolve("https://citrate.ai/join/Ab3dE7gh");
    await tauriJoin.redeem("Ab3dE7gh");
    await tauriJoin.revoke("Ab3dE7gh");
    await tauriJoin.list();
    expect(calls).toEqual([
      { cmd: "join_mint", args: { request: req, inviteLink: "citrate://invite?g=a&t=b&k=c" } },
      { cmd: "join_mint", args: { request: { ...req, kind: "referral" }, inviteLink: null } },
      { cmd: "join_attach", args: { code: "Ab3dE7gh" } },
      { cmd: "join_resolve", args: { link: "https://citrate.ai/join/Ab3dE7gh" } },
      { cmd: "join_redeem", args: { code: "Ab3dE7gh" } },
      { cmd: "join_revoke", args: { code: "Ab3dE7gh" } },
      { cmd: "join_list", args: undefined },
    ]);
  });

  it("redeem hands back which path ran and the daemon's join view", async () => {
    const r = await tauriJoin.redeem("Ab3dE7gh");
    expect(r.path).toBe("rendezvous");
    if (r.path === "rendezvous") expect(r.join.state).toBe("waiting");
  });

  it("mint and resolve outlive two 15 s Rust-side HTTP calls", () => {
    expect(deadlineFor("join_mint")).toBeGreaterThan(30_000);
    expect(deadlineFor("join_resolve")).toBeGreaterThan(30_000);
    expect(deadlineFor("join_attach")).toBeGreaterThan(15_000);
    // GROW-S2 M3: the rendezvous join plus, on the §12 fallback, the legacy redeem.
    expect(deadlineFor("join_redeem")).toBeGreaterThan(16_000);
    expect(deadlineFor("join_redeem")).toBeGreaterThan(INVOKE_TIMEOUT_MS);
  });
});
