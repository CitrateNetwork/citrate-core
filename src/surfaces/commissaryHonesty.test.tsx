// QA 2026-08-01 — the Commissary honesty pass (Rule 1 / display honesty + fail-closed gating).
//
// This surface carries an UNFIXED VARIANT of the bug class already remediated on
// Storage (see storageHonesty.test.tsx): a `setTimeout` progress bar that ends by
// claiming cryptographic verification which never happened.
//
// Four defects these tripwires pin:
//
//   1. RULE 1 — `startDownload` is a pure setTimeout chain. It streams no bytes,
//      computes no digest, and writes no audit row, then renders
//      "✓ verified · sha256 match · audit-logged". Three false claims in one line.
//
//   2. RULE 1 — the seeded checksums are ELIDED PLACEHOLDERS ("sha256:2f8e17aa…c9c41").
//      A hash with a "…" in it cannot verify anything; rendering it in a mono font
//      next to the word "verified" dresses a placeholder as provenance.
//
//   3. FAIL-OPEN GATING — `locked = rank[minTier] > rank[effTier]`. An unknown tier
//      yields `undefined`, and `2 > undefined` is false, so an UNRECOGNISED tier
//      renders every gated card UNLOCKED. The identity authority mints tiers this
//      map does not list, so this is reachable, not theoretical.
//
//   4. UNDER-GATING — service cards are built as `CATALOG.services` with no `locked`
//      computation at all. Per the owner's 2026-07-30 decision, apps AND services
//      require a verified member; only SDKs and docs are exempt.
//
// The client is not the authority — core-membership re-checks entitlement and KYC at
// download time, so none of these is an auth bypass. They are honesty and UX defects:
// the surface tells a member they hold access they do not hold, then the server
// refuses them.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Commissary } from "./Commissary";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

const stubStore = {
  setState: () => {},
  save: () => {},
  copy: () => {},
} as unknown as Store;

function commissaryState(patch: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  return { ...s, cTab: "apps", ...patch };
}

function html(patch: Partial<AppState> = {}): string {
  return renderToStaticMarkup(<Commissary store={stubStore} s={commissaryState(patch)} />);
}

describe("Commissary honesty — no fabricated verification claim (Rule 1)", () => {
  // Swept across EVERY download state, not just the one the current implementation
  // happens to use. An earlier version of this test pinned only `done`; when the
  // render moved to an `unavailable` branch the assertion stopped reaching the
  // markup and passed vacuously — it survived a mutant that restored the false
  // claim. Sweeping the state space is what makes this bite regardless of which
  // branch a future client wires up.
  const DL_STATES = ["idle", "mint", "dl", "verify", "done", "unavailable"] as const;

  it.each(DL_STATES)("NEVER claims sha256 verification in dl state '%s'", (st) => {
    const dl: AppState["dl"] = {};
    for (const id of ["citrate-native", "citrate-core", "citrate-studio", "citrate-comms"]) {
      dl[id] = { st, pct: 100 };
    }
    const out = html({ tier: "enterprise", dl });
    expect(out).not.toContain("sha256 match");
    expect(out).not.toContain("audit-logged");
    expect(out).not.toContain("verifying sha256");
  });

  it("NEVER renders an ELIDED placeholder checksum as if it were a real digest", () => {
    const out = html({ tier: "enterprise" });
    // A digest containing an ellipsis is a placeholder, not a hash.
    expect(out).not.toMatch(/sha256:[0-9a-f]*…/);
  });
});

describe("Commissary gating — fail CLOSED on a tier it does not recognise", () => {
  it("does NOT unlock gated cards for a tier absent from the rank map", () => {
    // The authority can mint tiers this client has never heard of. An unrecognised
    // tier must collapse to the LEAST access, never the most.
    const unknown = html({ tier: "some-tier-minted-later" as AppState["tier"] });
    const free = html({ tier: "free" });
    // Whatever the free tier is refused, an unknown tier must also be refused.
    const lockedMarkers = (s: string) => (s.match(/Requires /g) ?? []).length;
    expect(lockedMarkers(unknown)).toBeGreaterThanOrEqual(lockedMarkers(free));
  });

  it("does NOT unlock gated cards when the tier is missing entirely", () => {
    const missing = html({ tier: undefined as unknown as AppState["tier"] });
    expect((missing.match(/Requires /g) ?? []).length).toBeGreaterThan(0);
  });
});

// NOTE — deliberately NOT asserted here: service-card gating.
//
// `CATALOG.services` entries carry only { id, name, desc, url } — no tier field —
// so the surface cannot distinguish a genuinely public link (explorer.citrate.ai,
// dashboard.citrate.ai) from a gated one (dataroom, which is 506(b)-restricted).
// Blanket-gating them would assert something false; leaving them open under-states
// the dataroom. That is a SEED MODELLING gap, not a render bug, and asserting it
// here would encode a decision nobody has made. Recorded in the QA report instead.
