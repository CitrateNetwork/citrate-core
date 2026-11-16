// 0.5.4 Hermes × FL, WP-H4: the HIC-2 envelope tools. The caps themselves are enforced and
// tested in core (src-tauri/src/fl_envelope_tests.rs); these tests check that Hermes only reads
// core's status, proposes on a card, and turns the envelope off without one.
import { describe, it, expect, vi, afterEach } from "vitest";
import { FL_ENVELOPE_TOOL_NAMES, runFlEnvelopeTool, statusWords, type FlEnvelopeDeps, type FlEnvelopeStatus } from "./flEnvelopeTools";
import { AGENT_TOOLS, AUTHORITY_LOWERING_AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { store } from "../shell/store";
import type { EnvelopeRecord } from "../fl/authority";

const NOT_DEPLOYED = "WorkMarket is not deployed on this network yet, so the envelope authorizes nothing.";
const RULES = ["The first join with a new group or poster always asks you.", "An offer's text can never change the envelope."];

const record = (over: Partial<EnvelopeRecord> = {}): EnvelopeRecord => ({
  on: true,
  name: "Nights",
  joinsPerWeek: 3,
  maxHoursPerRound: 8,
  approvedAt: 1,
  offAt: null,
  weekStart: 1,
  joinsThisWeek: 1,
  ...over,
});

const status = (over: Partial<FlEnvelopeStatus> = {}): FlEnvelopeStatus => ({
  scope: "fl_envelope",
  envelope: record(),
  joinsLeft: 2,
  workMarketPinned: false,
  authorizes: false,
  note: NOT_DEPLOYED,
  rules: RULES,
  ...over,
});

function deps(s: FlEnvelopeStatus = status(), decision = "approved") {
  const d = {
    api: {
      status: vi.fn(async () => s),
      approve: vi.fn(async (i: { name: string; joinsPerWeek: number; maxHoursPerRound: number }) => record({ ...i, joinsThisWeek: 0 })),
      off: vi.fn(async (): Promise<EnvelopeRecord | null> => record({ on: false, offAt: 2 })),
    },
    ask: vi.fn(async () => decision),
    toast: vi.fn(),
  };
  return d satisfies FlEnvelopeDeps;
}

const run = (name: (typeof FL_ENVELOPE_TOOL_NAMES)[number], args: Record<string, unknown>, d: FlEnvelopeDeps) => runFlEnvelopeTool(name, args, annotationFor(name), d);

afterEach(() => vi.restoreAllMocks());

describe("WP-H4: the envelope tools are offered and classified", () => {
  it("read is read-only; propose asks (it is neither read-only nor authority-lowering)", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name as string);
    for (const n of FL_ENVELOPE_TOOL_NAMES) expect(names).toContain(n);
    expect(READ_ONLY_AGENT_TOOLS.has("fl_envelope_read")).toBe(true);
    expect(READ_ONLY_AGENT_TOOLS.has("fl_envelope_propose")).toBe(false);
    expect(AUTHORITY_LOWERING_AGENT_TOOLS.has("fl_envelope_propose")).toBe(false);
  });
});

describe("fl_envelope_read", () => {
  it("quotes core's caps, that it authorizes nothing without WorkMarket, and the fixed rules", async () => {
    const out = await run("fl_envelope_read", {}, deps());
    expect(out.result).toContain('The envelope "Nights" is on: 2 of 3 joins left this week, up to 8 machine hours per round.');
    expect(out.result).toContain("It authorizes nothing yet: " + NOT_DEPLOYED);
    for (const r of RULES) expect(out.result).toContain(r);
  });
  it("no envelope or off is HIC-1; a core error is passed on", async () => {
    expect(statusWords(status({ envelope: null, joinsLeft: null }))).toMatch(/^No envelope is approved: every action asks the member \(HIC-1\)/);
    expect(statusWords(status({ envelope: record({ on: false }), joinsLeft: null }))).toMatch(/is off: every action asks/);
    const d = deps();
    d.api.status.mockRejectedValueOnce(new Error("The envelope is kept by the desktop app"));
    expect((await run("fl_envelope_read", {}, d)).result).toBe("The envelope could not be read: The envelope is kept by the desktop app");
  });
});

