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
    expect(V.ob2Lead).toMatch(/Unlocked · verify identity to withdraw/);
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
