// K3 — the Node page tabs (Status, Services, Rewards, Advanced), ported from design slice 2 under the
// UI truth contract: every screen has its data, empty, error and unavailable state; unknown is "—",
// never 0; no finality, no projections, no "Earnings".
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { freshState, type AppState } from "../../shell/state";
import type { Store } from "../../shell/store";
import { NodeStatusView } from "./NodeStatus";
import { NodeServices } from "./NodeServices";
import { NodeRewards } from "./NodeRewards";
import { NodeAdvancedView } from "./NodeAdvanced";
import type { ServiceReads, ServiceControls } from "./services";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function mount(el: React.ReactElement): Promise<HTMLDivElement> {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(el);
  });
  // Let the mount effects' reads settle.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
  return host;
}

const at = (patch: Partial<AppState> = {}): AppState => ({ ...freshState("p1"), stage: "done", ...patch });

function stubStore(over: Partial<Record<string, unknown>> = {}): Store & { calls: string[] } {
  const calls: string[] = [];
  const s = {
    calls,
    startNode: () => void calls.push("startNode"),
    stopNode: () => void calls.push("stopNode"),
    go: (r: string) => void calls.push("go:" + r),
    toast: (m: string) => void calls.push("toast:" + m),
    walletIsLinked: () => true,
    linkWallet: async () => void calls.push("linkWallet"),
    activateValidator: async () => {
      calls.push("activateValidator");
      return true;
    },
    ...over,
  };
  return s as unknown as Store & { calls: string[] };
}

const click = async (el: Element | null | undefined) => {
  if (!el) throw new Error("no element to click");
  await act(async () => {
    (el as HTMLElement).click();
    await new Promise((r) => setTimeout(r, 0));
  });
};
const button = (root: HTMLElement, label: string | RegExp) =>
  [...root.querySelectorAll("button")].find((b) => (typeof label === "string" ? b.textContent?.trim() === label : label.test(b.textContent ?? "")));

const BANNED = /\bfinal(ized)?\b|validating|earning|projected|estimate|per day|\btoday\b|APY|yield/i;

// ---------------------------------------------------------------------------------------------
describe("Node › Status", () => {
  const live = { publicHead: 510_032, compat: "compatible" as const, mode: "tauri" as const, statusError: null };

  it("one sentence in contract words, with direct peers and the read height", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4 })} {...live} />);
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced");
    expect(h.textContent).toContain("4 direct peers");
    expect(h.textContent).toContain("510,030 · 2 behind the public head");
    expect(h.textContent).not.toMatch(BANNED);
    expect(h.textContent).not.toContain("%");
  });

  it("v1.1 network rows: Registered and Eligible validators, and no consensus-set number", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4 })} {...live} />);
    expect(h.textContent).toContain("Registered validators");
    expect(h.textContent).toContain("Eligible validators");
    expect(h.textContent).toContain("Consensus validator set");
    expect(h.textContent).toContain("unavailable until a snapshot RPC exists");
    expect(h.textContent).not.toContain("Active validators");
  });

  it("the validator word joins the sentence only when it was read; otherwise '—' with the reason", async () => {
    const s = at({ node: "synced", height: 510_030, peers: 4 });
    const unread = await mount(<NodeStatusView store={stubStore()} s={s} {...live} />);
    expect(unread.querySelector('[data-testid="validator-word"]')?.textContent).toMatch(/^—/);
    act(() => root?.unmount());
    const read = await mount(<NodeStatusView store={stubStore()} s={s} {...live} validatorWord="Validator active" />);
    expect(read.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced · Validator active");
  });

  it("empty: an off node offers Start, and its numbers are unknown, not 0", async () => {
    const store = stubStore();
    const h = await mount(<NodeStatusView store={store} s={at({ node: "off", height: 0, peers: 0 })} {...live} />);
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Off");
    expect(button(h, "Stop node")).toBeUndefined();
    expect(h.textContent).toContain("— · not read");
    await click(button(h, "Start node"));
    expect(store.calls).toContain("startNode");
  });

  it("Update required shows its notice", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 10, peers: 1 })} {...live} compat="retired" />);
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Update required");
    expect(h.querySelector('[data-testid="node-notice"]')?.textContent).toMatch(/genesis/i);
  });

  it("error: a failed status read makes every number unknown and says which call failed", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4 })} {...live} statusError="supervisor not answering" />);
    expect(h.querySelector('[data-state="error"]')?.textContent).toContain("node_status");
    expect(h.textContent).toContain("supervisor not answering");
    expect(h.querySelector('[data-testid="node-sentence"]')).toBeNull();
  });

  it("unavailable: the web preview has no supervisor, so nothing is shown rather than sim rows", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 131_204, peers: 23 })} {...live} mode="sim" />);
    expect(h.querySelector('[data-state="unavailable"]')).not.toBeNull();
    expect(h.textContent).not.toContain("131,204");
    expect(h.textContent).not.toContain("23 direct peers");
  });

  it("activation is offered on a synced node, without an amount or earning copy", async () => {
    const store = stubStore();
    const h = await mount(<NodeStatusView store={store} s={at({ node: "synced", height: 510_030, peers: 4 })} {...live} />);
    const b = button(h, /Activate validator/);
    expect(b?.textContent).not.toMatch(/32,000|SALT/);
    await click(b);
    expect(store.calls).toContain("activateValidator");
  });
});

