// K6-3: Settings › Network reads block 0 over the profile's RPC only while Node configuration is
// open. Another section open, or Settings closed, makes no block-0 read.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

const reads = { getBlock: 0 };
vi.mock("viem", async (orig) => {
  const real = await orig<typeof import("viem")>();
  return {
    ...real,
    createPublicClient: () => ({
      getBlock: async () => {
        reads.getBlock++;
        return { hash: null };
      },
      getBlockNumber: async () => 1n,
    }),
  };
});

import { useNetworkState } from "./NetworkPanel";
import { ACTIVE_PROFILE } from "./profile";
import { Settings } from "../surfaces/Settings";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const stubStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "s", wallet: "", tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

let root: Root | null = null;
async function mount(el: React.ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(el);
  });
  await act(async () => {});
}

function Probe({ enabled }: { enabled: boolean }) {
  const st = useNetworkState(ACTIVE_PROFILE, enabled);
  return <span>{st.status}</span>;
}

describe("block 0 is read only while Settings › Network is open (K6-3)", () => {
  beforeEach(() => {
    reads.getBlock = 0;
  });
  afterEach(async () => {
    await act(async () => root?.unmount());
    root = null;
  });

  it("the hook makes no read while disabled", async () => {
    await mount(<Probe enabled={false} />);
    expect(reads.getBlock).toBe(0);
  });

  it("the hook reads block 0 once while enabled (positive control)", async () => {
    await mount(<Probe enabled={true} />);
    expect(reads.getBlock).toBe(1);
  });

  for (const sSec of ["api", "account", "keys"] as AppState["sSec"][]) {
    it(`Settings with "${sSec}" open makes no block-0 read`, async () => {
      await mount(<Settings store={stubStore} s={{ ...freshState("p1"), sSec, signedIn: true }} />);
      expect(reads.getBlock).toBe(0);
    });
  }

  it("Settings with Node configuration open reads block 0 (positive control)", async () => {
    await mount(<Settings store={stubStore} s={{ ...freshState("p1"), sSec: "node", signedIn: true }} />);
    expect(reads.getBlock).toBe(1);
  });
});
