// GROW-S1 task 11 follow-up: the member's invite links are recovered from the join service
// (`join.list`), so leaving and coming back to a group (which remounts the panel) never loses the
// way to finish a reserved link or revoke a shared one.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { bridge } from "../bridge";
import type { JoinCodeEntry, JoinMinted } from "../bridge/domains";
import { YourInviteLinks } from "./InviteLinks";
import { codeActions, finishPendingCode, revokeCode } from "./inviteFlow";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PENDING: JoinCodeEntry = { code: "Ab3dE7gh", kind: "direct", status: "pending", exp: "2100-01-01T00:00:00.000Z", resolveCount: 0 };
const PUBLIC: JoinCodeEntry = { code: "Zz9yX8wv", kind: "referral", status: "valid", exp: "2100-01-01T00:00:00.000Z", resolveCount: 3 };
const SPENT: JoinCodeEntry = { code: "Hh4jK5mn", kind: "direct", status: "exhausted", exp: "2100-01-01T00:00:00.000Z", resolveCount: 50 };
// The share URL a finished link returns: its fragment is the one-time key, built at runtime.
const FRAG = btoa(String.fromCharCode(...Array.from({ length: 16 }, (_, i) => i))).replace(/=+$/, "");
const SHARE = `https://citrate.ai/join/${PENDING.code}#${FRAG}`;
const finished: JoinMinted = { code: PENDING.code, kind: "direct", status: "valid", exp: PENDING.exp, shareUrl: SHARE, attachError: null };
const src = (p: string) => readFileSync(resolve(__dirname, p), "utf8");

describe("codeActions", () => {
  it("a pending direct code can be finished or revoked; a live one revoked; a dead one neither", () => {
    expect(codeActions(PENDING)).toEqual(["finish", "revoke"]);
    expect(codeActions(PUBLIC)).toEqual(["revoke"]);
    expect(codeActions({ ...PUBLIC, kind: "direct" })).toEqual(["revoke"]);
    for (const status of ["expired", "revoked", "exhausted"] as const) {
      expect(codeActions({ ...PUBLIC, status }), status).toEqual([]);
    }
  });
});

describe("finishPendingCode / revokeCode", () => {
  const deps = (attach: () => Promise<JoinMinted>) => ({ join: { mint: vi.fn(), attach: vi.fn(attach), revoke: vi.fn(async () => undefined) } });

  it("returns the ready link, or still-pending, from attach", async () => {
    const d = deps(async () => finished);
    expect(await finishPendingCode(d, PENDING.code)).toEqual({ kind: "link", minted: finished });
    expect(d.join.attach).toHaveBeenCalledWith(PENDING.code);
    const p = { ...finished, status: "pending", shareUrl: null };
    expect(await finishPendingCode(deps(async () => p), PENDING.code)).toEqual({ kind: "pending", minted: p });
  });

  it("a link this app run no longer holds says to revoke it and make a new one", async () => {
    const out = await finishPendingCode(deps(async () => { throw new Error("join: invite not found"); }), PENDING.code);
    expect(out.kind).toBe("error");
    if (out.kind === "error") expect(out.message).toMatch(/revoke it and create a new one/i);
  });

  it("other attach failures read as join errors", async () => {
    const out = await finishPendingCode(deps(async () => { throw new Error("join: invite links are temporarily unavailable"); }), PENDING.code);
    expect(out).toEqual({ kind: "error", message: "Short invite links are not available right now. Try again later." });
  });

  it("revokeCode revokes that code at the service", async () => {
    const d = deps(async () => finished);
    await revokeCode(d, PUBLIC.code);
    expect(d.join.revoke).toHaveBeenCalledWith(PUBLIC.code);
  });
});

describe("YourInviteLinks (mounted)", () => {
  let host: HTMLDivElement;
  let root: Root;
  let toasts: string[];
  let list: ReturnType<typeof vi.spyOn>;
  let attach: ReturnType<typeof vi.spyOn>;
  let revoke: ReturnType<typeof vi.spyOn>;
  const store = { toast: (t: string) => void toasts.push(t) };
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
  const mount = async () => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => root.render(<YourInviteLinks store={store} />));
    await act(async () => {});
  };
  const unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  const click = async (id: string) => {
    const el = q(id);
    expect(el, id).toBeTruthy();
    await act(async () => el!.click());
    await act(async () => {});
  };

  beforeEach(() => {
    toasts = [];
    list = vi.spyOn(bridge.join, "list").mockResolvedValue([PENDING, PUBLIC, SPENT]);
    attach = vi.spyOn(bridge.join, "attach").mockResolvedValue(finished);
    revoke = vi.spyOn(bridge.join, "revoke").mockResolvedValue(undefined);
  });
  afterEach(() => vi.restoreAllMocks());

  it("lists the member's codes from the service with what can be done to each", async () => {
    await mount();
    expect(list).toHaveBeenCalledTimes(1);
    expect(q(`your-link-${PENDING.code}`)).toBeTruthy();
    expect(q(`your-link-finish-${PENDING.code}`)).toBeTruthy();
    expect(q(`your-link-revoke-${PUBLIC.code}`)).toBeTruthy();
    expect(q(`your-link-${SPENT.code}`)).toBeTruthy();
    expect(q(`your-link-revoke-${SPENT.code}`)).toBeNull();
    await unmount();
  });

  it("after leaving and coming back, a reserved link can still be finished and a public one revoked", async () => {
    await mount();
    await unmount();
    await mount();
    expect(list).toHaveBeenCalledTimes(2);
    await click(`your-link-finish-${PENDING.code}`);
    expect(attach).toHaveBeenCalledWith(PENDING.code);
    expect(host.textContent).toContain(SHARE);
    await click(`your-link-revoke-${PUBLIC.code}`);
    expect(revoke).toHaveBeenCalledWith(PUBLIC.code);
    expect(list).toHaveBeenCalledTimes(3);
    expect(toasts.join("\n")).not.toContain(FRAG);
    await unmount();
  });

  it("a list failure is shown honestly with a way to try again", async () => {
    list.mockRejectedValueOnce(new Error("join: sign in to create invite links"));
    await mount();
    expect(host.textContent).toMatch(/sign in/i);
    await click("your-links-reload");
    expect(list).toHaveBeenCalledTimes(2);
    expect(q(`your-link-${PENDING.code}`)).toBeTruthy();
    await unmount();
  });
});

describe("where the recovered list is shown", () => {
  it("Groups and Community both render it", () => {
    expect(src("./Groups.tsx")).toMatch(/<YourInviteLinks\b/);
    expect(src("./Community.tsx")).toMatch(/<YourInviteLinks\b/);
  });
});
