// L0-WP08 review round 2: placeView.render.test.tsx (an L0 fallback suite no lane can edit) runs
// here with a lane file that registers the tabs that broke its strip helpers:
//   - a Node tab whose label has characters the markup escapes ("&", "'", "<");
//   - a tab on Wallet, a place with one built-in tab, so the strip shows only because of the lane.
// l6.tsx is replaced the way bootstrap.test.tsx replaces it, so registration runs through the real
// index.ts, registry, places and PlaceView chain. Every test in the render suite must pass as is.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { WagmiProvider } from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig } from "../../wagmi";
import { PlaceView } from "../PlaceView";
import { store } from "../store";
import { freshState } from "../state";
import { registeredRoutes } from "./registry";
import "../placeView.render.test";

vi.mock("./l6", async () => {
  const { registerSurfaces } = await import("./registry");
  const { createElement: h } = await import("react");
  registerSurfaces([
    { route: "node/wp08-escaped", lane: "L6", tab: { place: "node", label: `Logs & "traces" <'raw'>`, order: 35 }, render: () => h("div", { "data-lane-surface": "node/wp08-escaped" }) },
    { route: "wallet/wp08-extra", lane: "L6", tab: { place: "wallet", label: "WP08 extra", order: 20 }, render: () => h("div", { "data-lane-surface": "wallet/wp08-extra" }) },
  ]);
  return {};
});

const strip = (route: string) => {
  const s = { ...freshState("p1"), stage: "done" as const, route };
  const m = renderToStaticMarkup(
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={new QueryClient()}>
        <PlaceView store={store} s={s} />
      </QueryClientProvider>
    </WagmiProvider>,
  ).match(/<div[^>]*data-testid="place-tabs"[^>]*>(.*?)<\/div>/);
  return m?.[1] ?? "";
};

describe("L0-WP08 render suite with lane tabs", () => {
  it("the lane tabs are registered and shown, so the render suite above runs against them", () => {
    expect(registeredRoutes()).toEqual(["node/wp08-escaped", "wallet/wp08-extra"]);
    expect(strip("node")).toContain("Logs &amp; &quot;traces&quot; &lt;&#x27;raw&#x27;&gt;");
    expect(strip("wallet")).toContain("WP08 extra");
  });
});
