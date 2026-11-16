// #709 review (P2): once an update is downloaded, a check (Check now or the 6 h timer) must not
// replace it with a fresh, undownloaded handle, and Restart installs only the downloaded handle,
// never draining the services without one.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
const invoke = vi.fn(async (cmd: string): Promise<unknown> => {
  if (cmd.startsWith("update_policy")) return { currentVersion: "0.5.4", minimumVersion: null, updateRequired: false, reason: null, refused: null };
  if (cmd === "update_prepare") return { allStopped: true, services: [], tookMs: 1 };
  return undefined;
});
vi.mock("../bridge/tauri/invoke", () => ({ invoke: (cmd: string) => invoke(cmd) }));
const relaunch = vi.fn(async () => {});
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: () => relaunch() }));

type Handle = { version: string; body: null; download: ReturnType<typeof vi.fn>; install: ReturnType<typeof vi.fn> };
const handle = (version: string): Handle => ({
  version,
  body: null,
  download: vi.fn(async (on: (e: { event: string }) => void) => on({ event: "Finished" })),
  install: vi.fn(async () => {}),
});
let next: Handle[] = [];
const pluginCheck = vi.fn(async () => next.shift() ?? null);
vi.mock("@tauri-apps/plugin-updater", () => ({ check: () => pluginCheck() }));

import { useAppUpdate } from "./updater";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let api: ReturnType<typeof useAppUpdate> | null = null;
function Probe() {
  api = useAppUpdate();
  return <span data-testid="st">{api.state.status}</span>;
}
let root: Root | null = null;
let host: HTMLDivElement;
const flush = async () => {
  for (let i = 0; i < 6; i++) await act(async () => {});
};
beforeEach(async () => {
  pluginCheck.mockClear();
  invoke.mockClear();
  relaunch.mockClear();
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  api = null;
  next = [];
});

describe("updater: a downloaded update is kept until Restart", () => {
  it("Given a downloaded update, a later check does nothing and Restart installs the downloaded handle", async () => {
    const first = handle("0.5.5");
    const second = handle("0.5.5");
    next = [first, second];
    await act(async () => root!.render(<Probe />));
    await flush();
    expect(api!.state.status).toBe("available");
    await act(async () => void (await api!.install()));
    await flush();
    expect(api!.state.status).toBe("ready");
    const calls = pluginCheck.mock.calls.length;
    await act(async () => void (await api!.check()));
    await flush();
    expect(pluginCheck.mock.calls.length).toBe(calls);
    expect(api!.state.status).toBe("ready");
    await act(async () => void (await api!.restart()));
    await flush();
    expect(first.install).toHaveBeenCalledTimes(1);
    expect(second.install).not.toHaveBeenCalled();
    expect(relaunch).toHaveBeenCalledTimes(1);
  });

  it("Given no downloaded update, Restart stops nothing and says so", async () => {
    next = [handle("0.5.5")];
    await act(async () => root!.render(<Probe />));
    await flush();
    await act(async () => void (await api!.restart()));
    await flush();
    expect(invoke.mock.calls.map((c) => c[0])).not.toContain("update_prepare");
    expect(api!.state.status).toBe("error");
    expect(api!.state.error).toMatch(/nothing was stopped or installed/);
  });
});
