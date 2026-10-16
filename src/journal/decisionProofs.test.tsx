// HUP-S7.3 + S7.5 (US-7.2 AC3, US-7.3 AC3): in the Journal: the member lists past decisions and
// asks core to prove one (core checks the proof and reads AnchorRegistry itself), and shares a
// closed day's aggregates (one wallet card per metric, nothing signed here).
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DecisionProofs } from "./DecisionProofs";
import { BenchmarkShare } from "./BenchmarkShare";
import { describeRecord, type RecordsPage, type ProofVerdict } from "./proofView";
import type { ReportInvoke } from "./HermesDailyReport";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function makeInvoke(handlers: Record<string, (args: Record<string, unknown>) => unknown>) {
  return vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    const h = handlers[cmd];
    if (!h) throw new Error(`unexpected command ${cmd}`);
    return h(args);
  }) as unknown as ReportInvoke & ReturnType<typeof vi.fn>;
}

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {});
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}

const decision = (seq: number, day: number, anchored: boolean, subject: string) => ({
  seq,
  tsMs: day * 86_400_000 + 1000,
  day,
  date: day === 20727 ? "2026-10-01" : "2026-10-02",
  hash: "ab".repeat(32),
  batched: anchored,
  anchored,
  anchorTx: anchored ? "0x" + "cd".repeat(32) : null,
  anchorBlock: anchored ? 4 : null,
  record: {
    v: 1,
    seq,
    ts_ms: day * 86_400_000 + 1000,
    prev: "00".repeat(32),
    actor: { kind: "member", id: "member" },
    entry: { decision: { tier: "hic-1", kind: "deploy", subject, decision: "denied", reason: "rejected on the card", evidence: [] } },
  },
});

const outcome = (seq: number) => ({
  ...decision(seq, 20727, true, ""),
  record: {
    v: 1,
    seq,
    ts_ms: 1,
    prev: "00".repeat(32),
    actor: { kind: "agent", id: "hermes" },
    entry: { outcome: { decision_seq: 0, outcome: "completed", detail: "mined" } },
  },
});

function page(over: Partial<RecordsPage> = {}): RecordsPage {
  return {
    configured: true,
    recordsPresent: true,
    limit: 10,
    records: [decision(5, 20728, false, "today"), decision(4, 20727, true, "deploy HelloMint"), outcome(1)],
    nextBefore: null,
    ...over,
  };
}

const proven: ProofVerdict = {
  seq: 4,
  day: 20727,
  date: "2026-10-01",
  commitment: "0x" + "e2".repeat(32),
  inclusionOk: true,
  inclusionError: null,
  recordBound: true,
  chain: { state: "anchored", registry: "0xa1", anchor: { kind: 2, root: "0x" + "e2".repeat(32), committer: "0x31", blockNumber: 4, timestamp: 1 }, by_you: true },
  proven: true,
  line: "Proven. Record #4 is in the decisions of 2026-10-01 (UTC), anchored on 40204 in block 4 by your anchor key.",
  record: null,
};

describe("describeRecord", () => {
  it("names decisions and outcomes in plain words", () => {
    expect(describeRecord(decision(4, 20727, true, "deploy HelloMint").record)).toBe("Denied: deploy HelloMint (HIC-1, deploy)");
    expect(describeRecord(outcome(1).record)).toBe("Outcome of #0: completed");
    expect(describeRecord({})).toBe("Unrecognized record");
  });
});

