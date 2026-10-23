// Behaviour tests for the 0.5.2 design prototype (design/CitrateCore.dc.html).
//
// The prototype is a single-file DC page: its logic is one `class Component extends DCLogic` in the
// `<script data-dc-script>` block. These tests load that class the way the DC runtime does
// (support.js `evalDcLogic`), with a stand-in base class whose setState applies the patch at once,
// like the runtime's `__setLogicState`. Nothing is rendered; `renderVals()` gives the view values
// the template binds to. The chat replies come from the prototype's own demo provider
// (design/chat-harness.js).
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const DESIGN = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../design") + path.sep;

class FakeLogic {
  constructor(props) {
    this.props = props || {};
    this.state = {};
  }
  setState(update, cb) {
    const patch = typeof update === "function" ? update(this.state) : update;
    this.state = { ...this.state, ...patch };
    if (cb) cb();
  }
  forceUpdate() {}
}

function loadComponent() {
  const html = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8");
  const m = /<script type="text\/x-dc" data-dc-script>([\s\S]*?)<\/script>/.exec(html);
  if (!m) throw new Error("no data-dc-script block in CitrateCore.dc.html");
  // Same shape as support.js evalDcLogic. React is only used to draw the canvas, which needs data.
  return new Function("DCLogic", "StreamableLogic", "React", m[1] + "\n;return Component;")(FakeLogic, FakeLogic, { createElement: () => null });
}

const Component = loadComponent();
// The prototype's own save key (it is bumped when the saved shape changes).
const SAVE_KEY = /localStorage\.getItem\('([^']+)'/.exec(readFileSync(DESIGN + "CitrateCore.dc.html", "utf8"))[1];
let harness;
let coreData;

/** A prototype instance for a persona, as after a fresh install of that persona. */
async function mount(persona, opts = {}) {
  if (!opts.keepStorage) localStorage.removeItem(SAVE_KEY);
  const c = new Component({});
  if (persona) c.state = c.freshState(persona);
  c.data = coreData;
  const real = harness.createDemoProvider(() => c.snapshot());
  c.sent = [];
  c.provider = {
    send: (args) => {
      c.sent.push(args.messages.map((x) => ({ ...x })));
      return real.send(args);
    },
  };
  return c;
}

const lastHermes = (c) => [...c.state.chatMsgs].reverse().find((x) => x.who === "Hermes");
const userTurns = (msgs, text) => msgs.filter((x) => x.role === "user" && x.content === text).length;
const settle = () => vi.runAllTimersAsync();

beforeEach(async () => {
  vi.useFakeTimers();
  vi.stubGlobal("requestAnimationFrame", (f) => setTimeout(f, 0));
  harness = await import(DESIGN + "chat-harness.js");
  coreData = await import(DESIGN + "core-data.js");
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  localStorage.removeItem(SAVE_KEY);
});

describe("ask before gateway (HIC-1): one pending turn at a time", () => {
  it("refuses a new message while a gateway decision is pending, and the composer says so", async () => {
    const c = await mount("p3"); // P3: routing = ask
    c.sendChat("Plan my week");
    await settle();
    expect(c.renderVals().askText).toBe("Plan my week");
    c.sendChat("Network status");
    await settle();
    expect(c.sent).toHaveLength(0);
    expect(c.state.chatMsgs.filter((x) => x.who === "You").map((x) => x.text)).toEqual(["Plan my week"]);
    const V = c.renderVals();
    expect(V.chatBusy).toBe(true);
    expect(V.askBlockNote).toMatch(/gateway/i);
  });

  it("does not let a second gateway request replace the first", async () => {
    const c = await mount("p3");
    c.sendChat("Plan my week");
    c.sendChat("Draft a release note");
    await settle();
    expect(c.renderVals().askText).toBe("Plan my week");
    expect(c.state.chatMsgs.filter((x) => x.who === "You")).toHaveLength(1);
  });

  it("approving sends exactly the approved turn, once, and the reply answers it", async () => {
    const c = await mount("p3");
    c.sendChat("Plan my week");
    c.sendChat("Network status"); // refused while pending
    c.resolveAsk(true);
    c.resolveAsk(true); // a double click sends nothing more
    await settle();
    expect(c.sent).toHaveLength(1);
    const msgs = c.sent[0];
    expect(msgs[msgs.length - 1]).toEqual({ role: "user", content: "Plan my week" });
    expect(userTurns(msgs, "Plan my week")).toBe(1);
    expect(lastHermes(c).text).toMatch(/infer\.citrate\.ai/);
    expect(lastHermes(c).text).toMatch(/From your journal this week/);
    expect(c.state.records[0]).toMatchObject({ decision: "approved", reason: "chat · ask before gateway" });
    expect(c.renderVals().chatBusy).toBe(false);
  });

  it("keeping it local answers the same turn on the local model and records a denial", async () => {
    const c = await mount("p3");
    c.sendChat("Rewrite my intro");
    c.resolveAsk(false);
    await settle();
    const msgs = c.sent[0];
    expect(msgs[msgs.length - 1]).toEqual({ role: "user", content: "Rewrite my intro" });
    expect(lastHermes(c).text).toMatch(/local model on this machine/);
    expect(c.state.records[0]).toMatchObject({ decision: "denied" });
  });

  it("the approval record names where the request was made", async () => {
    const c = await mount("p3");
    c.sendChat("Plan my week", { origin: "journal" });
    c.resolveAsk(false);
    await settle();
    expect(c.state.records[0].reason).toBe("journal · ask before gateway");
  });

  it("a refused Journal message stays in the Journal input", async () => {
    const c = await mount("p3");
    c.sendChat("Plan my week", { origin: "journal" });
    const input = { value: "Draft a thank-you note" };
    c.jChatInputEl = input;
    c.renderVals().onJChatSend();
    await settle();
    expect(input.value).toBe("Draft a thank-you note");
  });

  it("an ordinary message is sent to the model once", async () => {
    const c = await mount("p2"); // P2: local only
    c.sendChat("Network status");
    await settle();
    expect(c.sent).toHaveLength(1);
    expect(userTurns(c.sent[0], "Network status")).toBe(1);
  });
});

