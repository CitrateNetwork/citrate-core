// The owner's report (0.5.8): the relay "constantly degrades". A dropped link that the comms daemon
// is redialing reads as reconnecting, not as a fault; only a daemon that cannot redial reads as down,
// and that says what to do.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { bridge } from "../bridge";
import { RelayStatusChip, RELAY_CHIP } from "./RelayStatusChip";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
async function mount(state: string) {
  vi.spyOn(bridge.groups, "relayStatus").mockResolvedValue(state);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<RelayStatusChip />);
  });
  await act(async () => {});
}
const chip = () => host.querySelector("span[title]") as HTMLElement;

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe("RelayStatusChip", () => {
  it("shows a redialing link as reconnecting, with what happens next", async () => {
    await mount("reconnecting");
    expect(chip().textContent).toContain("Reconnecting…");
    expect(chip().title).toMatch(/redialing it on its own/);
  });

  it("shows a link that will not come back alone as down, and says what to do", async () => {
    await mount("degraded");
    expect(chip().textContent).toContain("Relay down");
    expect(chip().title).toMatch(/Quit and reopen Citrate Core/);
  });

  it("shows a connected link as connected", async () => {
    await mount("connected");
    expect(chip().textContent).toContain("Relay connected");
  });

  it("has words for every state the backend reports", () => {
    for (const s of ["idle", "local", "connecting", "connected", "reconnecting", "degraded", "error"]) {
      expect(RELAY_CHIP[s as keyof typeof RELAY_CHIP]?.label, s).toBeTruthy();
    }
  });
});
