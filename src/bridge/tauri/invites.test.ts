// GROW-S2 (Join 9, M3) review: the redeem bridge returns the daemon's answer as given, so the toast
// never says "You're in" for a rendezvous join that only reached Waiting.
import { describe, it, expect, vi } from "vitest";

const answer = { path: "rendezvous", join: { group: "g", name: "n", state: "waiting", reason: null, expiresAt: 1, graceUntil: 2, updatedAt: 3 } };
const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
vi.mock("./invoke", async (orig) => {
  const real = await orig<typeof import("./invoke")>();
  return {
    ...real,
    invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return cmd === "group_invite_redeem" ? answer : null;
    }),
  };
});

import { tauriInvites } from "./invites";

describe("tauriInvites.redeem", () => {
  it("calls group_invite_redeem with the link and passes the answer through", async () => {
    const r = await tauriInvites.redeem("citrate://invite?g=g&t=t");
    expect(calls).toEqual([{ cmd: "group_invite_redeem", args: { link: "citrate://invite?g=g&t=t" } }]);
    expect(r).toEqual(answer);
  });
});
