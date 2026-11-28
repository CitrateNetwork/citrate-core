// L6-WP05b: disconnect revokes at the provider first, then removes the token. When the provider
// does not confirm the revoke, the token is still removed on the member's request and the command
// rejects with a message that says so. The row must then read disconnected (status reloaded even
// on a rejection) and the message must reach the member.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Connections } from "./Connections";
import { bridge } from "../bridge";
import { store } from "../shell/store";
import { freshState } from "../shell/state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  host = null;
  root = null;
  vi.restoreAllMocks();
});

const REVOKE_FAILED =
  "Disconnected on this device: the token is removed from this computer. GitHub did not confirm it revoked access (the relay answered HTTP 502); you can remove Citrate Core in your GitHub account settings.";

describe("Connections › Disconnect when the provider revoke fails", () => {
  it("reloads the rows and shows the honest message", async () => {
    const status = vi
      .spyOn(bridge.connections, "status")
      .mockResolvedValueOnce([{ service: "github", connected: true, scope: "repo,read:org", connectedAt: 1_700_000_000 }])
      .mockResolvedValue([{ service: "github", connected: false, scope: null, connectedAt: null }]);
    vi.spyOn(bridge.connections, "disconnect").mockRejectedValue(new Error(REVOKE_FAILED));
    const toast = vi.spyOn(store, "toast");

    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<Connections store={store} s={freshState("p1")} />);
    });
    const button = () =>
      Array.from(host?.querySelectorAll("button") ?? []).find((b) => b.textContent === "Disconnect" || b.textContent === "Connect");
    expect(button()?.textContent).toBe("Disconnect");

    await act(async () => {
      button()?.click();
    });

    expect(status).toHaveBeenCalledTimes(2);
    expect(button()?.textContent).toBe("Connect");
    expect(toast).toHaveBeenCalledTimes(1);
    const shown = toast.mock.calls[0]?.[0] ?? "";
    expect(shown).toContain("GitHub did not confirm it revoked access");
    expect(shown).not.toContain("—");
  });
});
