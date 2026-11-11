// CORE-D5 — Node › Services: each health row names its sidecar's log file on disk, and "Save
// diagnostics bundle" writes the scrubbed bundle to a file the member picks (Rust
// `sidecar_logs.rs` `diagnostics_logs_export`). Every word comes from what core answered.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { DiagnosticsExportBar, SidecarsHealthView, exportText } from "./NodeServices";
import type { DiagnosticsExport, SidecarHealth } from "../bridge/domains";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriServices } from "../bridge/tauri/services";
import { simServices } from "../bridge/sim/services";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_790_000_000_000;

function row(over: Partial<SidecarHealth> = {}): SidecarHealth {
  return {
    id: "node",
    label: "Node",
    owner: "src-tauri/src/node.rs",
    prerequisites: [],
    probe: "RPC",
    readySignal: "head advancing",
    logBudgetLines: 500,
    logsCommand: "node_logs",
    controls: true,
    observation: "observed",
    phase: "alive",
    readiness: "ready",
    lastTransition: { from: "starting", to: "alive", atUnixMs: NOW - 5_000 },
    restarts: 0,
    lastCrash: null,
    quarantine: null,
    counters: null,
    slowRetry: null,
    logTail: [],
    logLinesKept: 0,
    note: null,
    ...over,
  };
}

const done: DiagnosticsExport = { path: "/Users/m/Desktop/citrate-diagnostics-2026-11-10.txt", bytes: 52_000, sidecars: ["citrate", "hermes"], logLines: 1234 };

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
async function mount(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(el);
  });
  return host;
}
const $ = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
async function click(el: HTMLElement | null) {
  await act(async () => {
    el?.click();
  });
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

describe("CORE-D5: log files on the health rows", () => {
  it("names the log file, and says when lines could not be written", () => {
    const ok = renderToStaticMarkup(
      <SidecarsHealthView health={{ takenAtUnixMs: NOW, services: [row({ logFile: { path: "~/Library/Logs/ai.citrate.core/sidecars/citrate.log", writeFailures: 0, lastError: null } })] }} error={null} alerts={[]} />,
    );
    expect(ok).toContain("Log file: ~/Library/Logs/ai.citrate.core/sidecars/citrate.log");
    expect(ok).not.toContain("could not be written");

    const bad = renderToStaticMarkup(
      <SidecarsHealthView health={{ takenAtUnixMs: NOW, services: [row({ logFile: { path: "~/x/citrate.log", writeFailures: 3, lastError: "No space left on device" } })] }} error={null} alerts={[]} />,
    );
    expect(bad).toContain("3 lines could not be written (No space left on device)");
  });

  it("claims no log file when core reported none", () => {
    for (const logFile of [null, undefined]) {
      const html = renderToStaticMarkup(<SidecarsHealthView health={{ takenAtUnixMs: NOW, services: [row({ logFile })] }} error={null} alerts={[]} />);
      expect(html).not.toContain("Log file:");
    }
  });
});

describe("CORE-D5: Save diagnostics bundle", () => {
  it("writes to the picked file and says what is in it", async () => {
    const pickPath = vi.fn(async () => done.path);
    const exportTo = vi.fn(async () => done);
    const el = await mount(<DiagnosticsExportBar pickPath={pickPath} exportTo={exportTo} />);
    await click($(el, "diag-export"));
    expect(exportTo).toHaveBeenCalledWith(done.path);
    const text = $(el, "diag-export-out")?.textContent ?? "";
    expect(text).toContain(`Saved ${done.path} (51 KB, 1234 log lines from citrate, hermes)`);
    expect(text).toMatch(/Read it before you share it/);
    // #613 review: the scrub is best effort, and the text says so.
    expect(text).toMatch(/best-effort/);
    expect(text).toMatch(/model, memory, messaging and Hermes logs keep only start, stop and error lines/);
    expect(text).toMatch(/Nothing was sent\./);
  });

  it("a cancelled picker writes nothing and says nothing", async () => {
    const exportTo = vi.fn(async () => done);
    const el = await mount(<DiagnosticsExportBar pickPath={async () => null} exportTo={exportTo} />);
    await click($(el, "diag-export"));
    expect(exportTo).not.toHaveBeenCalled();
    expect($(el, "diag-export-out")).toBeNull();
  });

  it("a refusal from core is shown as core gave it", async () => {
    const el = await mount(
      <DiagnosticsExportBar
        pickPath={async () => "/tmp/x.txt"}
        exportTo={async () => {
          throw new Error("the bundle still holds something shaped like a provider key");
        }}
      />,
    );
    await click($(el, "diag-export"));
    expect($(el, "diag-export-out")?.textContent).toBe("The diagnostics bundle was not saved: the bundle still holds something shaped like a provider key");
    expect($(el, "diag-export-out")?.getAttribute("role")).toBe("alert");
  });

  it("says so when there are no sidecar logs yet", () => {
    expect(exportText({ path: "/tmp/a.txt", bytes: 100, sidecars: [], logLines: 0 })).toContain("(1 KB, no sidecar logs yet)");
  });
});

describe("CORE-D5: the bridge", () => {
  beforeEach(() => invokeMock.mockReset());

  it("tauri: exportDiagnostics calls diagnostics_logs_export with the path", async () => {
    invokeMock.mockResolvedValue(done);
    await expect(tauriServices.exportDiagnostics("/tmp/b.txt")).resolves.toEqual(done);
    expect(invokeMock).toHaveBeenLastCalledWith("diagnostics_logs_export", { path: "/tmp/b.txt" });
  });

  it("sim: the browser preview has nothing to bundle and says so", async () => {
    const sim = simServices({} as never);
    await expect(sim.exportDiagnostics("/tmp/b.txt")).rejects.toThrow(/no sidecar logs/);
  });
});
