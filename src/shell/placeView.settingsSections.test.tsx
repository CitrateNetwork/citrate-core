// Five-place navigation (0.5.2): Settings keeps the account-level sections. Each one in its sub-nav
// must open when clicked, including Connections (review on #343: the composition forced the
// Connections section back to Account, so it could not be reached from Settings).
import { afterEach, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { PlaceView } from "./PlaceView";
import { store, useStore } from "./store";
import { ACCOUNT_SETTINGS } from "./places";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Sub-nav label → a line only that section renders.
const SECTIONS: Record<string, [string, string]> = {
  account: ["Account & RBAC", "Entitlement claim"],
  connections: ["Connections", "Connections · mount as MCP tools for your agent"],
  api: ["API endpoints & keys", "RPC endpoint"],
  keys: ["Keys & security", "Custody vault"],
  billing: ["Memberships & billing", "Receipts"],
  privacy: ["Privacy & recovery", "Device key recovery"],
  app: ["App", "Telemetry"],
};

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  host = null;
  root = null;
});

/** The page as the app renders it: PlaceView over the live store, re-rendering on every change. */
function Live() {
  const s = useStore();
  return <PlaceView store={store} s={s} />;
}

async function openSettings(sSec = "account") {
  store.setState({ stage: "done", route: "settings", sSec });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<Live />);
  });
}

/** The section buttons in the Settings sub-nav (the first column of the Settings surface). */
function subNavButton(label: string): HTMLButtonElement | null {
  const surface = host?.querySelector('[data-surface="settings"]');
  const buttons = [...(surface?.querySelectorAll("button") ?? [])] as HTMLButtonElement[];
  return buttons.find((b) => b.textContent === label) ?? null;
}

describe("Settings sub-nav: every account-level section opens", () => {
  it("covers exactly the sections Settings owns", () => {
    expect(Object.keys(SECTIONS).sort()).toEqual([...ACCOUNT_SETTINGS].sort());
  });

  for (const [id, [label, marker]] of Object.entries(SECTIONS)) {
    it(`clicking "${label}" shows the ${id} section in Settings`, async () => {
      // Start on a different section so the click has to change it.
      await openSettings(id === "account" ? "app" : "account");
      const button = subNavButton(label);
      expect(button, label).toBeTruthy();
      await act(async () => {
        button?.click();
      });
      expect(store.state.route).toBe("settings");
      expect(store.state.sSec).toBe(id);
      expect(host?.querySelector('[data-surface="settings"]')?.textContent ?? "").toContain(marker);
    });
  }
});
