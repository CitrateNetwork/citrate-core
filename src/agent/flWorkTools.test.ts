// 0.5.4 Hermes × FL, WP-H3: worker terms, consent and eligibility tools (gap analysis §5.2).
// Each tool runs through runFlWorkTool with its seams spied; the store-level cases drive
// store.handleTool, the entry point a Hermes turn uses.
import { describe, it, expect, vi, afterEach } from "vitest";
import { FL_WORK_TOOL_NAMES, checkRequirements, proposedTerms, runFlWorkTool, type FlWorkDeps } from "./flWorkTools";
import { AGENT_TOOLS, AUTHORITY_LOWERING_AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import type { FlOverview, TierReport } from "../bridge/domains";
import type { WorkerTerms } from "../fl/workerTerms";
import { NOT_AVAILABLE } from "../fl/networkState";
import { HELD_DURING_START, hermesRunningSlice } from "../shell/slices/hermesRunning";

const GB = 1024 ** 3;
const ROUND = "0x" + "ab".repeat(32);
const SAVED: WorkerTerms = { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: false, mainsOnly: true, dataClasses: ["text"] };
const NO_WORKMARKET = { addresses: { FederatedRoundLedger: "0x25fc000000000000000000000000000000053c5" } };
const WITH_WORKMARKET = { addresses: { WorkMarket: "0x6baf000000000000000000000000000000002369" } };

function overview(over: Partial<FlOverview> = {}): FlOverview {
  return {
    config: { url: null, source: "none" } as never,
    starts: [],
    gates: [],
    activeAdapter: null,
    rememberedAdapter: null,
    restoreError: null,
    consentedRounds: [],
    consentFile: null,
    consentError: null,
    storeError: null,
    ...over,
  };
}

function report(facts: Partial<TierReport["facts"]> = {}): TierReport {
  return {
    facts: { os: "macos", arch: "aarch64", totalRamBytes: 32 * GB, unifiedMemory: true, gpuVramBytes: null, diskFreeBytes: 200 * GB, ...facts },
    recommendation: {} as never,
    overrideTier: null,
    effective: "T1" as never,
    profiles: [],
  };
}

function deps(over: { saved?: WorkerTerms | null; decision?: string; book?: FlWorkDeps["book"] } = {}) {
  const d = {
    terms: {
      load: vi.fn(async () => (over.saved === undefined ? SAVED : over.saved)),
      save: vi.fn(async () => ({ savedAt: 1, termsHash: "f".repeat(64), sealed: true })),
    },
    flRounds: {
      overview: vi.fn(async () => overview({ consentedRounds: [ROUND] })),
      revokeConsent: vi.fn(async () => [] as string[]),
      trajectoryStatus: vi.fn(async () => ({ settings: { enabled: true, changedAtMs: 1 }, recordedFiles: 4, datasets: 1, appliesOnRestart: true, loadError: null })),
      setTrajectoryConsent: vi.fn(async () => ({ settings: { enabled: false, changedAtMs: 2 }, recordedFiles: 4, datasets: 0, appliesOnRestart: true, loadError: null })),
    },
    tier: { recommend: vi.fn(async (): Promise<TierReport | null> => report()) },
    book: over.book ?? NO_WORKMARKET,
    ask: vi.fn(async () => over.decision ?? "approved"),
    toast: vi.fn(),
  };
  return d satisfies FlWorkDeps;
}

const run = (name: (typeof FL_WORK_TOOL_NAMES)[number], args: Record<string, unknown>, d: FlWorkDeps) => runFlWorkTool(name, args, annotationFor(name), d);

afterEach(() => vi.restoreAllMocks());

describe("WP-H3: every terms, consent and eligibility tool is offered and classified", () => {
  it("is in AGENT_TOOLS with an annotation; reads are read-only, withdraw lowers authority", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name as string);
    for (const n of FL_WORK_TOOL_NAMES) {
      expect(names).toContain(n);
      expect(annotationFor(n), n).not.toBeNull();
    }
    for (const n of ["fl_terms_read", "fl_consent_list", "fl_eligibility"]) expect(READ_ONLY_AGENT_TOOLS.has(n), n).toBe(true);
    expect(AUTHORITY_LOWERING_AGENT_TOOLS.has("fl_consent_withdraw")).toBe(true);
    expect(READ_ONLY_AGENT_TOOLS.has("fl_terms_propose")).toBe(false);
    expect(AUTHORITY_LOWERING_AGENT_TOOLS.has("fl_terms_propose")).toBe(false);
  });
});

