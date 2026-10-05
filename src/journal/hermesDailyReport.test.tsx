// HUP-S7.3 + S7.5 — the Hermes daily report in the Journal: real numbers from the sidecar's
// metering, unknowns shown as unknown, and the anchor / benchmark state with toggles that stay
// off until the registries are deployed.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { HermesDailyReport, type ReportInvoke } from "./HermesDailyReport";
import { d27Rows, formatSalt, meteringRows, utcDay, type DailyResponse, type ChainStatus } from "./meteringView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = new Date("2026-10-01T04:05:06Z");

function daily(over: Partial<DailyResponse["report"]> = {}, top: Partial<DailyResponse> = {}): DailyResponse {
  return {
    day: "2026-10-01",
    source: "log",
    persisted: true,
    notMeasured: ["time to first token", "SALT spent"],
    markdown: "",
    report: {
      schema: 1,
      day: "2026-10-01",
      turns: 6,
      sessions: 2,
      outcomes: { answered: 5, stopped: 1, step_limit: 0, failed: 0, unknown: 0 },
      verification: { passed: 5, failed: 1, unverified: 0 },
      verified_success_bps: 8333,
      latency_ms: { p50: 1200, p95: 4100, max: 5000 },
      tokens: { tokens_in: 900, tokens_out: 300, turns_reporting: 4 },
      steps_total: 11,
      tool_calls: {},
      verifiers: {},
      models: {},
      tainted_turns: 1,
      ...over,
    },
    ...top,
  };
}

function chain(over: { deployed?: boolean; pending?: ChainStatus["anchor"]["pending"] } = {}): ChainStatus {
  const deployed = over.deployed ?? false;
  return {
    anchor: {
      gate: deployed ? "off" : "not_deployed",
      statusLine: deployed ? "Nightly anchoring is off." : "Nightly anchoring is off: AnchorRegistry is not in this app's 40204 address book. Decision records stay on this device.",
      registry: deployed ? "0x00000000000000000000000000000000000000a1" : null,
      enabled: false,
      anchorKey: null,
      anchorKeyError: null,
      sidecar: { configured: true, recordsPresent: false, pendingDays: [], awaitingConfirmation: [], anchored: [], incomplete: [] },
      sidecarError: null,
      pending: over.pending ?? [],
      submitted: [],
    },
    benchmark: {
      registry: deployed ? "0x00000000000000000000000000000000000000b1" : null,
      deployed,
      sharing: false,
      statusLine: deployed ? "Benchmark sharing is off. Nothing leaves this device." : "Benchmark sharing is off: BenchmarkRegistry is not in this app's 40204 address book. Nothing leaves this device.",
    },
    pendingOwnerSignOff: ["How the anchor key pays gas. Pending owner sign-off."],
  };
}

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

describe("meteringRows", () => {
  it("formats real numbers and the verified rate", () => {
    const rows = meteringRows(daily());
    const byLabel = Object.fromEntries(rows.map((r) => [r.label, r]));
    expect(byLabel["Turns"].value).toBe("6");
    expect(byLabel["Verified success rate"].value).toBe("83.33%");
    expect(byLabel["Verified passed / failed / unverified"].value).toBe("5 / 1 / 0");
    expect(byLabel["Latency p50 / p95"].value).toBe("1200 ms / 4100 ms");
    expect(byLabel["Tokens in / out"].value).toBe("900 / 300 (reported for 4 of 6 turns)");
    expect(byLabel["Turns that read untrusted content"].value).toBe("1");
  });

  it("shows unknowns as unknown, never as zero", () => {
    const rows = meteringRows(daily({ verified_success_bps: null, latency_ms: null, tokens: { tokens_in: 0, tokens_out: 0, turns_reporting: 0 } }));
    const byLabel = Object.fromEntries(rows.map((r) => [r.label, r]));
    expect(byLabel["Verified success rate"].unknown).toBe(true);
    expect(byLabel["Verified success rate"].value).toMatch(/^unknown/);
    expect(byLabel["Latency p50 / p95"].value).toMatch(/^unknown/);
    expect(byLabel["Tokens in / out"].value).toMatch(/^unknown/);
    expect(byLabel["Tokens in / out"].value).not.toMatch(/\b0 \/ 0\b/);
    // every measure the build does not collect is listed as unknown
    expect(byLabel["time to first token"].unknown).toBe(true);
    expect(byLabel["SALT spent"].value).toMatch(/^unknown/);
  });

  it("names the UTC day", () => {
    expect(utcDay(NOW, 0)).toBe("2026-10-01");
    expect(utcDay(NOW, 1)).toBe("2026-09-30");
    expect(utcDay(new Date("2026-03-01T00:30:00Z"), 1)).toBe("2026-02-28");
  });
});

