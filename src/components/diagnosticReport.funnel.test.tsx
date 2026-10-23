// 0.5.2 Join — the reviewed report is the only way join steps leave the machine. These pin the
// wiring in DiagnosticReport: off means no prepare control at all; on, the pending step ids ride
// in the bundle the member reviews, and after a send they are never sent again.
import { describe, it, expect, beforeEach, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { DiagnosticReport } from "./DiagnosticReport";
import { bridge } from "../bridge";
import { reachedStep, pendingJoinSteps, setJoinFunnelConsent } from "../privacy/joinFunnel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let consent = true;
beforeEach(() => {
  localStorage.clear();
  consent = true;
  setJoinFunnelConsent(() => consent);
  vi.restoreAllMocks();
});

async function mount(enabled: boolean) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<DiagnosticReport enabled={enabled} toast={() => {}} />));
  return host;
}
const button = (host: HTMLElement, text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;

describe("DiagnosticReport carries join steps only inside the reviewed report", () => {
  it("telemetry off: no way to prepare or send anything", async () => {
    consent = false;
    const bundle = vi.spyOn(bridge.telemetry, "bundle");
    const send = vi.spyOn(bridge.telemetry, "send");
    reachedStep("sign_in");
    const host = await mount(false);
    expect(button(host, "Prepare")).toBeUndefined();
    expect(bundle).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
  });

  it("telemetry on: pending steps are in the reviewed bundle, and are not sent twice", async () => {
    reachedStep("open_app");
    reachedStep("sign_in");
    const bundle = vi.spyOn(bridge.telemetry, "bundle");
    const send = vi.spyOn(bridge.telemetry, "send").mockResolvedValue(undefined);
    const host = await mount(true);
    await act(async () => button(host, "Prepare")?.click());
    expect(bundle).toHaveBeenCalledWith([], ["open_app", "sign_in"]);
    expect(host.textContent).toContain('"joinSteps"');
    await act(async () => button(host, "Send this report")?.click());
    expect(send).toHaveBeenCalledTimes(1);
    const sent = JSON.parse(String(send.mock.calls[0][0]));
    expect(sent.joinSteps).toEqual(["open_app", "sign_in"]);
    expect(pendingJoinSteps()).toEqual([]);
    reachedStep("sign_in");
    expect(pendingJoinSteps()).toEqual([]);
  });
});
