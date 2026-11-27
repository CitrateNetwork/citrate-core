// 0.6.0 L5-WP01 characterization L5-ADAPTERS: the adapter eval gate's honest notes as they are on
// citrate-core a826e441 (lane L5 evidence E13). The gate takes a local file only, and the packaged
// app cannot run the eval tools, so the member is sent to scripts/eval-tools.mjs. Both are pinned
// ON PURPOSE: L5-WP16 (in-app eval runner) and L5-WP17 (LoRAFactory read and download) replace
// these tests with stricter ones, in separate commits that say why. The protected suite
// src/fl/adapters.test.tsx stays green beside this one.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { FlOverview, FlRoundsDomain } from "../../../../bridge/domains";
import { AdaptersView } from "../../../../fl/Adapters";

const overview = (): FlOverview => ({
  config: { url: null, source: "none", settingsUrl: null, note: null },
  starts: [],
  gates: [],
  activeAdapter: null,
  rememberedAdapter: null,
  restoreError: null,
  consentedRounds: [],
  consentFile: null,
  consentError: null,
  storeError: null,
});

function domain(): FlRoundsDomain {
  return {
    overview: vi.fn(async () => overview()),
    setCoordinator: vi.fn(),
    plan: vi.fn(),
    lookupPlan: vi.fn(),
    start: vi.fn(),
    gateAdapter: vi.fn(),
    loadAdapter: vi.fn(),
    unloadAdapter: vi.fn(),
    revokeConsent: vi.fn(),
    trajectoryStatus: vi.fn(),
    setTrajectoryConsent: vi.fn(),
    buildTrainingSet: vi.fn(),
  } as unknown as FlRoundsDomain;
}

async function mount(fl: FlRoundsDomain) {
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  await act(async () => root.render(<AdaptersView fl={fl} toast={() => {}} go={() => {}} pick={async () => null} />));
  const done = () => {
    act(() => root.unmount());
    el.remove();
  };
  return { el, done };
}

const EVAL_SCRIPTS =
  "The packaged app cannot run the eval tools yet. Run scripts/eval-tools.mjs (and optionally scripts/eval-qa.mjs) for the base model and for the base model with this adapter (--adapter-sha256), then add both scorecards.";

describe("L5-WP01 characterization: adapter gate notes", () => {
  it("notes_lorafactory_download_is_not_built", async () => {
    const fl = domain();
    const m = await mount(fl);
    expect(m.el.textContent).toContain("Adapter download from LoRAFactory is not built: the gate takes a local file.");
    // Nothing is fetched from LoRAFactory or anywhere else: the view only reads the overview.
    expect(fl.overview).toHaveBeenCalled();
    expect(fl.gateAdapter).not.toHaveBeenCalled();
    expect(fl.loadAdapter).not.toHaveBeenCalled();
    m.done();
  });

  it("scorecards_needed_card_names_the_eval_scripts", async () => {
    const m = await mount(domain());
    const cardEl = m.el.querySelector('[data-testid="scorecards-needed"]');
    expect(cardEl).not.toBeNull();
    expect(cardEl?.textContent).toContain("Scorecards needed");
    expect(cardEl?.textContent).toContain(EVAL_SCRIPTS);
    // The gate cannot run until the member adds an adapter file and both tool-call scorecards.
    expect(m.el.querySelector<HTMLButtonElement>('[data-testid="gate-run"]')?.disabled).toBe(true);
    expect(m.el.textContent).toContain("thresholds pending owner sign-off");
    m.done();
  });
});