// HUP-S7.5 (D-27, US-7.3 AC1): the measures beyond counts, from a real sidecar report.
function d27(): DailyResponse {
  return daily(
    {
      schema: 2,
      ttft_ms: { p50: 180, p95: 420, max: 500 },
      speed: { tokens: 900, generation_ms: 30_000, tokens_per_s_milli: 30_000, turns_reporting: 5 },
      resources: { turns_sampled: 5, cpu_peak_bps: 8_750, ram_used_peak_bytes: 12_884_901_888, ram_total_bytes: 17_179_869_184, gpu_peak_bps: 9_100 },
      energy_estimate: { label: "estimate", microwatt_hours: 41_667, turns_estimated: 5, turns_without_gpu: 0 },
      self_review: { label: "opinion", pass: 3, fail: 1, unclear: 0, agreed_with_verifiers: 3, disagreed_with_verifiers: 1 },
    },
    {
      notMeasured: [],
      chain: { day: "2026-10-01", transactions: 2, reverted: 1, gasUsed: 150_000, feeWei: "150000000000000", valueWei: "500000000000000000", saltSpentWei: "500150000000000000", byPurpose: { registry_escalation: 1, benchmark: 1 } },
    },
  );
}

describe("D-27 measures", () => {
  it("show real values, each labelled, instead of unknown", () => {
    const by = Object.fromEntries(meteringRows(d27()).map((r) => [r.label, r]));
    expect(by["Time to first token p50 / p95"].value).toBe("180 ms / 420 ms (the model server's own timing)");
    expect(by["Tokens per second"].value).toBe("30.0 (over 5 turns)");
    expect(by["Peak CPU / GPU / RAM (whole machine)"].value).toBe("87.50% / 91.00% / 12288 MiB of 16384 MiB");
    expect(by["Energy (estimate, not measured)"].value).toBe("41.667 mWh, estimate over 5 turns");
    expect(by["Self-review (opinion, not a verdict)"].value).toBe("PASS 3 / FAIL 1 / unclear 0 (opinion; agreed with verifiers 3, disagreed 1)");
    expect(by["SALT spent on chain"].value).toBe("0.50015 SALT (gas 0.00015 SALT, value sent 0.5 SALT)");
    expect(by["Gas used"].value).toBe("150000 over 2 transactions, 1 reverted");
    for (const r of d27Rows(d27())) expect(r.unknown, r.label).toBe(false);
  });

  it("a day without them says unknown and why, never zero", () => {
    const empty = daily({ schema: 2, ttft_ms: null, speed: null, resources: null, energy_estimate: null, self_review: { label: "opinion", pass: 0, fail: 0, unclear: 0, agreed_with_verifiers: 0, disagreed_with_verifiers: 0 } }, { notMeasured: [], chain: { day: "2026-10-01", transactions: 0, reverted: 0, gasUsed: 0, feeWei: "0", valueWei: "0", saltSpentWei: "0", byPurpose: {} } });
    const by = Object.fromEntries(meteringRows(empty).map((r) => [r.label, r]));
    expect(by["Time to first token p50 / p95"].value).toMatch(/^unknown \(the model server did not report it\)/);
    expect(by["Tokens per second"].unknown).toBe(true);
    expect(by["Peak CPU / GPU / RAM (whole machine)"].value).toMatch(/^unknown/);
    expect(by["Energy (estimate, not measured)"].value).toMatch(/^unknown/);
    expect(by["Self-review (opinion, not a verdict)"].value).toBe("none recorded");
    expect(by["SALT spent on chain"].value).toBe("none (no Hermes transaction this day)");
  });

  it("an older sidecar without the fields shows them as not measured by that build", () => {
    const by = Object.fromEntries(meteringRows(daily()).map((r) => [r.label, r]));
    for (const l of ["Time to first token p50 / p95", "Tokens per second", "Energy (estimate, not measured)", "SALT spent on chain", "Gas used"]) {
      expect(by[l].value, l).toBe("unknown (this Hermes build does not measure it)");
      expect(by[l].unknown).toBe(true);
    }
  });

  it("a GPU with no reading stays unknown inside the peak row", () => {
    const d = d27();
    d.report.resources = { ...d.report.resources!, gpu_peak_bps: null };
    const by = Object.fromEntries(meteringRows(d).map((r) => [r.label, r]));
    expect(by["Peak CPU / GPU / RAM (whole machine)"].value).toBe("87.50% / GPU unknown / 12288 MiB of 16384 MiB");
  });

  it("formats SALT exactly from wei and refuses anything else", () => {
    expect(formatSalt("2000000000000000000")).toBe("2 SALT");
    expect(formatSalt("1")).toBe("0.000000000000000001 SALT");
    expect(formatSalt("0")).toBe("0 SALT");
    for (const bad of ["", "-1", "1e18", "0x10", "1".repeat(40)]) expect(formatSalt(bad), bad).toBeNull();
  });

  it("the Journal panel renders them", async () => {
    const invoke = makeInvoke({ hermes_metering_daily: () => d27(), hermes_chain_status: () => chain() });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => NOW} />);
    const text = host.textContent ?? "";
    expect(text).toContain("180 ms / 420 ms");
    expect(text).toContain("41.667 mWh, estimate");
    expect(text).toContain("0.50015 SALT");
    expect(text).toContain("Self-review (opinion, not a verdict)");
    expect(text).not.toMatch(/unknown \(not measured yet\)/);
    expect(text).not.toMatch(/—/);
    root.unmount();
  });
});

