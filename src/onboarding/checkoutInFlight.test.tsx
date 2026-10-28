// core#584: one checkout attempt at a time on onboarding S3.
//
// Before checkout opens, onS3Pay reads the chain to see whether this account already holds the
// membership grant (the money guard: a held grant never goes to checkout). That read takes time.
// While it runs:
//   1. a second Pay (double click) must not start a second attempt: at most one checkout opens;
//   2. Back to the free node must stop the attempt: when the read resolves, no checkout opens;
//   3. Back and then choosing membership again must still end in one checkout, not two;
//   4. "Continue free" and Pay are disabled, so neither can race the read.
// The existing guards stay as they are: an unreadable chain still opens checkout (it is not
// evidence of a grant), and a held grant never opens it.
//
// These run against the real Store with a mocked bridge whose grant read stays pending until the
// test resolves it.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, useSyncExternalStore } from "react";
import { createRoot, type Root } from "react-dom/client";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

type GrantShape = { attributedStakeWei: string; hasSbt: boolean };
const NOT_GRANTED: GrantShape = { attributedStakeWei: "0", hasSbt: false };
const GRANTED: GrantShape = { attributedStakeWei: (32000n * 10n ** 18n).toString(), hasSbt: true };

const m = vi.hoisted(() => {
  type Pending = { resolve: (g: unknown) => void; reject: (e: unknown) => void };
  const pending: Pending[] = [];
  return {
    pending,
    checkout: vi.fn(async (_email?: string) => {}),
    grantStatus: vi.fn(
      (_addr: string) =>
        new Promise((resolve, reject) => {
          pending.push({ resolve, reject });
        }),
    ),
    userinfo: vi.fn(async () => ({ signedIn: true, sub: "usr_x", tier: "free", walletAddr: "0xabc", email: "d@example.com" })),
  };
});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: {
      checkout: (email?: string) => m.checkout(email),
      grantStatus: (addr: string) => m.grantStatus(addr),
    },
    auth: {
      status: async () => ({ signedIn: false }),
      userinfo: () => m.userinfo(),
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
// S3 reads no network name, but the module graph imports wagmi; keep its block hook inert.
vi.mock("wagmi", async (orig) => ({
  ...(await orig<typeof import("wagmi")>()),
  useBlock: () => ({ data: undefined }),
}));

import { Store } from "../shell/store";
import { S3 } from "./Onboarding";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness({ store }: { store: Store }) {
  const s = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  return <S3 store={store} s={s} />;
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mountS3(store: Store): Promise<HTMLDivElement> {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<Harness store={store} />);
  });
  return host;
}

function button(re: RegExp): HTMLButtonElement | undefined {
  return Array.from(host!.querySelectorAll("button")).find((b) => re.test(b.textContent ?? ""));
}

async function click(re: RegExp): Promise<void> {
  const b = button(re);
  expect(b, `button ${re}`).toBeTruthy();
  await act(async () => b!.click());
}

/** Resolve every pending grant read (oldest first) and let the attempts run on. */
async function resolveReads(grant: GrantShape = NOT_GRANTED): Promise<void> {
  await act(async () => {
    for (const p of m.pending.splice(0)) p.resolve(grant);
    for (let i = 0; i < 8; i++) await Promise.resolve();
  });
}

function linkedStore(): Store {
  const store = new Store();
  // A linked wallet (claim === custody) at S3, idle: choosing membership auto-opens the attempt.
  store.setState({ stage: "s3", s3: "idle", s3PollExhausted: false, walletAddr: "0xabc", custodyAddr: "0xabc", authEmail: "d@example.com" });
  return store;
}

beforeEach(() => {
  vi.useFakeTimers();
  m.pending.splice(0);
  m.checkout.mockClear();
  m.grantStatus.mockClear();
  m.userinfo.mockClear();
});

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  host?.remove();
  host = null;
  vi.clearAllTimers();
  vi.useRealTimers();
});

