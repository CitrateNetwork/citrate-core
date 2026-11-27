// 0.6.0 L3-WP00 characterization: the onboarding stages as they are on citrate-core a826e441,
// pinned before L3-WP01 (sign in, then the HIC choice, then the app) replaces the wizard. A failure
// here means onboarding changed: update the expectation only together with that reviewed change.
//
//   1. The progress spine lists seven stages, s1 to s7, in this order and with these labels.
//   2. S6 shows the local model step only once the node is genuinely pulling the chain (syncing with
//      at least one peer) or is validating or synced.
//   3. S6 Continue goes to S7 for a member with a grant, and into the app for anyone else.
//   4. "Explore free" on S0 goes straight into the app with the coach marked done.
//   5. Entering the app starts the coach unless it was already done.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { Onboarding } from "../../../../onboarding/Onboarding";
import { freshState, type AppState } from "../../../state";
import { store as realStore, type Store } from "../../../store";

// Onboarding renders from `s`; `store` is read during render only by the calls below.
const noopStore = {
  startNode: () => {},
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

function at(over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  return { ...s, signedIn: true, stage: "s6", node: "syncing", height: 100_000, peers: 3, ...over };
}

const textOf = (html: string) => html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ");
const SPINE = ["Sign in", "Verify identity", "Free node or membership", "Wallet ready", "Membership grant", "Node ignition", "Activate validator"];

describe("L3-WP00 characterization: onboarding stages", () => {
  it("seven_stage_spine_labels", () => {
    const src = readFileSync("src/onboarding/Onboarding.tsx", "utf8").replace(/\s+/g, " ");
    expect(src).toContain('const stageOrder = ["s1", "s2", "s3", "s4", "s5", "s6", "s7"];');
    expect(src).toContain(
      'const stages: [string, string][] = [ ["s1", "Sign in"], ["s2", "Verify identity"], ["s3", "Free node or membership"], ["s4", "Wallet ready"], ["s5", "Membership grant"], ["s6", "Node ignition"], ["s7", "Activate validator"], ];',
    );
    // The rendered spine shows the seven labels in order.
    const text = textOf(renderToStaticMarkup(<Onboarding store={noopStore} s={at()} />));
    let from = 0;
    for (const label of SPINE) {
      const i = text.indexOf(label, from);
      expect(i, label).toBeGreaterThanOrEqual(from);
      from = i + label.length;
    }
  });

  it("s6_model_step_hidden_until_peers_gt_0", () => {
    const html = (over: Partial<AppState>) => renderToStaticMarkup(<Onboarding store={noopStore} s={at(over)} />);
    // Syncing with no peer: no model step, no persona step, nothing to continue with yet.
    const noPeer = html({ node: "syncing", peers: 0 });
    expect(noPeer).toContain("S6 · Node ignition");
    expect(noPeer).not.toContain('data-testid="model-step"');
    expect(noPeer).not.toContain('data-testid="onboarding-persona-step"');
    expect(noPeer).not.toContain('data-testid="model-enter"');
    // Syncing with a peer, validating or synced: the model step (and its Enter) shows.
    for (const over of [{ node: "syncing", peers: 1 }, { node: "validating", peers: 0 }, { node: "synced", peers: 0 }] as Partial<AppState>[]) {
      const h = html(over);
      expect(h, JSON.stringify(over)).toContain('data-testid="model-step"');
      expect(h, JSON.stringify(over)).toContain('data-testid="onboarding-persona-step"');
      expect(h, JSON.stringify(over)).toContain('data-testid="model-enter"');
    }
    // Node off or provisioning: no model step either.
    for (const node of ["off", "prov"] as AppState["node"][]) expect(html({ node, peers: 5 }), node).not.toContain('data-testid="model-step"');
  });
});

describe("L3-WP00 characterization: onboarding transitions (store)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(realStore, "refreshActivation").mockResolvedValue(false);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("s6_continue_goes_s7_with_grant_else_done", () => {
    realStore.setState({ stage: "s6", hasGrant: true });
    realStore.onS6Continue();
    expect(realStore.state.stage).toBe("s7");
    expect(realStore.refreshActivation).toHaveBeenCalledTimes(1);

    realStore.setState({ stage: "s6", hasGrant: false });
    realStore.onS6Continue();
    expect(realStore.state.stage).toBe("done");
    expect(realStore.refreshActivation).toHaveBeenCalledTimes(1);
  });

  it("on_explore_sets_done_and_coachDone", () => {
    realStore.setState({ stage: "s0", tier: "pilot", coachDone: false, coach: -1, chatMsgs: [] });
    realStore.onExplore();
    expect(realStore.state.stage).toBe("done");
    expect(realStore.state.tier).toBe("free");
    expect(realStore.state.coachDone).toBe(true);
    // Explore does not start the coach, and it opens chat on one greeting.
    expect(realStore.state.coach).toBe(-1);
    expect(realStore.state.chatMsgs.length).toBe(1);
  });

  it("on_enter_starts_coach_when_not_done", () => {
    realStore.setState({ stage: "s6", hasGrant: false, coachDone: false, coach: -1, chatMsgs: [] });
    realStore.onEnter();
    expect(realStore.state.stage).toBe("done");
    expect(realStore.state.coach).toBe(0);
    expect(realStore.state.chatMsgs.length).toBe(1);

    // Coach already done: entering does not show it again, and an open chat is kept as it is.
    const kept = realStore.state.chatMsgs;
    realStore.setState({ stage: "s6", coachDone: true, coach: 2 });
    realStore.onEnter();
    expect(realStore.state.stage).toBe("done");
    expect(realStore.state.coach).toBe(-1);
    expect(realStore.state.chatMsgs).toBe(kept);
  });
});
