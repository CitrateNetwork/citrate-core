// GROW-S1 task 11 — the inviter side: a direct invite (relay invite sealed into a short link), a public
// link (referral code, display only), revoke, and the honest fall back to the app-only link when the
// short-link service is unavailable. The logic is tested with fake domains; the view per state.
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { createDirectInvite, createPublicLink, revokeDirectInvite, type InviteDeps } from "./inviteFlow";
import { InviteLinksView, copyLink, type InviteLinksViewProps } from "./InviteLinks";
import type { JoinMinted } from "../bridge/domains";

const SHARE = "https://citrate.ai/join/Ab3dE7gh#AAECAwQFBgcICQoLDA0ODw";
const minted: JoinMinted = { code: "Ab3dE7gh", kind: "direct", status: "valid", exp: "2026-11-01T00:00:00.000Z", shareUrl: SHARE, attachError: null };
const RELAY = "citrate://invite?g=grp_1&t=tok&k=02ab";

function deps(over: Partial<{ mint: InviteDeps["join"]["mint"]; create: InviteDeps["invites"]["create"] }> = {}) {
  const d = {
    invites: {
      create: over.create ?? vi.fn(async () => ({ token: "tok", link: RELAY })),
      revoke: vi.fn(async () => undefined),
    },
    join: {
      mint: over.mint ?? vi.fn(async () => minted),
      attach: vi.fn(async () => minted),
      revoke: vi.fn(async () => undefined),
    },
  };
  return d as unknown as InviteDeps & { invites: { create: ReturnType<typeof vi.fn>; revoke: ReturnType<typeof vi.fn> }; join: { mint: ReturnType<typeof vi.fn>; revoke: ReturnType<typeof vi.fn> } };
}

describe("createDirectInvite", () => {
  it("creates the relay invite, then seals it into a short link (direct)", async () => {
    const d = deps();
    const out = await createDirectInvite(d, "grp_1", "Garden club", "storage");
    expect(d.invites.create).toHaveBeenCalledWith("grp_1", "shared link");
    expect(d.join.mint).toHaveBeenCalledWith({ kind: "direct", clusterName: "Garden club", goal: "storage" }, RELAY);
    expect(out).toEqual({ kind: "link", minted, relayToken: "tok" });
  });

  it("the short link carries no group id and no wallet address", async () => {
    const out = await createDirectInvite(deps(), "grp_1", "Garden club", "general");
    expect(out.kind).toBe("link");
    if (out.kind === "link") {
      expect(out.minted.shareUrl).not.toContain("grp_1");
      expect(out.minted.shareUrl).not.toMatch(/0x[0-9a-f]{40}/i);
    }
  });

  it("when the short-link service is unavailable it falls back to the app-only invite, honestly", async () => {
    const d = deps({ mint: vi.fn(async () => { throw new Error("join: invite links are temporarily unavailable"); }) });
    const out = await createDirectInvite(d, "grp_1", "Garden club", "general");
    expect(out).toEqual({ kind: "fallback", relayLink: RELAY, relayToken: "tok" });
  });

  it("a rejected name is reported with the service's rule and the unused relay invite is revoked", async () => {
    const d = deps({ mint: vi.fn(async () => { throw new Error('join: that cluster name isn\'t allowed on an invite. Use letters from one of Latin, Greek, Cyrillic or Cherokee (not mixed), digits and spaces, and avoid reserved words such as "official" or "support"'); }) });
    const out = await createDirectInvite(d, "grp_1", "Сад", "general");
    expect(out.kind).toBe("error");
    if (out.kind === "error") expect(out.message).toMatch(/one of Latin, Greek, Cyrillic or Cherokee \(not mixed\), digits and spaces/);
    expect(d.invites.revoke).toHaveBeenCalledWith("grp_1", "tok");
  });

  it("a pending (reserved, not attached) code is reported for retry, with no share link", async () => {
    const pending = { ...minted, status: "pending" as const, shareUrl: null, attachError: "join: could not reach the invite service" };
    const out = await createDirectInvite(deps({ mint: vi.fn(async () => pending) }), "grp_1", "Garden club", "general");
    expect(out).toEqual({ kind: "pending", minted: pending, relayToken: "tok" });
  });

  it("if the relay invite itself can't be made, nothing is minted", async () => {
    const d = deps({ create: vi.fn(async () => { throw new Error("comms daemon not running"); }) });
    const out = await createDirectInvite(d, "grp_1", "Garden club", "general");
    expect(out.kind).toBe("error");
    expect(d.join.mint).not.toHaveBeenCalled();
  });
});