describe("S3 checkout: one attempt at a time while the grant check runs (core#584)", () => {
  it("double-clicking Pay during the check opens at most one checkout", async () => {
    const store = linkedStore();
    await mountS3(store);
    await click(/Become a member/); // auto-opens attempt 1 (linked wallet)
    // The check is still running; the member clicks Pay twice.
    const pay = button(/Check out in your browser/);
    if (pay) {
      await act(async () => pay.click());
      await act(async () => pay.click());
    }
    await resolveReads();
    expect(m.checkout).toHaveBeenCalledTimes(1);
    expect(store.state.s3).toBe("paying");
  });

  it("two direct onS3Pay calls during one check start one attempt and one checkout", async () => {
    const store = linkedStore();
    store.onS3Pay();
    store.onS3Pay();
    await resolveReads();
    expect(m.grantStatus).toHaveBeenCalledTimes(1);
    expect(m.checkout).toHaveBeenCalledTimes(1);
  });

  it("Back during the check, then the check resolves: no checkout opens", async () => {
    const store = linkedStore();
    await mountS3(store);
    await click(/Become a member/);
    expect(m.grantStatus).toHaveBeenCalledTimes(1);
    await click(/Back to the free node/);
    await resolveReads();
    expect(m.checkout).not.toHaveBeenCalled();
    expect(store.state.s3).toBe("idle");
    // Still on the free-node choice.
    expect(button(/Run a node for free/)).toBeTruthy();
  });

  it("Back and choosing membership again during the check still makes one attempt", async () => {
    const store = linkedStore();
    await mountS3(store);
    await click(/Become a member/); // attempt 1, read pending
    await click(/Back to the free node/);
    await click(/Become a member/); // attempt 2 (auto-open again), attempt 1's read still pending
    await resolveReads();
    expect(m.checkout).toHaveBeenCalledTimes(1);
    expect(store.state.s3).toBe("paying");
  });

  it("leaving S3 another way during the check opens no checkout", async () => {
    const store = linkedStore();
    store.onS3Pay();
    store.setState({ stage: "s1", s1: "idle" }); // e.g. a claim link sends a signed-out session to sign-in
    await resolveReads();
    expect(m.checkout).not.toHaveBeenCalled();
    expect(store.state.s3).toBe("idle");
  });

  it("'Continue free' and Pay are disabled while the check runs; Back stays live", async () => {
    const store = linkedStore();
    await mountS3(store);
    await click(/Become a member/);
    expect(button(/Continue free/)?.disabled).toBe(true);
    expect(button(/Check out in your browser/)?.disabled).toBe(true);
    // Back stays available: it is how the member leaves the attempt.
    expect(button(/Back to the free node/)?.disabled).toBe(false);
    // A click on the disabled free button does nothing.
    await act(async () => button(/Continue free/)!.click());
    expect(store.state.stage).toBe("s3");
    await resolveReads();
    expect(store.state.s3).toBe("paying");
  });

  it("the free path is live again after Back cancels the check", async () => {
    const store = linkedStore();
    await mountS3(store);
    await click(/Become a member/);
    await click(/Back to the free node/);
    await click(/Run a node for free/);
    await resolveReads();
    expect(store.state.stage).toBe("s4");
    expect(store.state.tier).toBe("free");
    expect(m.checkout).not.toHaveBeenCalled();
  });

  it("unchanged: an unreadable chain still opens checkout once, and the controls come back", async () => {
    const store = linkedStore();
    store.onS3Pay();
    await act(async () => {
      for (const p of m.pending.splice(0)) p.reject(new Error("rpc down"));
      for (let i = 0; i < 8; i++) await Promise.resolve();
    });
    expect(m.checkout).toHaveBeenCalledTimes(1);
    expect(store.state.s3).toBe("paying");
    expect(store.state.s3Checking).toBe(false);
  });

  it("unchanged: a held grant settles S3 without checkout, and the check ends", async () => {
    const store = linkedStore();
    store.onS3Pay();
    store.onS3Pay();
    await resolveReads(GRANTED);
    expect(m.checkout).not.toHaveBeenCalled();
    expect(store.state.s3).toBe("settled");
    expect(store.state.s3Checking).toBe(false);
  });

  it("unchanged: an unlinked wallet goes to the link ceremony, not a check", async () => {
    const store = new Store();
    store.setState({ stage: "s3", s3: "idle", walletAddr: "0xclaim", custodyAddr: "0xdevice" });
    const link = vi.spyOn(store, "linkWallet").mockResolvedValue(undefined as never);
    store.onS3Pay();
    await resolveReads();
    expect(link).toHaveBeenCalledTimes(1);
    expect(m.grantStatus).not.toHaveBeenCalled();
    expect(m.checkout).not.toHaveBeenCalled();
    expect(store.state.s3Checking).toBe(false);
  });
});

// Tripwire for the bug class: a checkout opened after an awaited check without asking whether the
// attempt is still current. (1) onS3Pay keeps its guard order: refuse a second call while a check
// runs, then, after the awaited grant read, drop a superseded attempt BEFORE opening checkout.
// (2) The app opens checkout from exactly two call sites (S3 and Settings › Billing). A new one
// fails here, so whoever adds it checks it for one attempt at a time and updates this count.
describe("tripwire: checkout opens only from a current attempt (core#584)", () => {
  const SRC = join(__dirname, "..");
  const store = readFileSync(join(SRC, "shell", "store.ts"), "utf8");

  it("onS3Pay refuses a second call during the check and drops a stale attempt before checkout", () => {
    const start = store.indexOf("  onS3Pay(): void {");
    expect(start).toBeGreaterThan(-1);
    const body = store.slice(start, store.indexOf("\n  }\n", start));
    const busy = body.indexOf("if (this.state.s3Checking) return;");
    const read = body.indexOf("await this.settleS3IfAlreadyGranted()");
    const stale = body.indexOf("if (attempt !== this.s3Attempt) return;");
    const open = body.indexOf("bridge.membership.checkout(");
    expect(busy).toBeGreaterThan(-1);
    expect(read).toBeGreaterThan(busy);
    expect(stale).toBeGreaterThan(read);
    expect(open).toBeGreaterThan(stale);
  });

  it("checkout is opened from exactly two call sites in app code", () => {
    const sites: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const p = join(dir, name);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name)) {
          readFileSync(p, "utf8")
            .split("\n")
            .forEach((line, i) => {
              if (/bridge\.membership\.checkout\(/.test(line) && !/^\s*(\/\/|\*)/.test(line)) sites.push(`${p}:${i + 1}`);
            });
        }
      }
    };
    walk(SRC);
    expect(sites, sites.join("\n")).toHaveLength(2);
  });
});