describe("fl_terms_read", () => {
  it("reads the sealed terms in the member's words", async () => {
    const out = await run("fl_terms_read", {}, deps());
    expect(out.result).toContain("Floor: 1.20 SALT per unit");
    expect(out.result).toContain("Hours: 23:00 to 07:00");
    expect(out.result).toContain("only the floor is enforced on chain");
  });
  it("says not set when none are saved, and passes a core refusal on", async () => {
    expect((await run("fl_terms_read", {}, deps({ saved: null }))).result).toMatch(/^No worker terms are set/);
    const d = deps();
    d.terms.load.mockRejectedValueOnce(new Error("the keychain is not reachable; your work settings stay closed"));
    expect((await run("fl_terms_read", {}, d)).result).toBe("The worker terms could not be read: the keychain is not reachable; your work settings stay closed");
  });
});

describe("fl_terms_propose", () => {
  it("merges the call over the saved terms", () => {
    const p = proposedTerms({ floorSaltPerUnit: "1.50" }, SAVED);
    expect(p).toEqual({ terms: { ...SAVED, floorSaltPerUnit: "1.50" } });
    expect(proposedTerms({ hours: "any", dataClasses: "text, code" }, SAVED)).toEqual({ terms: { ...SAVED, hours: "any", dataClasses: ["text", "code"] } });
  });

  it("with no saved terms, a floor and hours are required; nothing is asked", async () => {
    const d = deps({ saved: null });
    expect((await run("fl_terms_propose", { hours: "any" }, d)).result).toMatch(/Ask the member for a floor/);
    expect((await run("fl_terms_propose", { floorSaltPerUnit: "1.20" }, d)).result).toMatch(/Ask the member for hours/);
    expect(d.ask).not.toHaveBeenCalled();
  });

  it("refuses malformed terms before any card", async () => {
    const d = deps();
    expect((await run("fl_terms_propose", { floorSaltPerUnit: "one" }, d)).result).toMatch(/^The floor must be a SALT amount like 1.20/);
    expect((await run("fl_terms_propose", { hoursStart: "25:00", hoursEnd: "07:00" }, d)).result).toMatch(/^Hours must be times like 23:00/);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.terms.save).not.toHaveBeenCalled();
  });

  it("shows every field and what changes; with no WorkMarket there are no comparables", async () => {
    const d = deps({ decision: "declined" });
    const out = await run("fl_terms_propose", { floorSaltPerUnit: "1.50", mainsOnly: false }, d);
    const spec = d.ask.mock.calls[0][0] as unknown as { title: string; rows: { k: string; v: string }[] };
    expect(spec.title).toBe("Change your worker terms");
    const row = (k: string) => spec.rows.find((r) => r.k === k)?.v;
    expect(row("Floor")).toBe("1.20 SALT per unit → 1.50 SALT per unit");
    expect(row("Only on mains power")).toBe("yes → no");
    expect(row("Hours")).toBe("23:00 to 07:00");
    expect(row("Comparable offers")).toMatch(/WorkMarket is not deployed on this network/);
    expect(d.terms.save).not.toHaveBeenCalled();
    expect(out.result).toBe("The member declined; the terms did not change.");
  });

  it("saves exactly the terms on the card, only after Approve", async () => {
    const d = deps({ book: WITH_WORKMARKET });
    const out = await run("fl_terms_propose", { floorSaltPerUnit: "2", weekdaysOnly: "true" }, d);
    expect(d.terms.save).toHaveBeenCalledWith({ ...SAVED, floorSaltPerUnit: "2", weekdaysOnly: true });
    expect(out.result).toMatch(/^Saved the worker terms, sealed on this device \(terms hash ffffffffffff\)/);
    const spec = d.ask.mock.calls[0][0] as unknown as { rows: { k: string; v: string }[] };
    expect(spec.rows.find((r) => r.k === "Comparable offers")?.v).toMatch(/fl_offers_list/);
  });

  it("core's refusal on save is passed on", async () => {
    const d = deps();
    d.terms.save.mockRejectedValueOnce(new Error("A data class must be letters, digits, dots, dashes or underscores"));
    const out = await run("fl_terms_propose", { dataClasses: ["bad class!"] }, d);
    expect(out.result).toBe("The terms were not saved: A data class must be letters, digits, dots, dashes or underscores");
  });
});

