// BC-6.3 (Rule 1 — display honesty) — the Settings "Memberships & billing" card
// must render the REAL entitlement expiry folded from the /userinfo `expires_at`
// claim (state.authExpiresAt), NEVER a hardcoded date.
//
// Before the fix the account "expiresAt" row and the billing renewal line rendered
// HARDCODED literals ("2027-07-11", "2026-07-25 · 14 days", "2026-06-28"). We
// assert:
//   - the REAL folded expiry (a distinct value, "2026-11-30") appears, and the old
//     hardcoded "2027-07-11" does NOT (a negative control proving it is claim-
//     derived, not a constant);
//   - an honest "—" renders when the claim is null/absent (no fabricated date);
//   - the negative control bites: changing the claim changes the rendered value.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings, fmtExpiresAt } from "./Settings";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// The billing card renders from `s` (AppState) + a render-time `store.identity()`
// (the account claim rows). Static server render never fires the useEffect hooks.
const stubStore = {
  identity: () => ({
    name: "Member",
    initials: "M",
    email: "member@example.com",
    sub: "sub-1",
    wallet: "0xC0ffee0000000000000000000000000000ABCDEF",
    tier: "pilot",
    role: "member",
    org: null,
    real: true,
  }),
} as unknown as Store;

function billingState(expiresAt: string | null, entitlement: AppState["entitlement"] = "active"): AppState {
  const s = freshState("p1");
  s.sSec = "billing";
  s.signedIn = true;
  s.tier = "pilot";
  s.entitlement = entitlement;
  s.authExpiresAt = expiresAt;
  return s;
}

describe("Settings billing — BC-6.3 real expiresAt", () => {
  it("renders the REAL folded expiresAt from the claim, not a hardcoded date", () => {
    // A DELIBERATELY distinct real expiry. If the card were hardcoded this would
    // fail — the value must trace to the folded /userinfo `expires_at` claim.
    const html = renderToStaticMarkup(<Settings store={stubStore} s={billingState("2026-11-30")} />);
    expect(html).toContain("2026-11-30");
    // The old hardcoded literals must be GONE (negative controls).
    expect(html).not.toContain("2027-07-11");
    expect(html).not.toContain("2026-07-25");
    expect(html).not.toContain("2026-06-28");
  });

  it("renders '—' (never a fabricated date) when the expiry claim is null", () => {
    const html = renderToStaticMarkup(<Settings store={stubStore} s={billingState(null)} />);
    expect(html).not.toContain("2027-07-11");
    // The account claim row shows "—" for an absent expiry.
    expect(html).toContain("—");
  });

  it("is claim-derived — a different claim value renders a different date (negative control)", () => {
    const a = renderToStaticMarkup(<Settings store={stubStore} s={billingState("2028-01-15")} />);
    const b = renderToStaticMarkup(<Settings store={stubStore} s={billingState("2029-09-09")} />);
    expect(a).toContain("2028-01-15");
    expect(a).not.toContain("2029-09-09");
    expect(b).toContain("2029-09-09");
    expect(b).not.toContain("2028-01-15");
  });

  it("a lapsed entitlement still shows the REAL expiry, not a hardcoded lapse date", () => {
    // A DISTINCT lapse expiry so the POSITIVE assertion bites: if the lapsed line
    // stopped being claim-derived (e.g. a hardcode mutation) this value would vanish.
    const html = renderToStaticMarkup(
      <Settings store={stubStore} s={billingState("2026-05-01", "lapsed")} />,
    );
    // POSITIVE: the real claim-derived expiry is rendered in the lapsed case.
    expect(html).toContain("2026-05-01");
    // NEGATIVE controls: the old hardcoded lapse/renewal literals stay gone.
    expect(html).not.toContain("2026-06-28");
    expect(html).not.toContain("2027-07-11");
  });
});

// F1 — the render-layer expiry formatter. A raw epoch must never reach the UI as a
// bare integer; ISO passes through as YYYY-MM-DD; absent stays the honest "—".
describe("fmtExpiresAt — F1 render-layer expiry normalisation", () => {
  it("renders epoch-milliseconds as a YYYY-MM-DD date, not the raw integer", () => {
    // 1783939200000 ms = 2026-07-13 (UTC). Must not render as the raw integer.
    const out = fmtExpiresAt("1783939200000");
    expect(out).toBe("2026-07-13");
    expect(out).not.toContain("1783939200000");
    expect(/^\d+$/.test(out)).toBe(false);
  });

  it("renders epoch-seconds (< 1e12) as a YYYY-MM-DD date", () => {
    // 1783939200 s = 2026-07-13 (UTC).
    expect(fmtExpiresAt("1783939200")).toBe("2026-07-13");
  });

  it("passes an ISO/date-ish string through, normalised to YYYY-MM-DD", () => {
    expect(fmtExpiresAt("2026-11-30")).toBe("2026-11-30");
    expect(fmtExpiresAt("2026-11-30T12:34:56Z")).toBe("2026-11-30");
  });

  it("returns the honest '—' for null / empty (never a fabricated date)", () => {
    expect(fmtExpiresAt(null)).toBe("—");
    expect(fmtExpiresAt(undefined)).toBe("—");
    expect(fmtExpiresAt("")).toBe("—");
    expect(fmtExpiresAt("   ")).toBe("—");
  });
});
