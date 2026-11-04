// CORE-D4 — Node › Services › Health renders `sidecars_health` (Rust `services.rs`) and nothing
// else: each row's state, restarts, failures in a row, last exit, the slow retry after Failed
// (CORE-D3), the node's liveness note (CORE-D2) and the scrubbed log tail. CORE-D3 alerts show as
// a banner. A failed read clears the rows and says why.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { SidecarsHealthView, until } from "./NodeServices";
import type { SidecarAlert, SidecarHealth, SidecarsHealth } from "../bridge/domains";

const NOW = 1_790_000_000_000;

function health(over: Partial<SidecarHealth> = {}): SidecarHealth {
  return {
    id: "hermes",
    label: "Hermes",
    owner: "src-tauri/src/hermes.rs",
    prerequisites: ["the Hermes sidecar binary is bundled"],
    probe: "HTTP GET /health",
    readySignal: "/health answers 200",
    logBudgetLines: 500,
    logsCommand: null,
    controls: true,
    observation: "observed",
    phase: "alive",
    readiness: "ready",
    lastTransition: { from: "starting", to: "alive", atUnixMs: NOW - 5_000 },
    restarts: 2,
    lastCrash: { atUnixMs: NOW - 120_000, exit: "exit status: 1", restartAttempt: 1 },
    quarantine: null,
    counters: {
      attempts: "3",
      respawns: "2",
      initialFailures: "1",
      consecutiveFailures: "0",
      maxRetries: "8",
      healthCredits: "1",
    },
    slowRetry: null,
    logTail: [
      { ts: NOW - 2_000, stream: "out", line: "listening on 127.0.0.1:18090" },
      { ts: NOW - 1_000, stream: "err", line: "warn: slow tool" },
    ],
    logLinesKept: 120,
    note: null,
    ...over,
  };
}

const read = (services: SidecarHealth[]): SidecarsHealth => ({ takenAtUnixMs: NOW, services });
const render = (h: SidecarsHealth | null, error: string | null = null, alerts: SidecarAlert[] = []) =>
  renderToStaticMarkup(<SidecarsHealthView health={h} error={error} alerts={alerts} />);

describe("Node › Services › Health (sidecars_health)", () => {
  it("names its source and renders one row per sidecar", () => {
    const html = render(read([health(), health({ id: "node", label: "Node" })]));
    expect(html).toContain("source · sidecars_health");
    expect(html.match(/data-health="/g)?.length).toBe(2);
  });

  it("shows restarts, failures in a row, the last exit and the log tail", () => {
    const html = render(read([health()]));
    expect(html).toContain("Restarts: 2");
    expect(html).toContain("Failures in a row: 0 of 9");
    expect(html).toContain("Last exit: exit status: 1 · 2 min ago");
    expect(html).toContain("Last 2 log lines of 120 kept");
    expect(html).toContain("listening on 127.0.0.1:18090");
    expect(html).toContain("! warn: slow tool");
  });

  it("a Failed sidecar says when the slow retry tries it next, or that it stays stopped", () => {
    const failed = health({
      phase: "failed",
      slowRetry: { everyMs: "600000", nextAtUnixMs: String(NOW + 9 * 60_000), attempts: "1" },
    });
    let html = render(read([failed]));
    expect(html).toContain("Next automatic try in 9 min (tried 1 time so far)");
    html = render(read([health({ phase: "failed", slowRetry: null })]));
    expect(html).toContain("It stays stopped until it is started again");
  });

  it("the node row carries its liveness note", () => {
    const html = render(read([health({ id: "node", label: "Node", note: "liveness restart #1 at 5: head stuck at 10000" })]));
    expect(html).toContain("liveness restart #1");
  });

  it("an unobserved row claims no counters, crash or log lines", () => {
    const html = render(
      read([
        health({
          observation: "notSupervised",
          phase: null,
          readiness: null,
          restarts: null,
          lastCrash: null,
          counters: null,
          logTail: [],
          logLinesKept: null,
        }),
      ]),
    );
    expect(html).toContain("Not supervised");
    expect(html).not.toContain("Restarts:");
    expect(html).not.toContain("log line");
  });

  it("a failed read clears the rows and says why", () => {
    const html = render(null, "the browser preview supervises no processes; open the desktop app");
    expect(html).toContain("Health couldn&#x27;t be read");
    expect(html).not.toContain("data-health=");
  });

  it("alerts show as a banner: failed with its next retry, and recovered", () => {
    const alerts: SidecarAlert[] = [
      { id: "memory", label: "Memory", kind: "failed", reason: "exit status: 101", nextRetryAtUnixMs: NOW + 600_000, atUnixMs: NOW },
      { id: "hermes", label: "Hermes", kind: "recovered", reason: null, nextRetryAtUnixMs: null, atUnixMs: NOW },
    ];
    const html = render(read([]), null, alerts);
    expect(html).toContain("Memory failed (exit status: 101). It is tried again in 10 min.");
    expect(html).toContain("Hermes is running again.");
  });

  it("until() counts forward from the read time", () => {
    expect(until(NOW, NOW + 30_000)).toBe("in 30 s");
    expect(until(NOW, NOW + 3 * 3_600_000)).toBe("in 3 h");
    expect(until(NOW, NOW - 5_000)).toBe("in 0 s");
  });
});
