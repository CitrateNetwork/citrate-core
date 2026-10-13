// v0.5.0-rc.5 FIX 5 — the contract toolchain card: per-tool status, the install command for each
// missing tool on this OS, Re-check (re-runs core's probe), and Restart Hermes.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ToolchainCard, type ToolchainCardOps } from "./ToolchainCard";
import type { ToolchainStatus } from "../agent/contractForge";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function status(enabled: boolean, found: string[], solc: boolean): ToolchainStatus {
  return {
    settings: { enabled },
    programs: [
      ["forge", "forge_test"],
      ["slither", "slither_scan"],
      ["aderyn", "aderyn_scan"],
      ["medusa", "medusa_fuzz"],
    ].map(([program, tool]) => ({ program, tool, path: found.includes(program) ? "/x/" + program : null })),
    solc: solc ? "/x/solc" : null,
    searchPath: [],
    notices: [],
    appliesOnRestart: true,
    loadError: null,
  };
}

let host: HTMLDivElement;
let root: Root;
async function mount(ops: ToolchainCardOps, os: "macos" | "linux" | "windows" = "macos") {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(<ToolchainCard ops={ops} os={os} />));
}
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});
const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

describe("ToolchainCard", () => {
  it("shows each program's status and the install command for each missing one", async () => {
    await mount({ toolchainSettings: vi.fn(async () => status(true, ["forge", "slither"], false)), toolchainSetEnabled: vi.fn() });
    expect(q("cf-tc-forge")?.textContent).toContain("found");
    expect(q("cf-tc-aderyn")?.textContent).toContain("not installed");
    expect(q("cf-tc-solc")?.textContent).toContain("not found");
    expect(q("tc-install-aderyn")?.textContent).toBe("aderyn: brew install cyfrin/tap/aderyn");
    expect(q("tc-install-medusa")?.textContent).toContain("brew install medusa");
    expect(q("tc-install-solc")?.textContent).toContain("svm install 0.8.36");
    expect(q("tc-install-forge")).toBeNull();
    expect(q("tc-applies")?.textContent).toContain("Changes apply when Hermes restarts.");
  });

  it("uses the Linux commands on Linux", async () => {
    await mount({ toolchainSettings: vi.fn(async () => status(true, [], true)), toolchainSetEnabled: vi.fn() }, "linux");
    expect(q("tc-install-medusa")?.textContent).toContain("go install github.com/crytic/medusa@latest");
  });

  it("says Windows is not supported in 0.5.0 instead of listing commands", async () => {
    await mount({ toolchainSettings: vi.fn(async () => status(false, [], false)), toolchainSetEnabled: vi.fn() }, "windows");
    expect(q("tc-windows")?.textContent).toMatch(/not supported on Windows in 0.5.0/);
    expect(q("tc-install")).toBeNull();
  });

  it("Re-check re-runs the status probe and shows the new result", async () => {
    const probe = vi.fn(async () => status(true, [], true));
    await mount({ toolchainSettings: probe, toolchainSetEnabled: vi.fn() });
    expect(q("cf-tc-forge")?.textContent).toContain("not installed");
    probe.mockResolvedValueOnce(status(true, ["forge", "slither", "aderyn", "medusa"], true));
    await act(async () => q("tc-recheck")?.click());
    expect(probe).toHaveBeenCalledTimes(2);
    expect(q("cf-tc-forge")?.textContent).toContain("found");
    expect(q("tc-install")).toBeNull();
  });

  it("the switch calls core and shows its answer", async () => {
    const set = vi.fn(async (on: boolean) => status(on, [], true));
    await mount({ toolchainSettings: vi.fn(async () => status(false, [], true)), toolchainSetEnabled: set });
    await act(async () => (q("cf-tc-toggle") as HTMLInputElement).click());
    expect(set).toHaveBeenCalledWith(true);
    expect((q("cf-tc-toggle") as HTMLInputElement).checked).toBe(true);
  });

  it("Restart Hermes appears only when a restart is available, and runs it", async () => {
    const restart = vi.fn(async () => {});
    await mount({ toolchainSettings: vi.fn(async () => status(true, [], true)), toolchainSetEnabled: vi.fn(), restartHermes: restart });
    await act(async () => q("tc-restart")?.click());
    expect(restart).toHaveBeenCalledTimes(1);
    expect(q("tc-applies")?.textContent).toContain("Hermes restarted.");
  });

  it("no restart control without a restart op", async () => {
    await mount({ toolchainSettings: vi.fn(async () => status(true, [], true)), toolchainSetEnabled: vi.fn() });
    expect(q("tc-restart")).toBeNull();
  });

  it("a failed probe shows the error honestly", async () => {
    await mount({ toolchainSettings: vi.fn(async () => Promise.reject(new Error("core unavailable"))), toolchainSetEnabled: vi.fn() });
    expect(q("cf-tc-error")?.textContent).toBe("core unavailable");
  });
});
