// K1a-2 — Node › Services renders the services registry snapshot (Rust `services_snapshot`) and
// nothing else. Every state word on the tab is the one the snapshot reported for that row:
// - a row with no supervisor reads "Not supervised", a contended read "Unknown", never a phase;
// - readiness is its own line (SC2): Alive never reads Ready;
// - controls appear only where the registry allows them, and only for a phase they apply to;
// - logs: only the node has a log read (N-8); the other eight say "not available yet".
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { ServicesView, SERVICE_CONTROLS, PHASE_LABEL, OBSERVATION_LABEL } from "./NodeServices";
import type { ServiceEntry, ServicePhase, ServicesSnapshot, ServiceId } from "../bridge/domains";

const NOW = 1_790_000_000_000;
const IDS: ServiceId[] = ["node", "nodeAgent", "memory", "llamaChat", "llamaEmbed", "ipfs", "hermes", "comms", "cluster"];
const LABELS: Record<ServiceId, string> = {
  node: "Node",
  nodeAgent: "Node agent",
  memory: "Memory",
  llamaChat: "Llama chat",
  llamaEmbed: "Llama embed",
  ipfs: "IPFS",
  hermes: "Hermes",
  comms: "Comms",
  cluster: "Cluster",
};
const CONTROLLED = new Set<ServiceId>(["node", "memory", "hermes"]);

function row(id: ServiceId, over: Partial<ServiceEntry> = {}): ServiceEntry {
  return {
    id,
    label: LABELS[id],
    owner: `src-tauri/src/${id}.rs`,
    prerequisites: ["the binary is bundled"],
    probe: "Process exit only: the supervisor restarts it if it exits",
    readySignal: "None: a process-only service reads Not assessed while it is Alive",
    logBudgetLines: 500,
    logsCommand: id === "node" ? "node_logs" : null,
    controls: CONTROLLED.has(id),
    observation: "notSupervised",
    phase: null,
    readiness: null,
    lastTransition: null,
    restarts: null,
    lastCrash: null,
    quarantine: null,
    ...over,
  };
}

function observed(id: ServiceId, phase: ServicePhase, over: Partial<ServiceEntry> = {}): ServiceEntry {
  return row(id, {
    observation: "observed",
    phase,
    readiness: "notAssessed",
    lastTransition: { from: "starting", to: phase, atUnixMs: NOW - 42_000 },
    restarts: 0,
    ...over,
  });
}

function snap(services: ServiceEntry[]): ServicesSnapshot {
  return { takenAtUnixMs: NOW, services };
}

const noop = () => {};
const render = (s: ServicesSnapshot | null, extra: Partial<Parameters<typeof ServicesView>[0]> = {}) =>
  renderToStaticMarkup(<ServicesView snapshot={s} error={null} pending={{}} onControl={noop} onOpenLogs={noop} {...extra} />);

/** The HTML of one service's row. */
function rowHtml(html: string, id: ServiceId): string {
  const start = html.indexOf(`data-service="${id}"`);
  expect(start, `row ${id} rendered`).toBeGreaterThanOrEqual(0);
  const next = html.indexOf("data-service=", start + 1);
  return html.slice(start, next < 0 ? undefined : next);
}

/** Every state word shown in a row's state slot. */
function stateWords(rowH: string): string[] {
  return [...rowH.matchAll(/data-testid="svc-state"[^>]*>([^<]*)</g)].map((m) => m[1]);
}

const ALL_STATE_WORDS = [...Object.values(PHASE_LABEL), ...Object.values(OBSERVATION_LABEL)];