describe("createPublicLink / revokeDirectInvite", () => {
  it("a public link is a referral code, with no relay invite behind it", async () => {
    const ref = { ...minted, kind: "referral" as const, shareUrl: "https://citrate.ai/join/Zz9yX8wv" };
    const d = deps({ mint: vi.fn(async () => ref) });
    const out = await createPublicLink(d, "Garden club", "general");
    expect(d.join.mint).toHaveBeenCalledWith({ kind: "referral", clusterName: "Garden club", goal: "general" });
    expect(d.invites.create).not.toHaveBeenCalled();
    expect(out).toEqual({ kind: "link", minted: ref });
  });

  it("a public link has no fallback to the older self-built format", async () => {
    const d = deps({ mint: vi.fn(async () => { throw new Error("join: invite links are temporarily unavailable"); }) });
    const out = await createPublicLink(d, "Garden club", "general");
    expect(out.kind).toBe("error");
    if (out.kind === "error") expect(out.message).toMatch(/not available right now/i);
  });

  it("revoking a direct invite revokes the short code and the relay invite inside it", async () => {
    const d = deps();
    await revokeDirectInvite(d, "grp_1", "Ab3dE7gh", "tok");
    expect(d.join.revoke).toHaveBeenCalledWith("Ab3dE7gh");
    expect(d.invites.revoke).toHaveBeenCalledWith("grp_1", "tok");
  });
});

const noop = () => {};
const base: InviteLinksViewProps = {
  direct: null,
  pub: null,
  busy: null,
  goal: "general",
  qr: {},
  onGoal: noop,
  onCreateDirect: noop,
  onCreatePublic: noop,
  onCopy: noop,
  onRevokeDirect: noop,
  onRevokePublic: noop,
  onRetryAttach: noop,
};
const view = (p: Partial<InviteLinksViewProps>) => renderToStaticMarkup(<InviteLinksView {...base} {...p} />);

describe("InviteLinksView", () => {
  it("offers both links before anything is made", () => {
    const html = view({});
    expect(html).toContain("Create an invite link");
    expect(html).toContain("Create a public link");
  });

  it("a direct link shows the short link, copy, expiry, revoke and a QR code", () => {
    const html = view({ direct: { kind: "link", minted, relayToken: "tok" }, qr: { [SHARE]: { size: 3, rows: ["101", "010", "111"] } } });
    expect(html).toContain("https://citrate.ai/join/Ab3dE7gh#");
    expect(html).toContain('data-testid="invite-direct-copy"');
    expect(html).toContain('data-testid="invite-direct-revoke"');
    expect(html).toContain('data-testid="invite-direct-qr"');
    expect(html).toMatch(/Expires/);
  });

  it("the fallback says plainly that this link only opens in the app", () => {
    const html = view({ direct: { kind: "fallback", relayLink: RELAY, relayToken: "tok" } });
    expect(html).toMatch(/not available right now/);
    expect(html).toMatch(/already have Citrate/);
    expect(html).toContain('data-testid="invite-direct-copy"');
  });

  it("a pending code offers Retry, not a link", () => {
    const pending = { ...minted, status: "pending" as const, shareUrl: null, attachError: "join: could not reach the invite service" };
    const html = view({ direct: { kind: "pending", minted: pending, relayToken: "tok" } });
    expect(html).toContain('data-testid="invite-direct-retry"');
    expect(html).not.toContain("https://citrate.ai/join/");
  });

  it("an error is shown as text", () => {
    expect(view({ direct: { kind: "error", message: "Use letters from one script" } })).toContain("Use letters from one script");
  });

  it("a public link shows its short link with no group id or address", () => {
    const pubUrl = "https://citrate.ai/join/Zz9yX8wv";
    const html = view({ pub: { kind: "link", minted: { ...minted, kind: "referral", shareUrl: pubUrl } } });
    expect(html).toContain(pubUrl);
    expect(html).toContain('data-testid="invite-public-revoke"');
    expect(html).not.toMatch(/[?&](by|ref|c|g)=/);
  });
});

describe("copyLink", () => {
  it("never repeats the link (code or key) in a toast, whether the copy works or not", async () => {
    const toasts: string[] = [];
    const s = { toast: (t: string) => void toasts.push(t) };
    const tick = () => new Promise((r) => setTimeout(r, 0));
    try {
      vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn(async () => undefined) } });
      copyLink(s, SHARE);
      await tick();
      vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn(async () => { throw new Error("denied"); }) } });
      copyLink(s, SHARE);
      await tick();
      vi.stubGlobal("navigator", {});
      copyLink(s, SHARE);
    } finally {
      vi.unstubAllGlobals();
    }
    expect(toasts).toEqual(["Link copied.", "Couldn't copy. Select the link and copy it.", "Couldn't copy. Select the link and copy it."]);
    const fragment = SHARE.split("#")[1];
    for (const t of toasts) {
      expect(t).not.toContain("Ab3dE7gh");
      expect(t).not.toContain(fragment);
    }
  });
});
