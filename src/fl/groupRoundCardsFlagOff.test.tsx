// FL round cards (0.5.2, first cut): with the flag OFF the app renders exactly what it rendered
// before the feature existed. The snapshots in __snapshots__/ were written from the code BEFORE the
// round cards were added (the commit that adds this file changes no surface), so a later diff in
// these strings means flag OFF changed behaviour.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { WagmiProvider } from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig } from "../wagmi";
import { PlaceView } from "../shell/PlaceView";
import { store } from "../shell/store";
import { freshState } from "../shell/state";
import { Groups } from "../surfaces/Groups";
import { groupsSlice } from "../shell/slices/groups";
import type { Store } from "../shell/store";

const rounds = () =>
  renderToStaticMarkup(
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={new QueryClient()}>
        <PlaceView store={store} s={{ ...freshState("p1"), stage: "done", route: "groups/rounds" }} />
      </QueryClientProvider>
    </WagmiProvider>,
  );

const stubStore = { toast: () => {}, identity: () => ({ wallet: "" }) } as unknown as Store;
const GROUP = { id: "0x" + "ab".repeat(16), name: "Garden club", owner: "0x" + "11".repeat(20), kind: "channel" as const, members: [] };
const chat = () => {
  groupsSlice.set({ groups: [GROUP], selectedId: GROUP.id, roster: [], messages: [] });
  return renderToStaticMarkup(<Groups store={stubStore} s={freshState("p1")} />);
};

describe("FL round cards flag OFF: behaviour is unchanged", () => {
  beforeEach(() => vi.stubEnv("VITE_FL_ROUND_CARDS", ""));
  afterEach(() => {
    vi.unstubAllEnvs();
    groupsSlice.set({ groups: [], selectedId: null });
  });

  it("Groups › Rounds renders the same markup as before the feature", () => {
    expect(rounds()).toMatchSnapshot();
  });

  it("a group's chat renders the same markup as before the feature", () => {
    expect(chat()).toMatchSnapshot();
  });
});