describe("Node › Services renders the registry snapshot", () => {
  it("lists the nine supervised services in the snapshot's order", () => {
    const html = render(snap(IDS.map((id) => row(id))));
    const order = [...html.matchAll(/data-service="([a-zA-Z]+)"/g)].map((m) => m[1]);
    expect(order).toEqual(IDS);
  });

  it("a row with no supervisor reads Not supervised, with no phase and no readiness claimed", () => {
    const html = render(snap([row("comms")]));
    const r = rowHtml(html, "comms");
    expect(stateWords(r)).toEqual([OBSERVATION_LABEL.notSupervised]);
    expect(r).toContain('data-observation="notSupervised"');
    expect(r).not.toMatch(/Readiness: (Not assessed|Ready|Awaiting|Stale|Unavailable)/);
  });

  it("a contended read is Unknown, never the last phase", () => {
    const r = rowHtml(render(snap([row("cluster", { observation: "busy" })])), "cluster");
    expect(stateWords(r)).toEqual([OBSERVATION_LABEL.busy]);
  });

  it("readiness is its own line: Alive with readiness not assessed never reads Ready", () => {
    const r = rowHtml(render(snap([observed("llamaChat", "alive")])), "llamaChat");
    expect(stateWords(r)).toEqual([PHASE_LABEL.alive]);
    expect(r).toContain("Readiness: Not assessed");
    expect(r).not.toMatch(/>Ready</);
  });

  it("shows readiness Ready only when the snapshot reports it", () => {
    const r = rowHtml(render(snap([observed("hermes", "alive", { readiness: "ready" })])), "hermes");
    expect(r).toContain("Readiness: Ready");
  });

  it("shows the last transition with its age from the snapshot, the restart count and the last crash", () => {
    const r = rowHtml(
      render(
        snap([
          observed("memory", "failed", {
            lastTransition: { from: "backoff", to: "failed", atUnixMs: NOW - 125_000 },
            restarts: 9,
            lastCrash: { atUnixMs: NOW - 125_000, exit: "exit status: 1", restartAttempt: 8 },
          }),
        ]),
      ),
      "memory",
    );
    expect(r).toContain("Restarting after a crash → Failed");
    expect(r).toContain("2 min ago");
    expect(r).toContain("Restarts: 9");
    expect(r).toContain("exit status: 1");
  });

  it("quarantined shows its reason and offers no Start", () => {
    const r = rowHtml(render(snap([observed("node", "quarantined", { quarantine: "the previous incarnation is not cleaned up yet" })])), "node");
    expect(stateWords(r)).toEqual([PHASE_LABEL.quarantined]);
    expect(r).toContain("the previous incarnation is not cleaned up yet");
    expect(r).not.toMatch(/data-control="start"/);
  });

  it("only the node has logs; the other eight say not available yet (N-8)", () => {
    const html = render(snap(IDS.map((id) => row(id))));
    for (const id of IDS) {
      const r = rowHtml(html, id);
      if (id === "node") {
        expect(r).toContain('data-logs="node"');
        expect(r).not.toContain("Logs: not available yet");
      } else {
        expect(r).toContain("Logs: not available yet");
      }
    }
  });

  it("shows the registry: owner, prerequisites, probe, ready signal and log budget", () => {
    const r = rowHtml(render(snap([row("ipfs")])), "ipfs");
    expect(r).toContain("src-tauri/src/ipfs.rs");
    expect(r).toContain("the binary is bundled");
    expect(r).toContain("Process exit only");
    expect(r).toContain("None: a process-only service");
    expect(r).toContain("500 lines");
  });

  it("offers Start and Stop only where the registry allows and the phase applies", () => {
    const html = render(
      snap([
        observed("node", "alive"),
        row("memory"),
        observed("hermes", "stopping"),
        observed("ipfs", "alive"),
        row("comms"),
      ]),
    );
    expect(rowHtml(html, "node")).toMatch(/data-control="stop"/);
    expect(rowHtml(html, "node")).not.toMatch(/data-control="start"/);
    expect(rowHtml(html, "memory")).toMatch(/data-control="start"/);
    expect(rowHtml(html, "hermes")).not.toMatch(/data-control=/);
    expect(rowHtml(html, "ipfs")).not.toMatch(/data-control=/);
    expect(rowHtml(html, "comms")).not.toMatch(/data-control=/);
  });

  it("a control in flight says requested and is disabled; the state stays the snapshot's", () => {
    const r = rowHtml(render(snap([observed("node", "alive")]), { pending: { node: "stop" } }), "node");
    expect(r).toContain("Stop requested");
    expect(r).toMatch(/data-control="stop"[^>]*disabled/);
    expect(stateWords(r)).toEqual([PHASE_LABEL.alive]);
  });

  it("an unreadable snapshot shows the reason and no rows", () => {
    const html = render(null, { error: "the browser preview supervises no processes" });
    expect(html).toContain("the browser preview supervises no processes");
    expect(html).not.toContain("data-service=");
  });
});