describe("fl_consent_list", () => {
  it("lists round consents, withdrawn ones and the conversation switch", async () => {
    const d = deps();
    d.flRounds.overview.mockResolvedValueOnce(
      overview({ consentedRounds: [ROUND], starts: [{ planHash: "p", coordinatorUrl: "u", requires: "federated" as never, authorizedAtMs: 1, roundId: "0x" + "cd".repeat(32), revokedAtMs: 1_800_000_000_000 }] }),
    );
    const out = await run("fl_consent_list", {}, d);
    expect(out.result).toContain(ROUND);
    expect(out.result).toContain("Withdrawn: 0x" + "cd".repeat(32));
    expect(out.result).toContain("Training on verified conversations: on (4 recorded, 1 assembled training sets)");
    expect(out.result).toContain("Dataset consents: this build has no dataset store");
  });
  it("an unreadable part is not known with its reason, never none", async () => {
    const d = deps();
    d.flRounds.overview.mockResolvedValueOnce(overview({ consentError: "fl_consent.json is not valid JSON" }));
    d.flRounds.trajectoryStatus.mockRejectedValueOnce(new Error("no app data folder"));
    const out = await run("fl_consent_list", {}, d);
    expect(out.result).toContain("Round consents: not known (fl_consent.json is not valid JSON)");
    expect(out.result).toContain("Training on verified conversations: not known (no app data folder)");
    expect(out.result).not.toContain("Round consents: none");
  });
});

describe("fl_consent_withdraw", () => {
  it("withdraws one round's consent at once, with no card", async () => {
    const d = deps();
    const out = await run("fl_consent_withdraw", { scope: "round", roundId: ROUND }, d);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.flRounds.revokeConsent).toHaveBeenCalledWith(ROUND);
    expect(out.result).toMatch(/Rounds still consented: none/);
    expect(d.toast).toHaveBeenCalledWith(expect.not.stringMatching(/approve/i));
  });
  it("turns conversation training off and never on", async () => {
    const d = deps();
    await run("fl_consent_withdraw", { scope: "conversations" }, d);
    expect(d.flRounds.setTrajectoryConsent).toHaveBeenCalledWith(false, false);
    expect(d.flRounds.setTrajectoryConsent).not.toHaveBeenCalledWith(true, expect.anything());
    expect(d.ask).not.toHaveBeenCalled();
  });
  it("while Hermes is starting, the conversation switch write is held and says so", async () => {
    const d = deps();
    const before = hermesRunningSlice.get();
    hermesRunningSlice.set({ ...before, starting: 1 });
    try {
      const out = await run("fl_consent_withdraw", { scope: "conversations" }, d);
      expect(out.result.startsWith(HELD_DURING_START)).toBe(true);
      expect(d.flRounds.setTrajectoryConsent).not.toHaveBeenCalled();
    } finally {
      hermesRunningSlice.set(before);
    }
  });
  it("a bad round id or scope withdraws nothing; core's refusal is passed on", async () => {
    const d = deps();
    expect((await run("fl_consent_withdraw", { scope: "round", roundId: "7" }, d)).result).toMatch(/nothing was withdrawn/);
    expect((await run("fl_consent_withdraw", { scope: "everything" }, d)).result).toMatch(/nothing was withdrawn/);
    expect(d.flRounds.revokeConsent).not.toHaveBeenCalled();
    d.flRounds.revokeConsent.mockRejectedValueOnce(new Error(`there is no consent for round ${ROUND} to withdraw`));
    expect((await run("fl_consent_withdraw", { scope: "round", roundId: ROUND }, d)).result).toBe(`The consent was not withdrawn: there is no consent for round ${ROUND} to withdraw`);
  });
});

