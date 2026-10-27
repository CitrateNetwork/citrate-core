// Settings › Billing for a free (non-member, non-lapsed) user (follow-up to #576):
//   - the button is "Become a member ↗" and runs the guarded checkout (store.becomeMember);
//   - a lapsed member keeps "Renew ↗" (store.renewMembership);
//   - the stake-attribution / vaulted-principal note shows only to someone with a grant;
//   - no price figure on the billing card for a free user (the price is on the checkout).
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings } from "./Settings";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function makeStore() {
  const spies = { becomeMember: vi.fn(async () => {}), renewMembership: vi.fn(async () => {}) };
  const identity = () => ({ name: "M", initials: "M", email: "m@example.com", sub: "s", wallet: "0xabc", tier: "free", role: "member", org: null, real: true });
  const store = new Proxy(spies as Record<string, unknown>, {
    get: (t, k: string) => (k in t ? t[k] : k === "identity" ? identity : () => undefined),
  }) as unknown as Store;
  return { store, spies };
}

function billing(patch: Partial<AppState>): AppState {
  const s = freshState("p1");
  s.sSec = "billing";
  s.signedIn = true;
  return Object.assign(s, patch);
}

const FREE: Partial<AppState> = { tier: "free", entitlement: "active", hasGrant: false, hasSbt: false };
const LAPSED: Partial<AppState> = { tier: "free", entitlement: "lapsed", hasGrant: true };
const MEMBER_NO_GRANT: Partial<AppState> = { tier: "pilot", entitlement: "active", hasGrant: false, hasSbt: false };
const MEMBER_GRANTED: Partial<AppState> = { tier: "pilot", entitlement: "active", hasGrant: true };

const text = (html: string) => html.replace(/<[^>]*>/g, " ");

async function clickBilling(s: AppState, re: RegExp) {
  const { store, spies } = makeStore();
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<Settings store={store} s={s} />));
  const btn = Array.from(host.querySelectorAll("button")).find((b) => re.test(b.textContent ?? ""));
  expect(btn).toBeTruthy();
  await act(async () => btn!.click());
  root.unmount();
  return spies;
}

describe("Settings › Billing for a free user", () => {
  it("says 'Become a member ↗', not 'Renew'", () => {
    const t = text(renderToStaticMarkup(<Settings store={makeStore().store} s={billing(FREE)} />));
    expect(t).toContain("Become a member ↗");
    expect(t).not.toMatch(/Renew/);
  });

  it("the button runs the guarded checkout (becomeMember), not renewal", async () => {
    const spies = await clickBilling(billing(FREE), /Become a member/);
    expect(spies.becomeMember).toHaveBeenCalledTimes(1);
    expect(spies.renewMembership).not.toHaveBeenCalled();
  });

  it("shows no stake-attribution / vaulted-principal note and no price", () => {
    const t = text(renderToStaticMarkup(<Settings store={makeStore().store} s={billing(FREE)} />));
    expect(t).not.toMatch(/Stake attribution|vaulted principal/i);
    expect(t).not.toMatch(/\$\s?\d|\bUSD\s?\d|\d\s?USD\b/);
  });

  it("a member without a grant sees no stake-attribution note either", () => {
    const t = text(renderToStaticMarkup(<Settings store={makeStore().store} s={billing(MEMBER_NO_GRANT)} />));
    expect(t).not.toMatch(/Stake attribution|vaulted principal/i);
  });
});

describe("Settings › Billing for members", () => {
  it("a lapsed member keeps 'Renew ↗', which renews", async () => {
    const t = text(renderToStaticMarkup(<Settings store={makeStore().store} s={billing(LAPSED)} />));
    expect(t).toContain("Renew ↗");
    expect(t).not.toContain("Become a member");
    const spies = await clickBilling(billing(LAPSED), /Renew/);
    expect(spies.renewMembership).toHaveBeenCalledTimes(1);
    expect(spies.becomeMember).not.toHaveBeenCalled();
  });

  it("a member with a grant still sees the stake-attribution note", () => {
    const t = text(renderToStaticMarkup(<Settings store={makeStore().store} s={billing(MEMBER_GRANTED)} />));
    expect(t).toMatch(/Stake attribution/);
  });
});