describe("DecisionProofs", () => {
  it("calls nothing until the member asks for the list", async () => {
    const invoke = makeInvoke({});
    const { host, root } = await mount(<DecisionProofs invoke={invoke} />);
    expect(invoke).not.toHaveBeenCalled();
    expect(q(host, "dp-load")).toBeTruthy();
    root.unmount();
  });

  it("lists past decisions with their anchor state and proves one through core", async () => {
    const invoke = makeInvoke({
      hermes_anchor_records: () => page(),
      hermes_anchor_proof: () => proven,
    });
    const { host, root } = await mount(<DecisionProofs invoke={invoke} />);
    await click(q(host, "dp-load"));
    expect(invoke).toHaveBeenCalledWith("hermes_anchor_records", { before: null, limit: 10 });
    expect(q(host, "dp-row-4")?.textContent).toMatch(/deploy HelloMint/);
    expect(q(host, "dp-row-4")?.textContent).toMatch(/anchored/);
    expect(q(host, "dp-row-5")?.textContent).toMatch(/not anchored yet/);
    // The open day has nothing to prove yet.
    expect(q<HTMLButtonElement>(host, "dp-prove-5")?.disabled).toBe(true);
    await click(q(host, "dp-prove-4"));
    expect(invoke).toHaveBeenCalledWith("hermes_anchor_proof", { seq: 4 });
    expect(q(host, "dp-verdict-4")?.textContent).toMatch(/^Proven\./);
    expect(q(host, "dp-verdict-4")?.getAttribute("data-proven")).toBe("true");
    root.unmount();
  });

  it("shows a refusal from core as it is, never as proven", async () => {
    const invoke = makeInvoke({
      hermes_anchor_records: () => page(),
      hermes_anchor_proof: () => ({ ...proven, proven: false, chain: { state: "not_anchored", registry: "0xa1" }, line: "The proof holds, but the decisions of 2026-10-01 are not anchored on 40204 yet." }),
    });
    const { host, root } = await mount(<DecisionProofs invoke={invoke} />);
    await click(q(host, "dp-load"));
    await click(q(host, "dp-prove-4"));
    expect(q(host, "dp-verdict-4")?.textContent).toMatch(/not anchored on 40204 yet/);
    expect(q(host, "dp-verdict-4")?.getAttribute("data-proven")).toBe("false");
    root.unmount();
  });

  it("pages to older records and reports errors honestly", async () => {
    let n = 0;
    const invoke = makeInvoke({
      hermes_anchor_records: (a) => {
        n += 1;
        if (n === 1) return page({ nextBefore: 1 });
        expect(a).toEqual({ before: 1, limit: 10 });
        return page({ records: [decision(0, 20727, true, "first")], nextBefore: null });
      },
    });
    const { host, root } = await mount(<DecisionProofs invoke={invoke} />);
    await click(q(host, "dp-load"));
    await click(q(host, "dp-older"));
    expect(q(host, "dp-row-0")?.textContent).toMatch(/first/);
    expect(q(host, "dp-older")).toBeNull();
    root.unmount();

    const failing = makeInvoke({
      hermes_anchor_records: () => {
        throw new Error("Hermes is not running, so the decision records cannot be read.");
      },
    });
    const m = await mount(<DecisionProofs invoke={failing} />);
    await click(q(m.host, "dp-load"));
    expect(q(m.host, "dp-error")?.textContent).toMatch(/not running/);
    m.root.unmount();
  });

  it("an empty folder says there is nothing recorded yet", async () => {
    const invoke = makeInvoke({ hermes_anchor_records: () => page({ records: [], recordsPresent: false }) });
    const { host, root } = await mount(<DecisionProofs invoke={invoke} />);
    await click(q(host, "dp-load"));
    expect(q(host, "dp-empty")?.textContent).toMatch(/No decisions are recorded yet/);
    root.unmount();
  });
});

describe("BenchmarkShare", () => {
  it("is not offered while sharing is off", async () => {
    const invoke = makeInvoke({});
    const { host, root } = await mount(<BenchmarkShare invoke={invoke} sharing={false} day="2026-09-30" />);
    expect(q<HTMLButtonElement>(host, "bs-share")?.disabled).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
    root.unmount();
  });

  it("raises the cards through core and says each needs approval", async () => {
    const invoke = makeInvoke({
      hermes_benchmark_share: () => ({
        day: "2026-09-30",
        registry: "0xb1",
        agentId: "7",
        calls: [
          { metric: "hermes.daily.turns", value: "3" },
          { metric: "hermes.daily.answered", value: "3" },
        ],
        cards: [{ id: "11" }, { id: "12" }],
        pendingOwnerSignOff: ["Benchmark sharing sends one wallet approval card per metric. Pending owner sign-off."],
      }),
    });
    const { host, root } = await mount(<BenchmarkShare invoke={invoke} sharing={true} day="2026-09-30" />);
    await click(q(host, "bs-share"));
    expect(invoke).toHaveBeenCalledWith("hermes_benchmark_share", { day: "2026-09-30" });
    expect(q(host, "bs-result")?.textContent).toMatch(/2 approval cards/);
    expect(q(host, "bs-result")?.textContent).toMatch(/Nothing is sent until you approve each one/);
    expect(host.textContent).toMatch(/hermes\.daily\.turns: 3/);
    expect(host.textContent).not.toMatch(/—/);
    root.unmount();
  });

  it("shows core's refusal", async () => {
    const invoke = makeInvoke({
      hermes_benchmark_share: () => {
        throw new Error("Sharing needs your Hermes identity (an AgentSBT) first.");
      },
    });
    const { host, root } = await mount(<BenchmarkShare invoke={invoke} sharing={true} day="2026-09-30" />);
    await click(q(host, "bs-share"));
    expect(q(host, "bs-error")?.textContent).toMatch(/AgentSBT/);
    root.unmount();
  });
});

describe("HermesDailyReport wiring", () => {
  it("offers the proof list and, when sharing is on, sharing of yesterday's numbers", async () => {
    const { HermesDailyReport } = await import("./HermesDailyReport");
    const status = {
      anchor: {
        gate: "off",
        statusLine: "Nightly anchoring is off.",
        registry: "0xa1",
        enabled: false,
        anchorKey: null,
        anchorKeyError: null,
        sidecar: null,
        sidecarError: null,
        pending: [],
        submitted: [],
      },
      benchmark: { registry: "0xb1", deployed: true, sharing: true, statusLine: "Benchmark sharing is on." },
      pendingOwnerSignOff: [],
    };
    const invoke = makeInvoke({
      hermes_metering_daily: () => {
        throw new Error("Hermes is not running, so today's numbers are unknown.");
      },
      hermes_chain_status: () => status,
    });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => new Date("2026-10-02T01:00:00Z")} />);
    expect(q(host, "dp-load")).toBeTruthy();
    const share = q<HTMLButtonElement>(host, "bs-share");
    expect(share?.disabled).toBe(false);
    expect(share?.textContent).toMatch(/2026-10-01/);
    root.unmount();
  });
});