describe("fl_eligibility", () => {
  it("an offer id on a network without WorkMarket says Not available yet, from the book", async () => {
    const d = deps();
    const out = await run("fl_eligibility", { offerId: "7" }, d);
    expect(out.result.startsWith(NOT_AVAILABLE)).toBe(true);
    expect(d.tier.recommend).not.toHaveBeenCalled();
  });

  it("checks this machine's probe against requirements, with every why-not", async () => {
    const d = deps();
    d.tier.recommend.mockResolvedValueOnce(report({ totalRamBytes: 16 * GB, unifiedMemory: false, gpuVramBytes: null, diskFreeBytes: 10 * GB }));
    const out = await run("fl_eligibility", { requirements: { minRamGb: 32, minDiskGb: 20 } }, d);
    expect(out.result).toMatch(/^Not eligible\./);
    expect(out.result).toContain("why not: needs memory at least 32 GB; this machine has memory 16 GB");
    expect(out.result).toContain("why not: needs free disk at least 20 GB; this machine has free disk 10 GB");
  });

  it("eligible when every checked fact is met", async () => {
    const out = await run("fl_eligibility", { requirements: { minRamGb: 16, minVramGb: 16, accelerator: true } }, deps());
    expect(out.result).toMatch(/^Eligible\./);
    expect(out.result).toContain("GPU memory (shared)");
  });

  it("a fact the probe could not read is not known, never a pass or a fail", () => {
    const v = checkRequirements({ minVramGb: 8, accelerator: true, mainsOnly: true }, report({ unifiedMemory: null, gpuVramBytes: null }));
    expect(v.map((x) => x.ok)).toEqual([null, null, null]);
    expect(v[2].have).toMatch(/does not read the battery/);
  });

  it("in the web preview nothing is probed or invented", async () => {
    const d = deps();
    d.tier.recommend.mockResolvedValueOnce(null);
    expect((await run("fl_eligibility", { requirements: { minRamGb: 1 } }, d)).result).toMatch(/only in the desktop app/);
  });
});

describe("store.handleTool routes the WP-H3 tools", () => {
  const call = (name: string, args: Record<string, unknown>): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });

  it("fl_terms_propose asks once and saves only on Approve", async () => {
    const save = vi.fn(async () => ({ savedAt: 1, termsHash: "a".repeat(64), sealed: true }));
    vi.spyOn(store, "flWorkTerms").mockReturnValue({ load: async () => SAVED, save } as never);
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("fl_terms_propose", { floorSaltPerUnit: "3" }), "m1", () => {}, { hic: "required", hicReason: "read untrusted" });
    expect(sig).toHaveBeenCalledTimes(1);
    expect(save).not.toHaveBeenCalled();
  });

  it("fl_consent_withdraw opens no approval and calls only the revoke", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    const revoke = vi.spyOn(bridge.flRounds, "revokeConsent").mockResolvedValue([]);
    const set = vi.spyOn(bridge.flRounds, "setTrajectoryConsent");
    const start = vi.spyOn(bridge.flRounds, "start");
    await store.handleTool(call("fl_consent_withdraw", { scope: "round", roundId: ROUND }), "m1", () => {});
    expect(sig).not.toHaveBeenCalled();
    expect(revoke).toHaveBeenCalledWith(ROUND);
    expect(set).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
  });
});
