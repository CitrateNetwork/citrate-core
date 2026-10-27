// core#431 — the Groups invite list says when the saved invites can't be read, instead of showing
// an empty list. Written red-first against 9773d25f.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { readOwnerInvites } from "./ownerInvites";
import type { PendingInvite } from "../bridge/domains";

const ONE: PendingInvite = { group: "g", token: "t", forHandle: "h", createdAt: 1, link: "citrate://invite?g=g&t=t" };
const DAMAGED = "Your saved invite list is damaged and can't be read. Creating an invite sets the damaged file aside and starts a new list.";

describe("readOwnerInvites", () => {
  it("a read list is shown, with no error", async () => {
    expect(await readOwnerInvites(async () => [ONE])).toEqual({ invites: [ONE], error: null });
  });

  it("a store that can't be read is an error with the app's reason, not an empty list", async () => {
    const r = await readOwnerInvites(async () => {
      throw new Error(DAMAGED);
    });
    expect(r.invites).toEqual([]);
    expect(r.error).toBe(DAMAGED);
  });

  it("an invoke rejection that is a plain string keeps its text", async () => {
    const r = await readOwnerInvites(() => Promise.reject(DAMAGED));
    expect(r.error).toBe(DAMAGED);
  });

  it("an unexplained failure is still an error, never silence", async () => {
    const r = await readOwnerInvites(() => Promise.reject(undefined));
    expect(r.error).toBeTruthy();
  });
});

describe("Groups reads its invite list through readOwnerInvites and shows the error", () => {
  const src = readFileSync(join(__dirname, "Groups.tsx"), "utf8");
  it("refreshInvites uses readOwnerInvites", () => {
    const start = src.indexOf("const refreshInvites = async");
    expect(start).toBeGreaterThan(-1);
    const body = src.slice(start, src.indexOf("};", start));
    expect(body).toContain("readOwnerInvites(");
    expect(body).not.toContain("setPendingInvites(await bridge.invites.list(");
  });
  it("the invite card renders the read error", () => {
    expect(src).toMatch(/\{invitesError && \(/);
  });
});
