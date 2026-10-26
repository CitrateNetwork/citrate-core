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

  it("mint and resolve outlive two 15 s Rust-side HTTP calls", () => {
    expect(deadlineFor("join_mint")).toBeGreaterThan(30_000);
    expect(deadlineFor("join_resolve")).toBeGreaterThan(30_000);
    expect(deadlineFor("join_attach")).toBeGreaterThan(15_000);
    expect(deadlineFor("join_redeem")).toBe(INVOKE_TIMEOUT_MS);
  });
});
