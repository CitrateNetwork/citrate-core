// HUP-S1.6 (US-1.6) — onboarding shows the machine's tier + rationale and an override select.
// Unknown hardware is shown as unknown (Rule 1).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { TierView } from "./TierPanel";
import { sampleReport } from "../shell/slices/tierTestReport";

const noop = () => {};

describe("HUP-S1.6 TierView", () => {
  it("names the tier, the memory, and every rationale line", () => {
    const html = renderToStaticMarkup(<TierView report={sampleReport()} loaded saving={false} error={null} onOverride={noop} />);
    expect(html).toContain("Your machine: T2");
    expect(html).toContain("32 GB unified memory");
    expect(html).toContain("24 GB or more usable → T2");
    expect(html).toContain("Qwen 3.6 35B-A3B");
    expect(html).toContain("64k context");
  });

  it("offers an override select with T0, T1 and T2 plus the recommendation", () => {
    const html = renderToStaticMarkup(<TierView report={sampleReport()} loaded saving={false} error={null} onOverride={noop} />);
    expect(html).toContain('data-testid="tier-override"');
    for (const t of ["T0", "T1", "T2"]) expect(html).toContain(`value="${t}"`);
    expect(html).toContain("Recommended (T2)");
  });

  it("shows the override in effect and its model", () => {
    const html = renderToStaticMarkup(
      <TierView report={sampleReport({ overrideTier: "T0", effective: "T0" })} loaded saving={false} error={null} onOverride={noop} />,
    );
    expect(html).toContain("Your machine: T2");
    expect(html).toContain("Using T0 (your choice)");
    expect(html).toContain("Gemma 4 E4B");
    expect(html).toContain("16k context");
  });

  it("says the tier context is a target the model start caps, applied at the next start", () => {
    const html = renderToStaticMarkup(<TierView report={sampleReport()} loaded saving={false} error={null} onOverride={noop} />);
    expect(html).toContain('data-testid="tier-ctx-note"');
    expect(html).toContain("the next time the local model starts");
    expect(html).toContain("what the model file supports");
    expect(html).toContain("memory free beside the node");
  });

  it("T0 recommends escalation (guided tier)", () => {
    const rep = sampleReport();
    rep.recommendation = { ...rep.profiles[0], rationale: ["Under 12 GB usable → T0"], guided: true, usableBytes: 9 * 2 ** 30 };
    rep.effective = "T0";
    const html = renderToStaticMarkup(<TierView report={rep} loaded saving={false} error={null} onOverride={noop} />);
    expect(html).toContain("guided");
  });

  it("unknown hardware is shown as unknown, never a number", () => {
    const rep = sampleReport();
    rep.facts = { ...rep.facts, totalRamBytes: null, unifiedMemory: null, gpuVramBytes: null, diskFreeBytes: null };
    const html = renderToStaticMarkup(<TierView report={rep} loaded saving={false} error={null} onOverride={noop} />);
    expect(html).toContain("memory unknown");
    expect(html).toContain("GPU memory unknown");
  });

  it("with no report (web preview / failed probe) it says so honestly", () => {
    const html = renderToStaticMarkup(<TierView report={null} loaded saving={false} error={null} onOverride={noop} />);
    expect(html).toContain("needs the desktop app");
    expect(html).not.toContain("Your machine: T");
    const err = renderToStaticMarkup(<TierView report={null} loaded saving={false} error="probe failed" onOverride={noop} />);
    expect(err).toContain("probe failed");
  });

  it("renders nothing tier-specific before the probe returns", () => {
    const html = renderToStaticMarkup(<TierView report={null} loaded={false} saving={false} error={null} onOverride={noop} />);
    expect(html).toContain("Checking this machine");
  });
});
