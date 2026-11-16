// 0.5.4 — Settings › Updates "Check now" runs the app's one updater check (no second updater),
// says what the last check found, and carries no stale "0.1.0-proto" or beta-channel copy.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { UpdatesCheck } from "./Settings";
import { INITIAL_UPDATE_STATE, publishAppUpdate, updateCheckText, type UpdateState } from "../shell/updater";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
async function mount() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<UpdatesCheck updText="citrate-core 0.5.4 · build abc · stable channel" />);
  });
  return host;
}
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  publishAppUpdate(null);
});

const st = (over: Partial<UpdateState>): UpdateState => ({ ...INITIAL_UPDATE_STATE, ...over });

describe("Settings › Updates", () => {
  it("Given the app's updater, then Check now runs its check", async () => {
    const check = vi.fn(async () => {});
    publishAppUpdate({ state: st({ status: "uptodate" }), check });
    const host = await mount();
    const btn = host.querySelector<HTMLButtonElement>('[data-testid="update-check-now"]')!;
    expect(btn.disabled).toBe(false);
    await act(async () => btn.click());
    expect(check).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[data-testid="update-check-status"]')?.textContent).toMatch(/^Last check: up to date\./);
    expect(host.textContent).not.toMatch(/0\.1\.0-proto|not available in this build|beta/);
  });

  it("Given a check in flight, then the button waits", async () => {
    publishAppUpdate({ state: st({ status: "checking" }), check: async () => {} });
    const host = await mount();
    const btn = host.querySelector<HTMLButtonElement>('[data-testid="update-check-now"]')!;
    expect(btn.disabled).toBe(true);
    expect(btn.textContent).toBe("Checking");
  });

  it("Given no updater (web preview), then the button is off and says why", async () => {
    const host = await mount();
    expect(host.querySelector<HTMLButtonElement>('[data-testid="update-check-now"]')!.disabled).toBe(true);
    expect(host.textContent).toMatch(/Update checks run in the desktop app/);
  });

  it("the status line names what the check found", () => {
    expect(updateCheckText(st({ status: "available", version: "0.5.5" }))).toBe("version 0.5.5 is available: download it from the update banner");
    expect(updateCheckText(st({ status: "error", error: "network down" }))).toBe("the check failed: network down");
    expect(updateCheckText(st({ status: "idle" }))).toBe("not checked yet");
  });
});

describe("Settings › Updates while an update is downloaded (#709 review)", () => {
  it("Check now is off while a download is pending, waiting for Restart, or installing", async () => {
    for (const status of ["downloading", "ready", "installing", "installed"] as const) {
      const check = vi.fn(async () => {});
      publishAppUpdate({ state: st({ status, version: "0.5.5" }), check });
      const host = await mount();
      const btn = host.querySelector<HTMLButtonElement>('[data-testid="update-check-now"]')!;
      expect(btn.disabled, status).toBe(true);
      await act(async () => root?.unmount());
      root = null;
    }
    expect(updateCheckText(st({ status: "ready", version: "0.5.5" }))).toMatch(/Check now is off until then/);
  });
});
