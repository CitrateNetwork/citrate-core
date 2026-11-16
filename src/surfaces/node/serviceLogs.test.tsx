// 0.5.4 — Node › Advanced "Service logs" reads any of the nine services' log files through
// sidecar_logs_tail: the newest lines, what an empty file means, a failed read with its call.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ServiceLogs, SERVICE_LOG_LINES, openServiceLogs, type ServiceLogsIo } from "./ServiceLogs";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement;
async function mount(io: ServiceLogsIo) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<ServiceLogs io={io} />);
  });
  await act(async () => {});
}
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  openServiceLogs("node");
});

describe("Node › Advanced › Service logs", () => {
  it("reads the service Node › Services opened, and switching reads the other one", async () => {
    openServiceLogs("ipfs");
    const tail = vi.fn(async (service: string) => ({ service, file: service + ".log", lines: ["daemon is ready", "pinned 2 blocks"], lifecycleOnly: false }) as never);
    await mount({ tail });
    expect(tail).toHaveBeenCalledWith("ipfs", SERVICE_LOG_LINES);
    const lines = [...host.querySelectorAll("[data-log-line]")].map((l) => l.textContent?.trim());
    expect(lines).toEqual(["daemon is ready", "pinned 2 blocks"]);
    const select = host.querySelector("select")!;
    await act(async () => {
      select.value = "cluster";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => {});
    expect(tail).toHaveBeenLastCalledWith("cluster", SERVICE_LOG_LINES);
  });

  it("an empty log says the service has not written one; a lifecycle-only log says why it is short", async () => {
    openServiceLogs("hermes");
    await mount({ tail: async () => ({ service: "hermes", file: "hermes.log", lines: [], lifecycleOnly: true }) });
    expect(host.querySelector('[data-testid="service-log"]')?.textContent).toMatch(/No log lines from Hermes yet/);
    expect(host.querySelector('[data-testid="service-logs-lifecycle"]')?.textContent).toMatch(/keeps only start, stop and error lines/);
  });

  it("a failed read names the call and its error", async () => {
    openServiceLogs("memory");
    await mount({ tail: async () => Promise.reject(new Error("the sidecar log folder is not set up")) });
    expect(host.querySelector('[data-testid="service-logs-error"]')?.textContent).toBe("Couldn’t read the Memory log (sidecar_logs_tail): the sidecar log folder is not set up");
  });
});