// ---------------------------------------------------------------------------------------------
function svcReads(over: Partial<ServiceReads> = {}): ServiceReads {
  return {
    node: async () => ({ state: "running" }),
    agent: async () => ({ state: "running" }),
    memory: async () => ({ state: "running" }),
    llama: async () => ({ state: "running" }),
    ipfs: async () => true,
    hermes: async () => ({ state: "running" }),
    commsRelay: async () => "connected",
    ...over,
  };
}
function svcControls(calls: string[]): ServiceControls {
  const c = (id: string) => ({ stop: async () => void calls.push(id + ".stop"), start: async () => void calls.push(id + ".start") });
  return { node: c("node"), agent: c("agent"), memory: c("memory"), llama: c("llama"), ipfs: c("ipfs"), hermes: c("hermes") };
}

describe("Node › Services", () => {
  it("a table of the nine services: phase from the supervisor, readiness '—' until a DTO carries it", async () => {
    const h = await mount(<NodeServices store={stubStore()} s={at({ node: "synced" })} mode="tauri" reads={svcReads()} controls={svcControls([])} />);
    const rows = [...h.querySelectorAll("[data-service]")];
    expect(rows.map((r) => r.getAttribute("data-service"))).toEqual(["node", "agent", "memory", "llama", "embed", "ipfs", "hermes", "comms", "cluster"]);
    const nodeRow = h.querySelector('[data-service="node"]');
    expect(nodeRow?.querySelector("[data-phase]")?.textContent).toBe("Alive");
    expect(nodeRow?.querySelector("[data-readiness]")?.textContent).toBe("—");
    expect(h.querySelector('[data-service="embed"] [data-phase]')?.textContent).toBe("—");
    expect(h.textContent).toContain("6 alive");
  });

  it("empty: nothing started yet", async () => {
    const stopped = async () => ({ state: "stopped" });
    const h = await mount(
      <NodeServices
        store={stubStore()}
        s={at({ node: "off" })}
        mode="tauri"
        reads={svcReads({ node: stopped, agent: stopped, memory: stopped, llama: stopped, hermes: stopped, ipfs: async () => false })}
        controls={svcControls([])}
      />,
    );
    expect(h.textContent).toContain("none started yet");
  });

  it("error: every read failed, so states are unknown and Try again re-reads", async () => {
    const fail = async () => Promise.reject("supervisor not answering");
    const ok = vi.fn(async () => ({ state: "running" }));
    let first = true;
    const node = async () => {
      if (first) {
        first = false;
        return Promise.reject("supervisor not answering");
      }
      return ok();
    };
    const h = await mount(
      <NodeServices
        store={stubStore()}
        s={at()}
        mode="tauri"
        reads={svcReads({ node, agent: fail, memory: fail, llama: fail, hermes: fail, ipfs: fail, commsRelay: fail })}
        controls={svcControls([])}
      />,
    );
    expect(h.querySelector('[data-state="error"]')?.textContent).toContain("supervisor not answering");
    await click(button(h, "Try again"));
    expect(ok).toHaveBeenCalled();
  });

  it("unavailable in the web preview", async () => {
    const h = await mount(<NodeServices store={stubStore()} s={at()} mode="sim" reads={svcReads()} controls={svcControls([])} />);
    expect(h.querySelector('[data-state="unavailable"]')).not.toBeNull();
    expect(h.querySelector("[data-service]")).toBeNull();
  });

  it("bounded log: the node's own log lines; other services say no log read exists yet", async () => {
    const logs = Array.from({ length: 700 }, (_, i) => ({ t: "00:00:0" + (i % 10), line: "line " + i, id: i }));
    const h = await mount(<NodeServices store={stubStore()} s={at({ node: "synced", logs })} mode="tauri" reads={svcReads()} controls={svcControls([])} />);
    await click(h.querySelector('[data-service="node"] button[data-action="log"]'));
    const log = h.querySelector('[data-testid="service-log"]');
    expect(log?.textContent).toContain("line 699");
    expect(log?.textContent).not.toContain("line 199 ");
    expect(log?.querySelectorAll("[data-log-line]").length).toBe(500);
    await click(h.querySelector('[data-service="memory"] button[data-action="log"]'));
    expect(h.querySelector('[data-testid="service-log"]')?.textContent).toMatch(/no log read/i);
  });

  it("restart runs the service's own stop and start commands", async () => {
    const calls: string[] = [];
    let hermesState = "running";
    const reads = svcReads({ hermes: async () => ({ state: hermesState }) });
    const controls = svcControls(calls);
    controls.hermes.stop = async () => {
      calls.push("hermes.stop");
      hermesState = "stopped";
    };
    const h = await mount(<NodeServices store={stubStore()} s={at({ node: "synced" })} mode="tauri" reads={reads} controls={controls} />);
    await click(h.querySelector('[data-service="hermes"] button[data-action="restart"]'));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });
    expect(calls).toEqual(["hermes.stop", "hermes.start"]);
  });

  it("no restart for a service with no command, and the button says why", async () => {
    const h = await mount(<NodeServices store={stubStore()} s={at()} mode="tauri" reads={svcReads()} controls={svcControls([])} />);
    const b = h.querySelector('[data-service="cluster"] button[data-action="restart"]') as HTMLButtonElement;
    expect(b.disabled).toBe(true);
    expect(b.title).toMatch(/no .*command/i);
  });
});

