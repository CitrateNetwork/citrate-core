// Five-place navigation (0.5.2): AI providers moved from Settings to Hermes › Controls. The
// "Cloud providers" row on Connections must open that new home on the AI providers section, not
// plain Settings (which no longer lists AI providers and would land on an unrelated account page).
import { afterEach, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Connections } from "./Connections";
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
});

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<Connections store={store} s={freshState("p1")} />);
  });
}

const providerLink = () => host?.querySelector('[data-testid="cloud-providers-link"]') as HTMLAnchorElement | null;

describe("Connections › Cloud providers link", () => {
  it("opens Hermes › Controls on the AI providers section", async () => {
    store.setState({ route: "settings/connections", sSec: "account" });
    await mount();
    const a = providerLink();
    expect(a).toBeTruthy();
    await act(async () => {
      a?.click();
    });
    expect(store.state.route).toBe("hermes/controls");
    expect(store.state.sSec).toBe("ai");
  });

  it("names the new home and links to it, not to plain Settings", async () => {
    await mount();
    const a = providerLink();
    expect(a?.getAttribute("href")).toBe("#/settings/ai");
    expect(a?.textContent).toContain("Hermes › Controls");
    expect(host?.querySelector('a[href="#/settings"]')).toBeNull();
  });
});