describe("daemon runs across a reload", () => {
  async function startWebDaemon(c) {
    c.setState({ webSearch: true });
    c.restartHermes();
    await vi.advanceTimersByTimeAsync(1000);
    c.daemonStart("d-notes");
    expect(c.state.daemons.find((d) => d.id === "d-notes").state).toBe("running");
  }

  it("a run cut off by a reload comes back paused and logged as stopped, not running", async () => {
    const c = await mount("p2");
    await startWebDaemon(c);
    await vi.advanceTimersByTimeAsync(500); // the save lands; the run has not finished
    c.componentWillUnmount();
    vi.clearAllTimers();

    const r = await mount(null, { keepStorage: true });
    const d = r.state.daemons.find((x) => x.id === "d-notes");
    expect(d.state).toBe("paused");
    expect(d.paused).toBe(true);
    expect(d.note).toMatch(/closed during a run/i);
    expect(r.state.runLog[0]).toMatchObject({ daemonId: "d-notes", outcome: "stopped" });
    expect(r.state.approvals.some((a) => a.daemonId === "d-notes")).toBe(false);
  });

  it("after the reload, Start runs it to its approval and approving writes the journal", async () => {
    const c = await mount("p2");
    await startWebDaemon(c);
    await vi.advanceTimersByTimeAsync(500);
    vi.clearAllTimers();

    const r = await mount(null, { keepStorage: true });
    r.daemonStart("d-notes");
    await vi.advanceTimersByTimeAsync(3000);
    const a = r.state.approvals.find((x) => x.daemonId === "d-notes");
    expect(a).toBeTruthy();
    r.resolveApproval(a.id, true);
    const today = new Date().toISOString().slice(0, 10);
    expect(r.state.jPages.find((p) => p.id === "d-" + today).blocks.join("\n")).toMatch(/Release-notes watcher/);
  });

  it("a daemon awaiting review keeps its state and its approval across a reload", async () => {
    const c = await mount("p2");
    await startWebDaemon(c);
    await vi.advanceTimersByTimeAsync(3000);
    expect(c.state.daemons.find((d) => d.id === "d-notes").state).toBe("awaiting");
    await vi.advanceTimersByTimeAsync(500);
    vi.clearAllTimers();

    const r = await mount(null, { keepStorage: true });
    expect(r.state.daemons.find((d) => d.id === "d-notes").state).toBe("awaiting");
    expect(r.state.approvals.some((a) => a.daemonId === "d-notes")).toBe(true);
    expect(r.state.runLog.filter((x) => x.outcome === "stopped")).toHaveLength(0);
  });
});

describe("click path 2 (web search -> Store -> install paused -> start -> approve)", () => {
  it("runs end to end and leaves the page in Journal and the decision in Records", async () => {
    const c = await mount("p2");
    const web = () => c.renderVals().ctlRows.find((r) => r.key === "web");
    web().toggle();
    c.restartHermes();
    await settle();
    c.stubInstall();
    c.daemonStart("d-brief");
    await vi.advanceTimersByTimeAsync(3000);
    const a = c.state.approvals.find((x) => x.daemonId === "d-brief");
    c.resolveApproval(a.id, true);
    const today = new Date().toISOString().slice(0, 10);
    expect(c.state.jPages.some((p) => p.title === "Web brief · " + today)).toBe(true);
    expect(c.state.records[0]).toMatchObject({ decision: "approved" });
    expect(c.state.cp2).toMatchObject({ web: true, installed: true, started: true, approved: true });
  });
});

describe("Controls depth 1 follows the shipped panel (core #341): saved is not effective", () => {
  it("web search and learning say the running Hermes keeps its old value until it restarts", async () => {
    const c = await mount("p2");
    const row = (k) => c.renderVals().ctlRows.find((r) => r.key === k);
    expect(row("web").pending).toBeFalsy();
    row("web").toggle();
    expect(row("web").pending).toBe("Until then the running Hermes does not have web search.");
    row("learn").toggle();
    expect(row("learn").pending).toBe("Until then the running Hermes does not record sessions.");
    c.restartHermes();
    await settle();
    expect(row("web").pending).toBeFalsy();
    expect(row("learn").pending).toBeFalsy();
  });

  it("a daemon that needs web search does not start until the running Hermes has it", async () => {
    const c = await mount("p2");
    c.renderVals().ctlRows.find((r) => r.key === "web").toggle();
    c.daemonStart("d-notes");
    const d = c.state.daemons.find((x) => x.id === "d-notes");
    expect(d.state).toBe("paused");
    expect(d.note).toMatch(/restart/i);
    c.restartHermes();
    await settle();
    c.daemonStart("d-notes");
    expect(c.state.daemons.find((x) => x.id === "d-notes").state).toBe("running");
  });
});

describe("deep links follow the K0 route map (src/shell/places.ts, core #343)", () => {
  const cases = [
    ["#/dashboard", { route: "hermes", hTab: "chat" }],
    ["#/agent", { route: "hermes", hTab: "work" }],
    ["#/journal", { route: "hermes", hTab: "journal" }],
    ["#/comms", { route: "groups" }],
    ["#/people", { route: "groups" }],
    ["#/train", { route: "groups" }],
    ["#/alf", { route: "groups" }],
    ["#/community", { route: "groups" }],
    ["#/files", { route: "node" }],
    ["#/storage", { route: "node" }],
    ["#/node/storage/files", { route: "node" }],
    ["#/node/earnings", { route: "node" }],
    ["#/commissary", { route: "store" }],
    ["#/models", { route: "store" }],
    ["#/connections", { route: "settings", sSec: "connections" }],
    ["#/settings/hermes", { route: "hermes", hTab: "controls", cDepth: 1 }],
    ["#/settings/ai", { route: "hermes", hTab: "controls", cDepth: 2, cSec: "providers" }],
    ["#/settings/escalation", { route: "hermes", hTab: "controls", cDepth: 2, cSec: "escalation" }],
    ["#/settings/mcp", { route: "hermes", hTab: "controls", cDepth: 2, cSec: "mcp" }],
    ["#/settings/web", { route: "hermes", hTab: "controls", cDepth: 2, cSec: "web" }],
    ["#/settings/budgets", { route: "hermes", hTab: "controls", cDepth: 2, cSec: "budgets" }],
    ["#/settings/node", { route: "node" }],
    ["#/settings/billing", { route: "settings", sSec: "billing" }],
    ["#/settings/privacy", { route: "settings", sSec: "privacy" }],
    ["#/hermes/controls/details", { route: "hermes", hTab: "controls", cDepth: 2 }],
    ["#/hermes/controls/records", { route: "hermes", hTab: "controls", cDepth: 3, cAud: "records" }],
    ["#/hermes/controls/web", { route: "hermes", hTab: "controls", cDepth: 2, cSec: "web" }],
  ];
  it.each(cases)("%s opens its K0 home", async (hash, want) => {
    const c = await mount("p2");
    expect(c.resolveHash(hash)).toMatchObject(want);
  });

  it("an unknown route opens nothing", async () => {
    const c = await mount("p2");
    expect(c.resolveHash("#/nowhere")).toBeNull();
    expect(c.resolveHash("#/settings/nowhere")).toBeNull();
  });

  it("Settings lists the shipped account-level sections, including Privacy & recovery", async () => {
    const c = await mount("p2");
    expect(c.renderVals().setNav.map((x) => x.label)).toEqual(["Account & RBAC", "Connections", "API endpoints & keys", "Keys & security", "Memberships & billing", "Privacy & recovery", "App"]);
  });
});