describe("HermesDailyReport", () => {
  it("outside the desktop app it says so and calls nothing", async () => {
    const invoke = makeInvoke({});
    const { host, root } = await mount(<HermesDailyReport mode="sim" invoke={invoke} now={() => NOW} />);
    expect(q(host, "hm-desktop-only")).toBeTruthy();
    expect(invoke).not.toHaveBeenCalled();
    root.unmount();
  });

  it("renders today's report, the anchor line and a disabled benchmark toggle", async () => {
    const invoke = makeInvoke({ hermes_metering_daily: () => daily(), hermes_chain_status: () => chain() });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => NOW} />);
    expect(invoke).toHaveBeenCalledWith("hermes_metering_daily", { day: "2026-10-01" });
    expect(host.textContent).toContain("83.33%");
    expect(q(host, "hm-source")?.textContent).toMatch(/stored on this device/);
    expect(q(host, "hm-anchor-line")?.textContent).toMatch(/not in this app's 40204 address book/);
    const bench = q<HTMLInputElement>(host, "hm-bench-toggle");
    expect(bench?.disabled).toBe(true);
    expect(bench?.checked).toBe(false);
    expect(q<HTMLInputElement>(host, "hm-anchor-toggle")?.disabled).toBe(true);
    expect(q(host, "hm-bench-line")?.textContent).toMatch(/BenchmarkRegistry is not in this app's 40204 address book/);
    expect(host.textContent).toMatch(/Pending owner sign-off/);
    expect(host.textContent).not.toMatch(/—/);
    root.unmount();
  });

  it("when Hermes is not running the numbers are unknown, with the reason", async () => {
    const invoke = makeInvoke({
      hermes_metering_daily: () => {
        throw new Error("Hermes is not running, so today's numbers are unknown.");
      },
      hermes_chain_status: () => chain(),
    });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => NOW} />);
    expect(q(host, "hm-error")?.textContent).toMatch(/not running/);
    expect(q(host, "hm-rows")).toBeNull();
    root.unmount();
  });

  it("an in-memory report says it will not survive a restart", async () => {
    const invoke = makeInvoke({ hermes_metering_daily: () => daily({}, { source: "memory", persisted: false }), hermes_chain_status: () => chain() });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => NOW} />);
    expect(q(host, "hm-source")?.textContent).toMatch(/until Hermes restarts/);
    root.unmount();
  });

  it("Yesterday asks for the previous UTC day", async () => {
    const invoke = makeInvoke({ hermes_metering_daily: (a) => daily({}, { day: String(a.day) }), hermes_chain_status: () => chain() });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => NOW} />);
    await click(q(host, "hm-yesterday"));
    expect(invoke).toHaveBeenCalledWith("hermes_metering_daily", { day: "2026-09-30" });
    root.unmount();
  });

  it("once deployed, the benchmark toggle sends the member's choice", async () => {
    const invoke = makeInvoke({
      hermes_metering_daily: () => daily(),
      hermes_chain_status: () => chain({ deployed: true }),
      hermes_chain_settings_set: () => chain({ deployed: true }),
    });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => NOW} />);
    const bench = q<HTMLInputElement>(host, "hm-bench-toggle");
    expect(bench?.disabled).toBe(false);
    await click(bench);
    expect(invoke).toHaveBeenCalledWith("hermes_chain_settings_set", { anchorNightly: false, shareBenchmarks: true });
    root.unmount();
  });

  it("a pending anchor card can be approved or rejected by its id", async () => {
    const card = {
      id: "7",
      origin: "hermes:nightly-anchor",
      day: 20361,
      date: "2026-09-30",
      commitment: "0x" + "ab".repeat(32),
      registry: "0x00000000000000000000000000000000000000a1",
      chainId: 40204,
      decoded: { action: "Anchor the decision records of 2026-09-30 (UTC) to AnchorRegistry", cost: "0 SALT value; gas is paid by the anchor key", destination: "0x00000000000000000000000000000000000000a1" },
    };
    const invoke = makeInvoke({
      hermes_metering_daily: () => daily(),
      hermes_chain_status: () => chain({ deployed: true, pending: [card] }),
      hermes_anchor_approve: () => ({ receipt: { day: 20361, commitment: card.commitment, txHash: "0x" + "cd".repeat(32), blockNumber: null, status: null }, anchored: false, statusLine: "Sent. Waiting for the block; the day is not marked anchored yet." }),
      hermes_anchor_reject: () => null,
    });
    const { host, root } = await mount(<HermesDailyReport mode="tauri" invoke={invoke} now={() => NOW} />);
    expect(host.textContent).toContain(card.decoded.action);
    await click(q(host, "hm-anchor-approve-7"));
    expect(invoke).toHaveBeenCalledWith("hermes_anchor_approve", { id: "7" });
    expect(q(host, "hm-anchor-result")?.textContent).toMatch(/not marked anchored yet/);
    await click(q(host, "hm-anchor-reject-7"));
    expect(invoke).toHaveBeenCalledWith("hermes_anchor_reject", { id: "7" });
    root.unmount();
  });
});