describe("fl_envelope_propose", () => {
  it("off applies at once with no card", async () => {
    const d = deps();
    const out = await run("fl_envelope_propose", { enabled: false }, d);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.api.off).toHaveBeenCalledTimes(1);
    expect(d.api.approve).not.toHaveBeenCalled();
    expect(out.result).toMatch(/Turned the envelope "Nights" off/);
    expect(d.toast).toHaveBeenCalledWith(expect.not.stringMatching(/approve/i));
  });

  it("off with no envelope says there is nothing to turn off", async () => {
    const d = deps();
    d.api.off.mockResolvedValueOnce(null);
    expect((await run("fl_envelope_propose", { enabled: "false" }, d)).result).toMatch(/no envelope to turn off/);
  });

  it("refuses caps outside core's limits without a card", async () => {
    const d = deps();
    expect((await run("fl_envelope_propose", { enabled: true, joinsPerWeek: 0, maxHoursPerRound: 8 }, d)).result).toMatch(/1 to 50/);
    expect((await run("fl_envelope_propose", { enabled: true, joinsPerWeek: 51, maxHoursPerRound: 8 }, d)).result).toMatch(/1 to 50/);
    expect((await run("fl_envelope_propose", { enabled: true, joinsPerWeek: 3, maxHoursPerRound: 73 }, d)).result).toMatch(/1 to 72/);
    expect((await run("fl_envelope_propose", { enabled: true, joinsPerWeek: 2.5, maxHoursPerRound: 8 }, d)).result).toMatch(/whole number/);
    expect((await run("fl_envelope_propose", {}, d)).result).toMatch(/nothing changed/);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.api.approve).not.toHaveBeenCalled();
  });

  it("the card names the caps, what always asks, and that this network authorizes nothing", async () => {
    const d = deps(status(), "declined");
    const out = await run("fl_envelope_propose", { enabled: true, joinsPerWeek: 3, maxHoursPerRound: 8, name: "Nights" }, d);
    const spec = d.ask.mock.calls[0][0] as unknown as { title: string; rows: { k: string; v: string }[] };
    const row = (k: string) => spec.rows.find((r) => r.k === k)?.v;
    expect(spec.title).toBe("Approve an HIC-2 envelope");
    expect(row("Joins per week")).toBe("3");
    expect(row("Machine hours per round")).toBe("up to 8");
    expect(row("Always asks you")).toMatch(/first join with a new group or poster/);
    expect(row("Never in the envelope")).toMatch(/opening a round, publishing a dataset, offboarding, disputes and claims/);
    expect(row("On this network")).toBe(NOT_DEPLOYED);
    expect(d.api.approve).not.toHaveBeenCalled();
    expect(out.result).toBe("The member declined; no envelope changed.");
  });

  it("saves exactly the carded caps on Approve, and says it authorizes nothing here", async () => {
    const d = deps();
    const out = await run("fl_envelope_propose", { enabled: true, joinsPerWeek: "4", maxHoursPerRound: 6 }, d);
    expect(d.api.approve).toHaveBeenCalledWith({ name: "Rounds within my terms", joinsPerWeek: 4, maxHoursPerRound: 6 });
    expect(out.result).toContain("4 joins per week, up to 6 machine hours per round.");
    expect(out.result).toContain(NOT_DEPLOYED);
  });

  it("core's refusal on save is passed on", async () => {
    const d = deps();
    d.api.approve.mockRejectedValueOnce(new Error("the keychain is not reachable; your work settings stay closed"));
    expect((await run("fl_envelope_propose", { enabled: true, joinsPerWeek: 3, maxHoursPerRound: 8 }, d)).result).toBe(
      "The envelope was not saved: the keychain is not reachable; your work settings stay closed",
    );
  });
});

describe("store.handleTool routes the WP-H4 tools", () => {
  const call = (name: string, args: Record<string, unknown>): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });

  it("fl_envelope_propose on asks once, even HIC-required; a decline saves nothing", async () => {
    const d = deps();
    vi.spyOn(store, "flEnvelopeApi").mockReturnValue(d.api);
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("fl_envelope_propose", { enabled: true, joinsPerWeek: 3, maxHoursPerRound: 8 }), "m1", () => {}, { hic: "required", hicReason: "read untrusted" });
    expect(sig).toHaveBeenCalledTimes(1);
    expect(d.api.approve).not.toHaveBeenCalled();
  });

  it("in the web preview the status read says the desktop app keeps the envelope", async () => {
    const out = await store.handleTool(call("fl_envelope_read", {}), "m1", () => {});
    expect(out).toBe("The envelope could not be read: The envelope is kept by the desktop app");
  });
});
