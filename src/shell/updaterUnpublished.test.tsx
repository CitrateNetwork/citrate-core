// 0.5.4 review (#709, P2): when the release feed has no entry for this platform (0.5.4 ships
// Linux x86_64 only, so a Linux aarch64 install asks for "linux-aarch64"), the updater must not
// say "up to date": Settings › Updates says no update is published for this platform and names it.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
vi.mock("../bridge/tauri/invoke", () => ({
  invoke: vi.fn(async (cmd: string) =>
    cmd.startsWith("update_policy") ? { currentVersion: "0.5.4", minimumVersion: null, updateRequired: false, reason: null, refused: null } : undefined,
  ),
}));
const NO_PLATFORM = 'None of the fallback platforms `["linux-aarch64-appimage", "linux-aarch64"]` were found in the response `platforms` object';
const pluginCheck = vi.fn(async (): Promise<unknown> => {
  throw new Error(NO_PLATFORM);
});
vi.mock("@tauri-apps/plugin-updater", () => ({ check: () => pluginCheck() }));

import { feedPlatformOf, updateCheckText, useAppUpdate, INITIAL_UPDATE_STATE } from "./updater";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Probe() {
  const u = useAppUpdate();
  return <span data-testid="probe">{updateCheckText(u.state)}</span>;
}

let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
});

describe("updater: no entry for this platform in the feed", () => {
  it("names the platform target from the updater's error", () => {
    expect(feedPlatformOf(NO_PLATFORM)).toBe("linux-aarch64-appimage");
    expect(feedPlatformOf("the platform `windows-x86_64` was not found on the response `platforms` object")).toBe("windows-x86_64");
    expect(feedPlatformOf("platform not available")).toBe("this platform");
  });

  it("a check that finds no entry says nothing is published for it, not up to date", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => root!.render(<Probe />));
    for (let i = 0; i < 5; i++) await act(async () => {});
    expect(pluginCheck).toHaveBeenCalled();
    expect(host.textContent).toBe("no update is published for this platform (linux-aarch64-appimage)");
    expect(host.textContent).not.toMatch(/up to date/);
  });

  it("a plain up-to-date result still reads up to date", () => {
    expect(updateCheckText({ ...INITIAL_UPDATE_STATE, status: "uptodate" })).toBe("up to date");
  });
});