describe("tripwire: the tab never shows a state the snapshot didn't report", () => {
  const phases: ServicePhase[] = ["stopped", "starting", "alive", "backoff", "failed", "stopping", "quarantined"];

  it.each(phases)("a row reported %s shows exactly that state word", (phase) => {
    const html = render(snap(IDS.map((id) => (id === "nodeAgent" ? observed(id, phase) : row(id)))));
    const r = rowHtml(html, "nodeAgent");
    expect(stateWords(r)).toEqual([PHASE_LABEL[phase]]);
    expect(r).toContain(`data-phase="${phase}"`);
    for (const id of IDS.filter((i) => i !== "nodeAgent")) {
      expect(stateWords(rowHtml(html, id))).toEqual([OBSERVATION_LABEL.notSupervised]);
    }
  });

  it("a phase word this build doesn't know reads Unknown, never a known phase", () => {
    const odd = observed("ipfs", "alive", { phase: "warming" as unknown as ServicePhase });
    const words = stateWords(rowHtml(render(snap([odd])), "ipfs"));
    expect(words).toEqual([OBSERVATION_LABEL.busy]);
    expect(Object.values(PHASE_LABEL)).not.toContain(words[0]);
  });

  it("every state word on the page is accounted for by a reported row", () => {
    const rows = [observed("node", "alive"), row("memory"), row("cluster", { observation: "busy" })];
    const html = render(snap(rows));
    const shown = [...html.matchAll(/data-testid="svc-state"[^>]*>([^<]*)</g)].map((m) => m[1]);
    expect(shown).toEqual([PHASE_LABEL.alive, OBSERVATION_LABEL.notSupervised, OBSERVATION_LABEL.busy]);
    for (const w of shown) expect(ALL_STATE_WORDS).toContain(w);
  });

  it("the tab reads state only from the snapshot, never from app state or seed data", () => {
    const src = readFileSync("src/surfaces/NodeServices.tsx", "utf8");
    expect(src).not.toMatch(/\bs\.(node|memDaemon|height|peers|syncPct)\b/);
    expect(src).not.toMatch(/data\/seed/);
    expect(src).not.toMatch(/agentSlice\.(use|get)\(\)/);
  });

  it("the controls the tab wires are exactly the registry's controls: true rows (Rust services.rs)", () => {
    const rust = readFileSync("src-tauri/src/services.rs", "utf8");
    // Each registry row, from its `id:` to its `controls:` line.
    const rows = rust
      .split("ServiceDescriptor {")
      .slice(1)
      .map((b) => b.slice(0, b.indexOf("\n", b.indexOf("controls:"))))
      .filter((b) => /id: ServiceId::\w+,/.test(b));
    expect(rows).toHaveLength(9);
    const allowed = rows
      .filter((b) => /controls: true,/.test(b))
      .map((b) => /id: ServiceId::(\w+),/.exec(b)?.[1] ?? "")
      .map((n) => n[0].toLowerCase() + n.slice(1))
      .sort();
    expect(allowed.length).toBeGreaterThan(0);
    expect(Object.keys(SERVICE_CONTROLS).sort()).toEqual(allowed);
  });
});
