// 0.5.2 Join — opting out must forget unsent join steps everywhere, but keep the once-per-install
// promise for steps already sent. Written red-first against fa42846 (review findings on #365):
//   1. a report prepared (or still being prepared) before an opt-out must not survive it, even if
//      the member opts back in without leaving Settings;
//   2. opting out clears PENDING steps only: a step already sent is never queued again.
import { describe, it, expect, beforeEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DiagnosticReport } from "../components/DiagnosticReport";
import { bridge } from "../bridge";
import type { DiagnosticBundle } from "../bridge/domains";
import { reachedStep, pendingJoinSteps, markJoinStepsSent, setJoinFunnelConsent, setTelemetryConsent } from "./joinFunnel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let on = true;
beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
  on = true;
  setJoinFunnelConsent(() => on);
});

const bundleOf = (joinSteps: string[]): DiagnosticBundle => ({
  reportId: "rpt_ab",
  appVersion: "0.5.2",
  os: "macos",
  crashTail: "",
  nodeLogTail: "",
  uiErrors: [],
  joinSteps,
});

async function mount(): Promise<{ host: HTMLDivElement; root: Root; render: (enabled: boolean) => Promise<void> }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const render = async (enabled: boolean) => {
    await act(async () => root.render(<DiagnosticReport enabled={enabled} toast={() => {}} />));
  };
  await render(true);
  return { host, root, render };
}
const button = (host: HTMLElement, text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;

/** Opt out then back in, the way the Settings toggle does it, re-rendering in between. */
async function optOutAndBackIn(render: (enabled: boolean) => Promise<void>) {
  on = false;
  setTelemetryConsent(false);
  await render(false);
  on = true;
  setTelemetryConsent(true);
  await render(true);
}

describe("opting out invalidates prepared reports", () => {
  it("prepare -> off -> on: the old report is gone and cannot be sent", async () => {
    reachedStep("sign_in");
    vi.spyOn(bridge.telemetry, "bundle").mockResolvedValue(bundleOf(["sign_in"]));
    const send = vi.spyOn(bridge.telemetry, "send").mockResolvedValue(undefined);
    const { host, render } = await mount();
    await act(async () => button(host, "Prepare")?.click());
    expect(button(host, "Send this report")).toBeDefined();

    await optOutAndBackIn(render);

    expect(button(host, "Send this report")).toBeUndefined();
    expect(host.textContent).not.toContain('"joinSteps"');
    expect(button(host, "Prepare")).toBeDefined();
    expect(send).not.toHaveBeenCalled();
  });

  it("a preparation still in flight during the opt-out is discarded when it lands", async () => {
    reachedStep("kyc");
    let resolve: (b: DiagnosticBundle) => void = () => {};
    vi.spyOn(bridge.telemetry, "bundle").mockImplementation(() => new Promise<DiagnosticBundle>((r) => (resolve = r)));
    const send = vi.spyOn(bridge.telemetry, "send").mockResolvedValue(undefined);
    const { host, render } = await mount();
    await act(async () => button(host, "Prepare")?.click());

    await optOutAndBackIn(render);
    await act(async () => resolve(bundleOf(["kyc"])));

    expect(button(host, "Send this report")).toBeUndefined();
    expect(host.textContent).not.toContain('"joinSteps"');
    expect(send).not.toHaveBeenCalled();
  });

  it("a fresh report after opting back in carries no forgotten steps", async () => {
    reachedStep("wallet");
    const bundle = vi.spyOn(bridge.telemetry, "bundle").mockImplementation(async (_e, steps) => bundleOf(steps ?? []));
    const { host, render } = await mount();
    await optOutAndBackIn(render);
    await act(async () => button(host, "Prepare")?.click());
    expect(bundle).toHaveBeenLastCalledWith([], []);
  });
});

describe("opting out keeps the sent-step set", () => {
  it("a step sent before an opt-out is not queued again after opting back in", () => {
    reachedStep("open_app");
    reachedStep("node_synced");
    markJoinStepsSent(["open_app", "node_synced"]);
    reachedStep("wallet"); // pending, not sent
    on = false;
    setTelemetryConsent(false);
    on = true;
    setTelemetryConsent(true);
    expect(pendingJoinSteps()).toEqual([]); // the unsent step was forgotten
    reachedStep("open_app");
    reachedStep("node_synced");
    expect(pendingJoinSteps()).toEqual([]); // already sent: never again
    reachedStep("wallet");
    expect(pendingJoinSteps()).toEqual(["wallet"]); // forgotten, so it may be recorded anew
  });
});