// ---------------------------------------------------------------------------------------------
const WEI = (salt: string) => (BigInt(Math.round(Number(salt) * 100)) * 10n ** 16n).toString();

describe("Node › Rewards", () => {
  it("rewardsOf total, claimable now and maturing, from the registry read", async () => {
    const read = vi.fn(async () => ({ totalWei: WEI("12.5"), claimableWei: WEI("2.5"), proposerPubkey: "0x" + "ab".repeat(32) }));
    const h = await mount(<NodeRewards mode="tauri" read={read} />);
    expect(h.querySelector('[data-reward="total"]')?.textContent).toBe("12.50 SALT");
    expect(h.querySelector('[data-reward="claimable"]')?.textContent).toBe("2.50 SALT");
    expect(h.querySelector('[data-reward="maturing"]')?.textContent).toBe("10.00 SALT");
    expect(h.textContent).toContain("rewardsOf");
    // The panel carries no money words; the page's one disclaimer names what is never shown.
    expect(h.querySelector('[data-testid="validator-rewards"]')?.textContent).not.toMatch(BANNED);
    expect(h.textContent).not.toContain("Earnings");
    expect(h.textContent).not.toMatch(/Claim\b/);
  });

  it("pin and round rewards are not live", async () => {
    const h = await mount(<NodeRewards mode="tauri" read={async () => ({ totalWei: "0", claimableWei: "0", proposerPubkey: "0x" + "ab".repeat(32) })} />);
    expect(h.textContent).toContain("Pin bonds not live yet");
    expect(h.textContent).toContain("Group rounds not live yet");
  });

  it("Not a validator is a word, not 0", async () => {
    const h = await mount(<NodeRewards mode="tauri" validatorWord="Not a validator" read={async () => ({ totalWei: "0", claimableWei: "0", proposerPubkey: "0x" + "ab".repeat(32) })} />);
    expect(h.querySelector('[data-reward="total"]')?.textContent).toBe("Not a validator");
    expect(h.querySelector('[data-reward="claimable"]')).toBeNull();
  });

  it("empty: a zero read says zero is not proof of registration", async () => {
    const h = await mount(<NodeRewards mode="tauri" read={async () => ({ totalWei: "0", claimableWei: "0", proposerPubkey: "0x" + "ab".repeat(32) })} />);
    expect(h.querySelector('[data-reward="total"]')?.textContent).toBe("0.00 SALT");
    expect(h.textContent).toMatch(/also reads 0 for a key that isn't registered/);
  });

  it("unavailable without a proposer key, error with '—' otherwise", async () => {
    const noKey = await mount(<NodeRewards mode="tauri" read={async () => Promise.reject("proposer.key not found: arm the producer first")} />);
    expect(noKey.querySelector('[data-state="unavailable"]')?.textContent).toContain("this node's key");
    act(() => root?.unmount());
    const err = await mount(<NodeRewards mode="tauri" read={async () => Promise.reject("eth_call timed out")} />);
    expect(err.querySelector('[data-state="error"]')?.textContent).toContain("eth_call timed out");
    expect(err.textContent).toContain("unknown, not zero");
    expect(err.querySelector('[data-reward="total"]')).toBeNull();
  });

  it("unavailable in the web preview", async () => {
    const read = vi.fn();
    const h = await mount(<NodeRewards mode="sim" read={read} />);
    expect(h.querySelector('[data-state="unavailable"]')).not.toBeNull();
    expect(read).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------------------------
describe("Node › Advanced", () => {
  const reads = (over: Partial<{ notice: string | undefined; crash: { reason: string; atUnixMs: number } | null; fail: boolean }> = {}) => ({
    nodeStatus: async () => {
      if (over.fail) return Promise.reject("supervisor not answering");
      return { state: "running", peers: 3, height: 10, syncPct: 100, notice: over.notice };
    },
    lastCrash: async () => (over.fail ? Promise.reject("supervisor not answering") : (over.crash ?? null)),
  });
  const GEN = "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1";

  it("network identity: matches this build, or Update required, or not checked yet", async () => {
    const ok = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads()} />);
    expect(ok.querySelector('[data-testid="genesis-word"]')?.textContent).toBe("matches this build");
    expect(ok.textContent).toContain("Update required at genesis");
    act(() => root?.unmount());
    const bad = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="retired" liveGenesis={"0x" + "11".repeat(32)} reads={reads()} />);
    expect(bad.querySelector('[data-testid="genesis-word"]')?.textContent).toBe("Update required");
    act(() => root?.unmount());
    const unk = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="unknown" liveGenesis={null} reads={reads()} />);
    expect(unk.querySelector('[data-testid="genesis-word"]')?.textContent).toBe("not checked yet · —");
  });

  it("the withdrawn-branch notice from node_status, and an honest line when there is none", async () => {
    const h = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads({ notice: "This node had followed a branch of the chain that the network withdrew." })} />);
    expect(h.querySelector('[data-testid="withdrawn"]')?.textContent).toContain("followed a branch");
    act(() => root?.unmount());
    const none = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads()} />);
    expect(none.querySelector('[data-testid="withdrawn"]')?.textContent).toMatch(/no notice/i);
    expect(none.querySelector('[data-testid="withdrawn"]')?.textContent).not.toMatch(/Resync required/);
  });

  it("crash records: the last record, or none", async () => {
    const h = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads({ crash: { reason: "exit 137", atUnixMs: Date.UTC(2026, 9, 16, 3, 12) } })} />);
    expect(h.querySelector('[data-testid="crashes"]')?.textContent).toContain("exit 137");
    act(() => root?.unmount());
    const none = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads()} />);
    expect(none.querySelector('[data-testid="crashes"]')?.textContent).toContain("No crashes recorded");
  });

  it("full log: the node's lines, or why there are none", async () => {
    const logs = [{ t: "12:00:01", line: "imported block 510030", id: 1 }];
    const h = await mount(<NodeAdvancedView s={at({ node: "synced", logs })} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads()} />);
    expect(h.querySelector('[data-testid="full-log"]')?.textContent).toContain("imported block 510030");
    act(() => root?.unmount());
    const off = await mount(<NodeAdvancedView s={at({ node: "off", logs: [] })} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads()} />);
    expect(off.querySelector('[data-testid="full-log"]')?.textContent).toMatch(/node is off/i);
  });

  it("error: a failed read shows the call and keeps the rest of the page", async () => {
    const h = await mount(<NodeAdvancedView s={at()} mode="tauri" compat="compatible" liveGenesis={GEN} reads={reads({ fail: true })} />);
    expect(h.querySelector('[data-testid="withdrawn"] [data-state="error"]')?.textContent).toContain("supervisor not answering");
    expect(h.querySelector('[data-testid="crashes"] [data-state="error"]')?.textContent).toContain("node_last_crash");
  });

  it("unavailable in the web preview", async () => {
    const h = await mount(<NodeAdvancedView s={at()} mode="sim" compat="unknown" liveGenesis={null} reads={reads()} />);
    expect(h.querySelector('[data-state="unavailable"]')).not.toBeNull();
  });
});
