// Q-A.1 — the Settings honesty pass (Rule 1 / I-3 display honesty).
//
// The #1 owner complaint was "excuses and notes baked into the UI": Settings
// controls that only existed to toast a fabricated apology ("…isn't wired yet, a
// scheduled build") or asserted an unverified fact ("healthy · TLS", a fake
// receipt date). Every control must be ONE of: a REAL working action, an
// honestly DISABLED + annotated control, or removed.
//
// These tests are the RED-then-GREEN tripwire: they FAIL if any known fabricated
// string renders anywhere in the Settings surface, across every sub-section. They
// prove the honesty holds and bite if a future edit reintroduces an excuse.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings } from "./Settings";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

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

const SECTIONS: AppState["sSec"][] = [
  "account",
  "connections",
  "ai",
  "node",
  "api",
  "keys",
  "billing",
  "app",
];

function sectionState(sSec: AppState["sSec"], patch: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.sSec = sSec;
  s.signedIn = true;
  s.tier = "pilot";
  s.entitlement = "active";
  s.authExpiresAt = "2026-11-30";
  s.hasSbt = true;
  return { ...s, ...patch };
}

// The exact excuse / hardcoded-fact strings the audit flagged. A control that
// only exists to toast one of these is a lie baked into the UI — it must become
// a disabled state or a real action, never render this copy.
const FABRICATED = [
  "isn't wired yet",
  "a scheduled build",
  "healthy · TLS",
  "2026-07-11 · Pilot membership · $48.00",
  "updater signature verified offline",
  "citrate-core:// scheme registered",
  "ADDRESS SET",
];

describe("Settings honesty pass — no fabricated excuses or hardcoded facts render (Q-A.1)", () => {
  for (const sSec of SECTIONS) {
    it(`section "${sSec}" renders no fabricated / hardcoded string`, () => {
      const html = renderToStaticMarkup(<Settings store={stubStore} s={sectionState(sSec)} />);
      for (const bad of FABRICATED) {
        expect(html).not.toContain(bad);
      }
    });
  }

  it("the file header no longer claims the false 'no dead controls / not wired' pattern", async () => {
    // Read the source so the tripwire also covers the top-of-file comment the
    // audit called out as 'currently false'.
    const fs = await import("node:fs");
    const path = await import("node:path");
    const src = fs.readFileSync(path.resolve(process.cwd(), "src/surfaces/Settings.tsx"), "utf8");
    expect(src).not.toContain("no dead controls, no \"declared but not");
    // The corrected header must state the true honesty contract.
    expect(src).toContain("honestly DISABLED + annotated control");
  });

  it("the RPC health line only asserts health from a real probe (never an unconditional 'healthy · TLS')", () => {
    const html = renderToStaticMarkup(<Settings store={stubStore} s={sectionState("api", { rpc: "public" })} />);
    // Before the fix the public RPC always read "rpc.citrate.ai · healthy · TLS"
    // with no probe. The honest baseline (pre-probe) must be 'checking…', never a
    // fabricated health assertion.
    expect(html).not.toContain("healthy · TLS");
    expect(html).toContain("checking");
  });

  it("the smart-wallet pill shows the real derived address (or '—'), never a fake 'ADDRESS SET'", () => {
    const withAddr = renderToStaticMarkup(
      <Settings store={stubStore} s={sectionState("keys", { walletAddr: "0xABCdef0000000000000000000000000000001234" })} />,
    );
    expect(withAddr).not.toContain("ADDRESS SET");
    // A truncated form of the real address is shown.
    expect(withAddr).toContain("0xABCd");

    const noAddr = renderToStaticMarkup(
      <Settings store={stubStore} s={sectionState("keys", { walletAddr: "" })} />,
    );
    expect(noAddr).not.toContain("ADDRESS SET");
  });

  it("the receipts card shows an honest 'No receipts yet' until a real receipt read exists", () => {
    const html = renderToStaticMarkup(<Settings store={stubStore} s={sectionState("billing")} />);
    expect(html).not.toContain("2026-07-11 · Pilot membership · $48.00");
    expect(html).toContain("No receipts yet");
  });

  it("infra-gated controls are honestly disabled, not clickable excuse-buttons", () => {
    // A disabled control communicates 'not available in this build' without a
    // fake success or an apology-on-click. Assert the disabled markup exists in
    // the sections that host the gated clusters.
    const api = renderToStaticMarkup(<Settings store={stubStore} s={sectionState("api")} />);
    expect(api).toContain("disabled");
    const conn = renderToStaticMarkup(<Settings store={stubStore} s={sectionState("connections")} />);
    expect(conn).toContain("disabled");
    const app = renderToStaticMarkup(<Settings store={stubStore} s={sectionState("app")} />);
    expect(app).toContain("disabled");
  });
});
