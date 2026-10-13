// v0.5.0-rc.5 FIX 2 — sign-in runs in the SYSTEM browser (passkeys need it on macOS).
// There is no in-app window whose close aborts the wait, so the app's Cancel must
// actually cancel the Rust loopback wait (auth_login_cancel). Before this, S1's Cancel
// only reset the UI while the flow kept listening, and a late browser completion could
// still sign the member in behind a screen that said "cancelled".
import { describe, it, expect, vi, beforeEach } from "vitest";

const cancelLoginMock = vi.fn(async () => true);
let resolveLogin: ((v: unknown) => void) | null = null;
let rejectLogin: ((e: unknown) => void) | null = null;
const loginMock = vi.fn(
  () =>
    new Promise((res, rej) => {
      resolveLogin = res;
      rejectLogin = rej;
    }),
);

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    auth: {
      status: async () => ({ signedIn: false }),
      login: () => loginMock(),
      userinfo: async () => ({ signedIn: false }),
      cancelLogin: () => cancelLoginMock(),
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";

async function flush(): Promise<void> {
  for (let i = 0; i < 6; i++) await Promise.resolve();
}

describe("sign-in cancel (FIX 2)", () => {
  beforeEach(() => {
    cancelLoginMock.mockClear();
    loginMock.mockClear();
    resolveLogin = null;
    rejectLogin = null;
  });

  it("S1 Cancel aborts the in-flight browser sign-in in Rust and returns to idle", async () => {
    const store = new Store();
    store.onS1Start();
    expect(store.state.s1).toBe("waiting");
    expect(loginMock).toHaveBeenCalledTimes(1);
    store.onS1Cancel();
    expect(cancelLoginMock).toHaveBeenCalledTimes(1);
    expect(store.state.s1).toBe("idle");
    // Rust answers the cancelled wait with an error; S1 stays idle, never "done".
    rejectLogin?.(new Error("auth: sign-in cancelled"));
    await flush();
    expect(store.state.s1).toBe("idle");
    expect(store.state.signedIn).toBeFalsy();
  });

  it("cancelSignIn is a clean no-op when the cancel command fails", async () => {
    cancelLoginMock.mockRejectedValueOnce(new Error("unavailable"));
    const store = new Store();
    await expect(store.cancelSignIn()).resolves.toBeUndefined();
  });

  it("a normal completion still signs in (cancel was never pressed)", async () => {
    const store = new Store();
    store.onS1Start();
    resolveLogin?.({ signedIn: true, sub: "u", tier: "pilot", org: null, role: "member", kycStatus: null, email: "a@b.co" });
    await flush();
    expect(cancelLoginMock).not.toHaveBeenCalled();
    expect(store.state.s1).toBe("done");
  });
});
