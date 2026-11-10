// Five-place navigation (0.5.2): AI providers moved from Settings to Hermes › Controls. The
// "Cloud providers" row on Connections must open that new home on the Models section (D-11, the
// governed vendor connections), not plain Settings. Its copy claims no vendor this build cannot use.
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
  it("opens Hermes › Controls › Details on the Models section", async () => {
    store.setState({ route: "settings/connections", sSec: "account" });
    await mount();
    const a = providerLink();
    expect(a).toBeTruthy();
    await act(async () => {
      a?.click();
    });
    expect(store.state.route).toBe("hermes/controls/details");
    expect(store.state.sSec).toBe("models");
  });

  it("names the new home and links to it, not to plain Settings", async () => {
    await mount();
    const a = providerLink();
    expect(a?.getAttribute("href")).toBe("#/settings/models");
    expect(a?.textContent).toContain("Hermes › Controls › Details › Models");
    expect(host?.querySelector('a[href="#/settings"]')).toBeNull();
  });

  it("the copy claims no particular vendor works and has no em-dash (P-9c)", async () => {
    await mount();
    const row = providerLink()?.parentElement?.textContent ?? "";
    expect(row).toContain("Cloud providers");
    expect(row).not.toMatch(/Anthropic/);
    expect(row).not.toContain("—");
    expect(row).toContain("Models shows which vendors this build can use");
  });
});