describe("tripwires", () => {
  const src = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8");

  it("state that only lives while the page runs is never saved", async () => {
    // A saved transient state comes back on reload without the timer or turn that ends it.
    const c = await mount("p2");
    c.setState({ askPending: { text: "x", origin: "chat", messages: [] }, chatStatus: "thinking", hermesRestarting: true, teachBusy: true, kiState: "running", toast: "t" });
    c.save();
    await vi.advanceTimersByTimeAsync(500);
    const saved = JSON.parse(localStorage.getItem(SAVE_KEY));
    for (const k of ["askPending", "chatStatus", "hermesRestarting", "hermesRun", "teachBusy", "kiState", "toast", "queue"]) expect(saved).not.toHaveProperty(k);
  });

  it("only sendChat and resolveAsk start a model turn, so nothing bypasses the one-pending-turn rule", () => {
    const calls = src.match(/this\.runTurn\(/g) || [];
    expect(calls).toHaveLength(2);
    const body = (name) => src.slice(src.indexOf("  " + name + "("), src.indexOf("\n  }\n", src.indexOf("  " + name + "(")));
    expect(body("sendChat")).toContain("this.runTurn(");
    expect(body("resolveAsk")).toContain("this.runTurn(");
  });

  it("every Journal caller of sendChat passes its origin and acts only on a message that was taken", () => {
    const lines = src.split("\n").filter((l) => /this\.sendChat\(/.test(l) && /^\s+V\.onJ|run: \(\)/.test(l));
    expect(lines.length).toBeGreaterThanOrEqual(3);
    for (const l of lines) {
      expect(l).toMatch(/origin: 'journal'/);
      expect(l).toMatch(/if \(this\.sendChat\(/);
    }
  });
});

describe("slice 2: Node › Services, node agent", () => {
  const agentRow = (c) => c.renderVals().svRows.find((r) => r.name === "Node agent");

  it("stopping the node agent shows it Stopped in the row and the detail", async () => {
    const c = await mount("p2"); // P2: node running
    c.setState({ route: "node", nPage: "services", svcSel: "agent" });
    expect(agentRow(c).phase).toBe("Alive");
    c.svcStop("agent");
    await vi.advanceTimersByTimeAsync(1100);
    expect(c.state.svc.agent.log.at(-1).line).toMatch(/stop complete/);
    expect(agentRow(c).phase).toBe("Stopped");
    expect(c.renderVals().svdPhase).toBe("Stopped");
  });

  it("a stopped agent starts again while the node is Alive", async () => {
    const c = await mount("p2");
    c.setState({ route: "node", nPage: "services", svcSel: "agent" });
    c.svcStop("agent");
    await vi.advanceTimersByTimeAsync(1100);
    c.svcRestart("agent");
    await vi.advanceTimersByTimeAsync(3000);
    expect(agentRow(c).phase).toBe("Alive");
  });

  it("starting the agent while the node is off is refused: it stays Stopped and nothing claims it is alive", async () => {
    const c = await mount("p3"); // P3: node off, agent stopped
    c.setState({ route: "node", nPage: "services", svcSel: "agent" });
    c.svcRestart("agent");
    expect(c.state.toast ?? "").toMatch(/node/i);
    await vi.advanceTimersByTimeAsync(3000);
    expect(agentRow(c).phase).toBe("Stopped");
    expect(c.state.svc.agent.phase).toBe("stopped");
    expect(c.state.svc.agent.log.map((l) => l.line).join("\n")).not.toMatch(/process alive/);
  });

  it("tripwire: with the node Alive, every service's row shows its own phase", async () => {
    const c = await mount("p2");
    c.setState({ route: "node", nPage: "services" });
    const names = Component.SERVICES().filter((d) => d.id !== "node");
    for (const d of names) {
      c.setState((st) => ({ svc: { ...st.svc, [d.id]: { ...st.svc[d.id], phase: "stopped" } } }));
      expect(c.renderVals().svRows.find((r) => r.name === d.name).phase, d.id).toBe("Stopped");
      c.setState((st) => ({ svc: { ...st.svc, [d.id]: { ...st.svc[d.id], phase: "alive" } } }));
      expect(c.renderVals().svRows.find((r) => r.name === d.name).phase, d.id).toBe("Alive");
    }
  });
});

describe("slice 2: Store daemons do what their listing says", () => {
  async function installStartFinish(c, id) {
    c.setState({ webSearch: true });
    c.restartHermes();
    await vi.advanceTimersByTimeAsync(1000);
    c.storeInstall(id);
    c.daemonStart(id);
    expect(c.state.daemons.find((d) => d.id === id).state).toBe("running");
    await vi.advanceTimersByTimeAsync(3000);
  }

  it("Group thread digest proposes its journal summary, and approving appends it to today's page", async () => {
    const c = await mount("p2");
    await installStartFinish(c, "d-triage");
    const a = c.state.approvals.find((x) => x.daemonId === "d-triage");
    expect(a).toBeTruthy();
    expect(c.state.daemons.find((d) => d.id === "d-triage").state).toBe("awaiting");
    c.resolveApproval(a.id, true);
    const today = new Date().toISOString().slice(0, 10);
    expect(c.state.jPages.find((p) => p.id === "d-" + today).blocks.join("\n")).toMatch(/Group thread digest/);
  });

  it("tripwire: every Store daemon whose listing says it writes after approval proposes a change", async () => {
    const writers = Component.LISTINGS().filter((l) => l.kind === "daemons" && l.perms.some(([t]) => /only after you approve/i.test(t)));
    expect(writers.length).toBeGreaterThanOrEqual(2);
    for (const l of writers) {
      const c = await mount("p2");
      await installStartFinish(c, l.id);
      expect(c.state.approvals.some((x) => x.daemonId === l.id), l.id).toBe(true);
    }
  });
});

describe("slice 2: deep links reach the Node, Store and web pages", () => {
  const cases = [
    ["#/settings/node", { route: "node", nPage: "advanced" }],
    ["#/storage", { route: "node", nPage: "storage", nStore: "memory" }],
    ["#/files", { route: "node", nPage: "storage", nStore: "files" }],
    ["#/node/storage/pins", { route: "node", nPage: "storage", nStore: "pins" }],
    ["#/node/earnings", { route: "node", nPage: "rewards" }],
    ["#/commissary", { route: "store", stPage: "catalog", stKind: "apps" }],
    ["#/store/models", { route: "store", stPage: "catalog", stKind: "models" }],
    ["#/store/listing/d-brief", { route: "store", stPage: "listing", stSel: "d-brief" }],
    ["#/web/download", { web: "download" }],
    ["#/web/join/ABC123", { web: "join" }],
  ];
  it.each(cases)("%s", async (hash, want) => {
    const c = await mount("p2");
    expect(c.resolveHash(hash)).toMatchObject(want);
  });
});

describe("slice 2: onboarding Verify step follows the real gate (Join 3 copy review V-03)", () => {
  const verifyStep = async () => {
    const c = await mount("p1"); // fresh install: the 0.5.2 onboarding
    c.setState({ stage: "j", jStage: "verify", jSub: "idle", web: null });
    return c;
  };

  it("says verification is optional for membership and names what it unlocks", async () => {
    const c = await verifyStep();
    const V = c.renderVals();
    expect(V.ob2Lead).toMatch(/^Verification is optional for membership\./);
    expect(V.ob2Lead).toMatch(/withdrawing your bond after the lock, once it is activated/);
    expect(V.ob2Lead).toMatch(/documents never touch this machine/);
  });

  it("offers Skip for now, which moves on to Membership without verifying", async () => {
    const c = await verifyStep();
    const V = c.renderVals();
    expect(V.ob2Secondary).toBe("Skip for now");
    V.onOb2Secondary();
    expect(c.state.jStage).toBe("membership");
    expect(c.state.kycOutcome).toBe("skipped");
  });

  it("tripwire: no copy says membership needs a verified identity", () => {
    const html = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8");
    expect(html).not.toMatch(/[Mm]embership needs a verified identity/);
  });
});

// ===================== slice 3: Wallet, Groups, Settings =====================

describe("slice 3: deep links reach the Groups tabs (K0 places.ts) and the Wallet tabs", () => {
  const cases = [
    ["#/groups", { route: "groups", gTab: "chat" }],
    ["#/groups/chat", { route: "groups", gTab: "chat" }],
    ["#/groups/chat/alerts", { route: "groups", gTab: "alerts" }],
    ["#/groups/rounds", { route: "groups", gTab: "rounds" }],
    ["#/groups/members", { route: "groups", gTab: "members" }],
    ["#/groups/cluster", { route: "groups", gTab: "cluster" }],
    ["#/groups/alf", { route: "groups", gTab: "alf" }],
    ["#/comms", { route: "groups", gTab: "alerts" }],
    ["#/people", { route: "groups", gTab: "members" }],
    ["#/cluster", { route: "groups", gTab: "cluster" }],
    ["#/train", { route: "groups", gTab: "rounds" }],
    ["#/alf", { route: "groups", gTab: "alf" }],
    ["#/community", { route: "groups", gTab: "community" }],
    ["#/wallet", { route: "wallet", wTab: "overview" }],
    ["#/wallet/staking", { route: "wallet", wTab: "staking" }],
    ["#/wallet/activity", { route: "wallet", wTab: "activity" }],
    ["#/settings/connections", { route: "settings", sSec: "connections" }],
  ];
  it.each(cases)("%s", async (hash, want) => {
    const c = await mount("p2");
    expect(c.resolveHash(hash)).toMatchObject(want);
  });
});

describe("slice 3: the validator word (contract §1, core chainTruth.ts)", () => {
  const W = (rec, epoch = 510) => Component.validatorWord(rec, epoch);

  it("Validator active needs isActive and actEpoch ≤ currentEpoch", () => {
    expect(W({ status: 1, isActive: true, actEpoch: 509 })).toBe("Validator active");
    expect(W({ status: 1, isActive: true, actEpoch: 510 })).toBe("Validator active");
  });

  it("isActive alone is not active: a future actEpoch reads Registered · active from epoch N", () => {
    expect(W({ status: 1, isActive: true, actEpoch: 512 })).toBe("Registered · active from epoch 512");
    expect(W({ status: 1, isActive: false, actEpoch: 1200 })).toBe("Registered · active from epoch 1,200");
  });

  it("an unread actEpoch or currentEpoch never reads as active", () => {
    expect(W({ status: 1, isActive: true, actEpoch: null })).toBe("Registered · active from epoch —");
    expect(W({ status: 1, isActive: true, actEpoch: 500 }, null)).not.toBe("Validator active");
  });

  it("reads that disagree say nothing rather than guess", () => {
    expect(W({ status: 1, isActive: false, actEpoch: 500 })).toBeNull();
  });

  it("Exiting and Slashed come from the registry status, before isActive", () => {
    expect(W({ status: 2, isActive: true, actEpoch: 1 })).toBe("Exiting");
    expect(W({ status: 3, isActive: true, actEpoch: 1 })).toBe("Slashed");
  });

  it("a granted member whose bond isn't activated is Bond locked · not staked; an unread bond is unknown", () => {
    expect(W({ status: 0, hasGrant: true, bondActivated: false })).toBe("Bond locked · not staked");
    expect(W({ status: 0, hasGrant: true, bondActivated: null })).toBeNull();
    expect(W({ status: 0, hasGrant: false })).toBe("Not a validator");
  });

  it("tripwire: Wallet shows exactly the derived word for every validator state, and Validator active only for the active record", async () => {
    const c = await mount("p2");
    c.setState({ route: "wallet", wTab: "overview" });
    for (const v of ["none", "locked", "staked", "active", "exiting", "slashed"]) {
      c.setState({ valWord: v });
      const want = Component.validatorWord(Component.valRecord(v, 510), 510);
      const V = c.renderVals();
      expect(V.wlValWord, v).toBe(want);
      if (v !== "active") expect(V.wlValWord, v).not.toBe("Validator active");
    }
  });
});

describe("slice 3: Wallet shows unknown, never zero", () => {
  const wallet = async (persona, tab = "overview", extra = {}) => {
    const c = await mount(persona);
    c.setState({ route: "wallet", wTab: tab, ...extra });
    return c;
  };

  it("an unread balance is —, not 0", async () => {
    const c = await wallet("p2");
    const t = c.renderVals().wlTiles.find((x) => x.k === "SALT · liquid");
    expect(t?.v).toBe("—");
    expect(t?.src).toMatch(/unknown, not 0/);
  });

  it("the grant in a locked bond is not staked: the Staked tile says so and shows no amount", async () => {
    const c = await wallet("p3"); // P3: Bond locked · not staked
    const t = c.renderVals().wlTiles.find((x) => x.k === "Staked");
    expect(t?.v).toBe("Bond locked · not staked");
    expect(t?.v).not.toMatch(/\d/);
  });

  it("Send waits while the balance is unread: nothing reaches the ceremony", async () => {
    const c = await wallet("p2");
    const ok = c.walletSend("0x" + "ab".repeat(20), "1.5");
    expect(ok).toBe(false);
    expect(c.state.queue).toHaveLength(0);
    expect(c.state.toast ?? "").toMatch(/balance/i);
  });

  it.each([["0x123", "1"], ["0x" + "ab".repeat(20), "1e3"], ["0x" + "ab".repeat(20), "1,000"], ["0x" + "ab".repeat(20), "0"], ["0x" + "ab".repeat(20), ""], ["ab".repeat(21), "1"]])(
    "Send refuses a malformed address or amount (%s, %s)",
    async (to, amt) => {
      const c = await wallet("p2", "overview", { wlBal: "read" });
      expect(c.walletSend(to, amt)).toBe(false);
      expect(c.state.queue).toHaveLength(0);
    },
  );

  it("with the balance read, Send opens one ceremony; approving shows Pending, then Included in block —", async () => {
    const c = await wallet("p2", "overview", { wlBal: "read" });
    expect(c.walletSend("0x" + "ab".repeat(20), "1.5")).toBe(true);
    expect(c.state.queue).toHaveLength(1);
    c.approveCer();
    await vi.advanceTimersByTimeAsync(3400);
    c.setState({ wTab: "activity" });
    expect(c.renderVals().wlTxRows[0]).toMatchObject({ kind: "Send", amount: "−1.5 SALT", word: "Pending" });
    await vi.advanceTimersByTimeAsync(3000);
    const row = c.renderVals().wlTxRows[0];
    expect(row?.word).toBe("Included in block —");
    expect(row?.word).not.toMatch(/final|settled/i);
  });

  it("declining the ceremony records nothing", async () => {
    const c = await wallet("p2", "overview", { wlBal: "read" });
    c.walletSend("0x" + "ab".repeat(20), "2");
    c.finishCer("declined");
    await settle();
    expect(c.state.wlTx).toHaveLength(0);
  });

  it("the error and unavailable states replace the screen with a card that names the call", async () => {
    const c = await wallet("p2");
    for (const v of ["error", "unavailable", "empty"]) {
      c.setVariant("wl-overview", v);
      const V = c.renderVals();
      expect(V.wlCard, v).toBe(true);
      expect(V.wlOverview, v).toBe(false);
      expect(V.wlCardWhy, v).toBeTruthy();
    }
  });

  it("activation from Wallet › Staking is refused until the node is Synced", async () => {
    const c = await wallet("p3", "staking"); // P3: node off, bond locked
    expect(c.activateFromWallet()).toBe(false);
    expect(c.state.queue).toHaveLength(0);
    expect(c.state.toast ?? "").toMatch(/Synced/);
  });

  it("activation goes through the ceremony and lands on Registered, never straight to Validator active", async () => {
    const c = await wallet("p4", "staking"); // P4: node synced, bond locked
    expect(c.activateFromWallet()).toBe(true);
    expect(c.state.valWord).toBe("locked");
    c.approveCer();
    await vi.advanceTimersByTimeAsync(3400);
    expect(c.state.valWord).toBe("staked");
    expect(c.renderVals().wlValWord).toBe("Registered · active from epoch —");
  });

  it("the grant explanation is core's grantCopy, with the disclaimer and the betanet line", async () => {
    const c = await wallet("p3", "staking");
    const g = c.renderVals().wlCards.find((x) => x.label === "Wallet grant");
    expect(g?.lead).toBe(Component.grantCustody({ amountSalt: null, unlockBlock: null, staked: false, granted: true }));
    expect(g?.lead).toMatch(/until block — \(not read yet\)/);
    expect(g?.note).toContain("The grant is not purchased, and it is not an investment.");
    expect(g?.note).toContain("Betanet. What carries over to mainnet is set in the genesis spec.");
  });

  it("durations in blocks show the measured time, never 12 s blocks", async () => {
    const c = await wallet("p2", "staking");
    const rows = c.renderVals().wlCards.flatMap((x) => x.rows);
    expect(rows.find((r) => r.k === "Withdrawal delay")?.v).toBe("50,400 blocks (~28 h)");
  });

  it("tripwire: this session's transactions and join phases are never saved", async () => {
    const c = await mount("p2");
    c.setState({ wlTx: [{ id: "t", kind: "Send", amount: "−1 SALT", hash: "0x1", state: "pending" }], gxJoin: { code: "X", phase: "joining" } });
    c.save();
    await vi.advanceTimersByTimeAsync(500);
    const saved = JSON.parse(localStorage.getItem(SAVE_KEY));
    expect(saved).not.toHaveProperty("wlTx");
    expect(saved).not.toHaveProperty("gxJoin");
  });
});

describe("slice 3: membership price (C1-P #379) and money copy", () => {
  it("priceLabel mirrors core's membershipPriceLabel: a price only when it was read, whole and per a known interval", () => {
    const P = (o) => Component.priceLabel(o);
    expect(P({ currency: "usd", unitAmount: 4800, interval: "year", displayAmount: "D" })).toBe("D a year");
    expect(P({ currency: "usd", unitAmount: 400, interval: "month", displayAmount: "D" })).toBe("D a month");
    expect(P(null)).toBeNull();
    expect(P({ currency: "usd", unitAmount: 0, interval: "year", displayAmount: "D" })).toBeNull();
    expect(P({ currency: "usd", unitAmount: 4800, interval: "year", displayAmount: "" })).toBeNull();
    expect(P({ currency: "usd", unitAmount: 4800, interval: "week", displayAmount: "D" })).toBeNull();
  });

  it("Settings › Billing shows the pricing link, not a number, when the price wasn't read", async () => {
    const c = await mount("p2");
    c.setState({ route: "settings", sSec: "billing", priceRead: false });
    const V = c.renderVals();
    expect(V.stgPriceText).toBe("See membership pricing");
    expect(V.stgPriceUrl).toBe("https://citrate.ai/membership");
    expect(V.stgPriceIsLink).toBe(true);
  });

  it("a read price is drawn as the served slot, never as an amount the prototype made up", async () => {
    const c = await mount("p2");
    c.setState({ route: "settings", sSec: "billing", priceRead: true });
    const V = c.renderVals();
    expect(V.stgPriceText).toBe(Component.priceLabel(Component.PRICE_SHAPE));
    expect(V.stgPriceText).not.toMatch(/\d/);
    expect(V.stgPriceIsLink).toBe(false);
  });

  it("tripwire: no pricing link points at membership.citrate.ai, and no membership amount is hard-coded", () => {
    const html = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8");
    expect(html).not.toMatch(/href="https:\/\/membership\.citrate\.ai"/);
    expect(html).not.toMatch(/\$48/);
  });
});

describe("slice 3: Groups", () => {
  const groups = async (persona, tab = "chat") => {
    const c = await mount(persona);
    c.setState({ route: "groups", gTab: tab });
    return c;
  };
  const J = () => Component.JOIN();

  it("click path 1: the onboarding join lists the group in Groups › Chat and opens it", async () => {
    const c = await mount("p1");
    c.setState({ stage: "j", jStage: "join", jSub: "idle", web: null, invite: { code: J().code, cluster: J().cluster, goal: J().goal, inviter: J().inviter, verified: true } });
    c.renderVals().onOb2Primary();
    await vi.advanceTimersByTimeAsync(1700);
    expect(c.state.groups.map((g) => g.name)).toContain(J().cluster);
    c.setState({ stage: "done", route: "groups", gTab: "chat" });
    const V = c.renderVals();
    expect(V.gxList.map((g) => g.name)).toContain(J().cluster);
    expect(V.gxSelName).toBe(J().cluster);
  });

  it("a pasted invite is only reviewed; nothing joins until the explicit Join click", async () => {
    const c = await groups("p3");
    expect(c.gxPaste("https://citrate.ai/join/" + J().code + "#" + J().key)).toBe(true);
    expect(c.renderVals().gxJoinWord).toBe("Ready to join " + J().cluster);
    await settle();
    expect(c.state.groups.some((g) => g.name === J().cluster)).toBe(false);
    c.gxJoinNow();
    expect(c.renderVals().gxJoinWord).toBe("Joining " + J().cluster + "…");
    expect(c.state.groups.some((g) => g.name === J().cluster)).toBe(false);
    await vi.advanceTimersByTimeAsync(1600);
    expect(c.renderVals().gxJoinWord).toBe("Joined " + J().cluster);
    expect(c.state.groups.some((g) => g.name === J().cluster)).toBe(true);
  });

  it("Join twice redeems once", async () => {
    const c = await groups("p3");
    c.gxPaste(J().code + "#" + J().key);
    c.gxJoinNow();
    expect(c.gxJoinNow()).toBe(false);
    await vi.advanceTimersByTimeAsync(1600);
    expect(c.state.groups.filter((g) => g.name === J().cluster)).toHaveLength(1);
  });

  it("an unknown code reads We couldn't find that invite, never expired; a link without its key asks for the full link", async () => {
    const c = await groups("p3");
    c.gxPaste("ZZZZ2222#" + J().key);
    expect(c.renderVals().gxJoinWord).toBe("We couldn’t find that invite");
    expect(c.renderVals().gxJoinWord).not.toMatch(/expired/i);
    c.gxPaste(J().code);
    expect(c.renderVals().gxJoinWord).toBe("This link is missing its key");
  });

  it.each(["", "hello", "https://evil.example/join/" + "7KQ2M9XD", "7KQ2M9X", "7kq2m9xd#" + "a".repeat(22), "7KQ2M9XD#short"])("junk is refused and nothing is kept (%s)", async (text) => {
    const c = await groups("p3");
    expect(c.gxPaste(text)).toBe(false);
    expect(c.state.gxJoin).toBeNull();
  });

  it("pasting an invite to a group you're in says Already a member", async () => {
    const c = await groups("p2"); // P2 is in Orchard Lab
    c.gxPaste(J().code + "#" + J().key);
    expect(c.renderVals().gxJoinWord).toBe("Already a member");
    expect(c.gxJoinNow()).toBe(false);
  });

  it("a message goes to the selected group; an empty one is refused", async () => {
    const c = await groups("p2");
    const before = c.state.groups.find((g) => g.id === c.state.gSel).msgs.length;
    expect(c.gxSend("   ")).toBe(false);
    expect(c.gxSend("See you at the sync")).toBe(true);
    const g = c.state.groups.find((x) => x.id === c.state.gSel);
    expect(g.msgs).toHaveLength(before + 1);
    expect(g.msgs.at(-1)).toMatchObject({ who: "You", text: "See you at the sync" });
  });

  it("with the relay unreachable, nothing is sent and the relay chip says so", async () => {
    const c = await groups("p2");
    c.setVariant("gx-chat", "error");
    expect(c.gxSend("hello")).toBe(false);
    expect(c.renderVals().gxRelay ?? "").toMatch(/unreachable/i);
  });

  it("an invite link starts Not used yet and Revoke makes it Withdrawn", async () => {
    const c = await groups("p2", "members");
    c.setState({ gSel: "g-night" }); // P2 owns Night shift ops
    expect(c.gxInvite()).toBe(true);
    let row = c.renderVals().gxInvites[0];
    expect(row?.word).toMatch(/^Not used yet · expires \d{4}-\d{2}-\d{2}$/);
    expect(row?.code ?? "").not.toContain(J().key);
    row?.revoke?.();
    row = c.renderVals().gxInvites[0];
    expect(row?.word).toBe("Withdrawn");
    expect(row?.canRevoke).toBe(false);
  });

  it("a member who isn't owner or admin can't invite or change roles", async () => {
    const c = await groups("p2", "members");
    c.setState({ gSel: "g-orchard" }); // P2 is a member here
    expect(c.gxInvite()).toBe(false);
    const r = c.state.groups.find((g) => g.id === "g-orchard").roster[2];
    expect(c.gxRoster(r.addr, "admin")).toBe(false);
    expect(c.state.queue).toHaveLength(0);
  });

  it("role changes stop at the signature ceremony: the roster changes only after approval", async () => {
    const c = await groups("p2", "members");
    c.setState({ gSel: "g-night" });
    const target = c.state.groups.find((g) => g.id === "g-night").roster.find((r) => r.role === "admin");
    expect(c.gxRoster(target.addr, "remove")).toBe(true);
    expect(c.state.queue).toHaveLength(1);
    expect(c.state.groups.find((g) => g.id === "g-night").roster.some((r) => r.addr === target.addr)).toBe(true);
    c.approveCer();
    await vi.advanceTimersByTimeAsync(2400);
    expect(c.state.groups.find((g) => g.id === "g-night").roster.some((r) => r.addr === target.addr)).toBe(false);
  });

  it("Rounds says Group rounds not live yet, with the flag off or on, until the ledgers are deployed", async () => {
    const c = await groups("p2", "rounds");
    for (const flag of [false, true]) {
      c.setState({ roundsFlag: flag, roundsLedger: "none" });
      const lead = c.renderVals().gxCards[0]?.lead;
      expect(lead ?? "", String(flag)).toMatch(/^Group rounds not live yet\./);
    }
  });

  it("fixture round states appear only with the round-card flag on, and carry the fixture tag", async () => {
    const c = await groups("p2", "rounds");
    c.setState({ roundsFlag: false });
    expect(c.renderVals().gxCards.some((x) => x.label === "Groups round states")).toBe(false);
    c.setState({ roundsFlag: true });
    const st = c.renderVals().gxCards.find((x) => x.label === "Groups round states");
    expect(st?.fx).toBe(true);
    expect(st?.rows?.every((r) => r.fx)).toBe(true);
  });

  it("the ALF tab is listed only with the ALF claim, and its route says members only without it", async () => {
    const c = await groups("p2");
    expect(c.renderVals().gxTabs.map((t) => t.label)).not.toContain("ALF");
    c.setState({ gTab: "alf" });
    const V = c.renderVals();
    expect(V.gxCard).toBe(true);
    expect(V.gxCardTitle).toBe("ALF members only");
    c.setState({ alfClaim: true });
    expect(c.renderVals().gxTabs.map((t) => t.label)).toContain("ALF");
    expect(c.renderVals().gxCard).toBe(false);
  });

  it("the tab strip follows K0: Chat, Rounds, Members, Cluster (+ ALF), with Groups and Alerts under Chat", async () => {
    const c = await groups("p4"); // P4 carries the ALF claim
    const V = c.renderVals();
    expect(V.gxTabs.map((t) => t.label)).toEqual(["Chat", "Rounds", "Members", "Cluster", "ALF"]);
    expect(V.gxSubTabs.map((t) => t.label)).toEqual(["Groups", "Alerts"]);
  });

  it("sharing to a cluster needs the cluster joined, and shares each file once", async () => {
    const c = await groups("p2", "cluster");
    c.setState({ gSel: "g-night" }); // not in that cluster
    expect(c.gxShare()).toBe(false);
    c.gxCluster(true);
    const n = c.state.files.length;
    for (let i = 0; i < n + 1; i++) c.gxShare();
    const files = c.state.groups.find((g) => g.id === "g-night").cluster.files.map((f) => f.name);
    expect(new Set(files).size).toBe(files.length);
    expect(files).toHaveLength(n);
  });
});

describe("slice 3: every screen of the five places and Settings is annotated (board: prototype v1)", () => {
  const screens = [
    ...["chat", "journal", "work", "learn"].map((t) => ["hermes " + t, { route: "hermes", hTab: t }]),
    ...[1, 2, 3].map((d) => ["hermes controls " + d, { route: "hermes", hTab: "controls", cDepth: d }]),
    ...["chat", "alerts", "rounds", "members", "cluster"].map((t) => ["groups " + t, { route: "groups", gTab: t }]),
    ["groups alf", { route: "groups", gTab: "alf", alfClaim: true }],
    ...["overview", "staking", "activity", "identity"].map((t) => ["wallet " + t, { route: "wallet", wTab: t }]),
    ...["status", "services", "rewards", "advanced"].map((p) => ["node " + p, { route: "node", nPage: p }]),
    ...["files", "memory", "pins"].map((x) => ["node storage " + x, { route: "node", nPage: "storage", nStore: x }]),
    ...["catalog", "installed"].map((p) => ["store " + p, { route: "store", stPage: p }]),
    ...["account", "connections", "api", "keys", "billing", "privacy", "app"].map((x) => ["settings " + x, { route: "settings", sSec: x }]),
  ];
  it.each(screens)("%s names its screen, route, bridge call and genesis flag, with the four variants", async (_, st) => {
    const c = await mount("p2");
    c.setState(st);
    const V = c.renderVals();
    expect(V.annShow).toBe(true);
    for (const k of ["annScreen", "annRoute", "annBridge", "annFlag"]) {
      expect(typeof V[k], k).toBe("string");
      expect(V[k].length, k).toBeGreaterThan(2);
      expect(V[k], k).not.toMatch(/stub/i);
    }
    expect(V.annHasVariants).toBe(true);
    expect(V.annVariants.map((x) => x.label)).toEqual(["data", "empty", "error", "unavailable"]);
  });

  it("no place is a stub any more", async () => {
    const c = await mount("p2");
    for (const route of ["hermes", "groups", "wallet", "node", "store", "settings"]) {
      c.setState({ route });
      const V = c.renderVals();
      expect(V.rStub, route).toBeFalsy();
      expect(V.annScreen, route).not.toMatch(/stub/i);
    }
  });
});

describe("slice 3: copy follows the UI truth contract on every Wallet, Groups and Settings state", () => {
  // Strings the slice-3 screens bind (wl*, gx*, stg*, plus setNav), collected recursively.
  const strings = (V) => {
    const out = [];
    const walk = (v) => {
      if (typeof v === "string") out.push(v);
      else if (Array.isArray(v)) v.forEach(walk);
      else if (v && typeof v === "object") Object.values(v).forEach(walk);
    };
    for (const [k, v] of Object.entries(V)) if (/^(wl|gx|stg)[A-Z]/.test(k) || k === "setNav") walk(v);
    return out;
  };
  const DISCLAIMER = "The grant is not purchased, and it is not an investment.";
  const BANNED = /\b(final|finali[sz]ed|finality is running|settled|irreversible|earn(?:s|ed|ing|ings)?|income|projected|projection|estimate[sd]?|apy|apr|yield|invest(?:ed|ing|ment|ments)?|profit|passive|appreciation|validating|producing again)\b/i;

  it("tripwire: the slice-3 template text has no banned word or price number either", () => {
    const html = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8");
    const a = html.indexOf("<!-- slice3:start -->");
    const b = html.indexOf("<!-- slice3:end -->");
    expect(a).toBeGreaterThan(0);
    expect(b).toBeGreaterThan(a);
    const text = html.slice(a, b).replace(/<!--[\s\S]*?-->/g, " ").replace(/\{\{[^}]*\}\}/g, " ").replace(/<[^>]+>/g, " ");
    expect(text).not.toMatch(BANNED);
    expect(text).not.toMatch(/\$\s?\d/);
  });

  it.each(["p1", "p2", "p3", "p4"])("%s: no banned word, no price number, no SALT price in any state", async (persona) => {
    const c = await mount(persona);
    c.setState({ stage: "done", web: null });
    const states = [
      ...["overview", "staking", "activity", "identity"].map((t) => [{ route: "wallet", wTab: t }, "wl-" + t]),
      ...["chat", "alerts", "rounds", "members", "cluster", "alf", "community"].map((t) => [{ route: "groups", gTab: t }, "gx-" + t]),
      ...["account", "connections", "api", "keys", "billing", "privacy", "app"].map((x) => [{ route: "settings", sSec: x }, "stg-" + x]),
    ];
    for (const [st, key] of states) {
      for (const variant of ["data", "empty", "error", "unavailable"]) {
        for (const extra of [{}, { roundsFlag: true, alfClaim: true, priceRead: true, wlBal: "read", wlRows: "drawn" }]) {
          c.setState({ ...st, ...extra, variants: { [key]: variant } });
          for (const text of strings(c.renderVals())) {
            const t = text.split(DISCLAIMER).join("");
            expect(t, key + " " + variant).not.toMatch(BANNED);
            expect(t, key + " " + variant).not.toMatch(/\$\s?\d/);
            expect(t, key + " " + variant).not.toMatch(/SALT price|price of SALT|USD/);
          }
        }
      }
    }
  }, 60000);
});

describe("slice 3 review (#387 Codex round 2)", () => {
  it("with the join service unavailable, Copy link copies the app-only link it shows, not a citrate.ai link", async () => {
    const c = await mount("p2");
    c.setState({ route: "groups", gTab: "members", gSel: "g-night", gxSvc: "down" });
    const copied = [];
    c.copy = (text) => copied.push(text);
    expect(c.gxInvite()).toBe(true);
    const row = c.renderVals().gxInvites[0];
    expect(row.code).toMatch(/^citrate:\/\/invite\?/);
    row.copy();
    expect(copied).toHaveLength(1);
    expect(copied[0]).toMatch(/^citrate:\/\/invite\?/);
    expect(copied[0]).not.toMatch(/citrate\.ai\/join/);
  });

  it("with the join service live, Copy link copies that invite's own citrate.ai link with its key", async () => {
    const c = await mount("p2");
    c.setState({ route: "groups", gTab: "members", gSel: "g-night", gxSvc: "live" });
    const copied = [];
    c.copy = (text) => copied.push(text);
    c.gxInvite();
    const inv = c.state.groups.find((g) => g.id === "g-night").invites[0];
    c.renderVals().gxInvites[0].copy();
    expect(copied[0]).toMatch(new RegExp("^https://citrate\\.ai/join/" + inv.code + "#[A-Za-z0-9_-]{22}$"));
  });

  it("tripwire: an invite's key and link are never saved", async () => {
    const c = await mount("p2");
    c.setState({ route: "groups", gTab: "members", gSel: "g-night" });
    for (const svc of ["live", "down"]) {
      c.setState({ gxSvc: svc });
      c.gxInvite();
    }
    c.save();
    await vi.advanceTimersByTimeAsync(500);
    const saved = localStorage.getItem(SAVE_KEY);
    for (const link of Object.values(c.inviteLinks)) {
      const key = /[#&]k?=?([A-Za-z0-9_-]{22})$/.exec(link)[1];
      expect(saved).not.toContain(key);
    }
  });

  it.each(["data", "empty", "error", "unavailable"])("without the ALF claim, the %s variant never shows ALF content", async (variant) => {
    const c = await mount("p2"); // P2 has no ALF claim
    c.setState({ route: "groups", gTab: "alf", alfClaim: false, variants: { "gx-alf": variant } });
    const V = c.renderVals();
    expect(V.gxCards).toHaveLength(0);
    expect(JSON.stringify(V.gxCards)).not.toMatch(/present/);
    expect(V.gxCard).toBe(true);
    if (variant !== "error") expect(V.gxCardTitle).toBe("ALF members only");
  });
});

describe("slice 2 follow-ups (core #393): web join page actions", () => {
  const joinPage = async (os, extra = {}) => {
    const c = await mount("p1");
    c.setState({ web: "join", joinState: "direct", inviterVerified: true, os, wbDl: false, ...extra });
    c.copied = [];
    c.copy = (text, note) => { c.copied.push(text); c.toast(note || "Copied"); };
    return c;
  };

  it("on Linux, Copy the install command copies the full command with the invite key", async () => {
    const c = await joinPage("linux");
    const V = c.renderVals();
    expect(V.wbPrimary).toBe("Copy the install command");
    V.onWbPrimary();
    expect(c.copied).toHaveLength(1);
    const J = Component.JOIN();
    expect(c.copied[0]).toBe("curl -fsSL https://citrate.ai/install.sh | CITRATE_JOIN='" + J.code + "#" + J.key + "' sh");
  });

  it("on Mac, the primary download copies nothing", async () => {
    const c = await joinPage("mac");
    c.renderVals().onWbPrimary();
    expect(c.copied).toHaveLength(0);
  });

  it.each([["phone", {}], ["windows", { winBuilt: false }]])("on %s with no primary action, the secondary action still has a label", async (os, extra) => {
    const c = await joinPage(os, extra);
    const V = c.renderVals();
    expect(V.wbPrimary).toBe("");
    expect(V.wbSecondary).toBeTruthy();
  });

  it("tripwire: the secondary action is not nested inside the primary action's conditional", () => {
    const html = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8");
    const open = '<sc-if value="{{ wbPrimary }}"';
    const i = html.indexOf(open);
    expect(i).toBeGreaterThan(0);
    // The matching </sc-if>, counting nested sc-if blocks.
    let depth = 0, j = i;
    const re = /<sc-if\b|<\/sc-if>/g;
    re.lastIndex = i;
    let m;
    while ((m = re.exec(html))) {
      depth += m[0] === "</sc-if>" ? -1 : 1;
      if (depth === 0) { j = m.index; break; }
    }
    expect(html.slice(i, j)).not.toContain("wbSecondary");
    expect(html).toContain('<sc-if value="{{ wbSecondary }}"');
  });
});

describe("slice 2 review: sample data that a secret scanner must not mistake for a key", () => {
  it("the sample invite key is an obvious placeholder of the real key's length", () => {
    const J = Component.JOIN();
    expect(J.key).toMatch(/^SAMPLE-not-a-real-key/);
    expect(J.key).toMatch(/^[A-Za-z0-9_-]{22}$/);
  });
});

describe("slice 2 review (core #417): withdraw wording needs an activated bond", () => {
  it("tripwire: no copy offers withdrawing a bond without saying it must be activated", () => {
    const html = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8");
    expect(html).not.toMatch(/withdrawing an unlocked bond/);
    for (const m of html.matchAll(/[^.'"]*withdraw(?:ing)? your bond[^.'"]*/gi)) expect(m[0]).toMatch(/activated/);
  });
});

describe("slice 3 review (DGX, UI truth contract v1.2)", () => {
  const wallet = async (persona, tab, extra = {}) => {
    const c = await mount(persona);
    c.setState({ route: "wallet", wTab: tab, ...extra });
    return c;
  };
  const rows = (V) => (V.wlCards || []).flatMap((x) => x.rows);

  it.each(["none", "locked"])("a bond that isn't activated (%s) is never shown as withdrawable", async (valWord) => {
    const c = await wallet("p3", "staking", { valWord, hasGrant: true });
    const w = rows(c.renderVals()).find((r) => r.k === "Withdraw");
    expect(w?.v).toBe("only an activated bond can be withdrawn");
    expect(w?.src ?? "").not.toMatch(/verify identity to withdraw/);
  });

  it.each(["staked", "active"])("an activated bond (%s) withdraws after the lock, gated on the bond's own identity check", async (valWord) => {
    const c = await wallet("p2", "staking", { valWord });
    const w = rows(c.renderVals()).find((r) => r.k === "Withdraw");
    expect(w?.v).toBe("not before the lock end");
    expect(w?.src ?? "").toMatch(/MemberBond\.isKycVerified/);
  });

  it("Identity names the bond's on-chain identity gate, not the userinfo claim, and says withdraw only for an activated bond", async () => {
    for (const valWord of ["locked", "active"]) {
      const c = await wallet("p2", "identity", { valWord });
      const r = rows(c.renderVals()).find((x) => x.k === "Identity verification");
      expect(r.src, valWord).not.toMatch(/Needed to withdraw/);
      if (valWord === "locked") expect(r.src).not.toMatch(/withdraw/i);
    }
  });

  it("tripwire: with an unactivated bond, no Wallet or Settings text implies the principal can be withdrawn", async () => {
    const c = await mount("p3");
    c.setState({ valWord: "locked", hasGrant: true, stage: "done", web: null });
    const all = [];
    for (const st of [{ route: "wallet", wTab: "staking" }, { route: "wallet", wTab: "identity" }, { route: "settings", sSec: "billing" }]) {
      c.setState(st);
      const V = c.renderVals();
      const walk = (v) => (typeof v === "string" ? all.push(v) : Array.isArray(v) ? v.forEach(walk) : v && typeof v === "object" ? Object.values(v).forEach(walk) : 0);
      for (const [k, v] of Object.entries(V)) if (/^(wl|stg)[A-Z]/.test(k)) walk(v);
    }
    for (const t of all.filter((x) => /withdraw/i.test(x) && /bond|lock|identity|principal|grant/i.test(x))) expect(t).toMatch(/only an activated bond can be withdrawn|activated/);
  });

  it("rounds read Group rounds not live yet in both ledger states, with deployed, no rounds as the row value", async () => {
    const c = await mount("p2");
    c.setState({ route: "groups", gTab: "rounds", roundsFlag: true, roundsLedger: "dormant", gTab2: null });
    const V = c.renderVals();
    expect(V.gxCards[0].lead).toMatch(/^Group rounds not live yet\./);
    expect(V.gxCards[0].rows.find((r) => r.k === "FederatedRoundLedger").v).toBe("deployed, no rounds (code at the pinned address)");
    c.setState({ gTab: "chat" });
    expect(c.renderVals().gxRoundWord).toBe("Group rounds not live yet");
  });

  it("tripwire: “Deployed · dormant” and “dormant” never appear on screen", () => {
    const html = readFileSync(DESIGN + "CitrateCore.dc.html", "utf8").replace(/<!--[\s\S]*?-->/g, "");
    const code = html.replace(/\/\/[^\n]*/g, "");
    expect(code).not.toMatch(/Deployed · dormant/i);
    for (const m of code.matchAll(/'[^'\n]*dormant[^'\n]*'/gi)) expect(m[0], m[0]).toMatch(/^'dormant'$/);
  });

  it("a reverted transaction reads Included in block N · reverted, with the fee charged, never nothing moved", async () => {
    const c = await wallet("p2", "activity", { wlRows: "drawn" });
    const r = c.renderVals().wlTxRows;
    const rev = r.find((x) => /reverted/.test(x.word));
    expect(rev?.word).toBe("Included in block — · reverted");
    expect(rev?.sub ?? "").toMatch(/^receipt status 0: the transfer didn't happen; the fee was charged/);
    expect(rev?.sub ?? "").toMatch(/gasUsed × effectiveGasPrice/);
    for (const x of r) {
      expect(x.word).not.toMatch(/^Failed|nothing moved|no fee/i);
      expect(x.sub).not.toMatch(/nothing moved|no fee/i);
    }
    expect(r.map((x) => x.word)).toContain("Sent, outcome unknown — check tx ⟨hash⟩");
  });
});
