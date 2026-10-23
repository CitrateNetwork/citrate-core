// K3 — Node › Services: the nine supervised processes, read through the status commands that exist
// today. Phase comes from each command's supervisor state; readiness (D-12) only when a DTO carries
// it (SC2), otherwise "—". A service with no status command reads unknown, never Stopped.
import { describe, expect, it, vi } from "vitest";
import {
  SERVICES,
  phaseFrom,
  readinessFrom,
  readServices,
  restartService,
  canRestart,
  type ServiceReads,
  type ServiceControls,
  type ServiceRow,
} from "./services";

function reads(over: Partial<ServiceReads> = {}): ServiceReads {
  return {
    node: async () => ({ state: "running" }),
    agent: async () => ({ state: "running" }),
    memory: async () => ({ state: "starting" }),
    llama: async () => ({ state: "restarting" }),
    ipfs: async () => true,
    hermes: async () => ({ state: "failed" }),
    commsRelay: async () => "connected",
    ...over,
  };
}

const row = (rows: ServiceRow[], id: string) => rows.find((r) => r.id === id) as ServiceRow;

describe("the service registry", () => {
  it("lists the nine supervised processes once, in the design's order", () => {
    expect(SERVICES.map((d) => d.name)).toEqual(["Node", "Node agent", "Memory", "Llama chat", "Llama embed", "IPFS", "Hermes", "Comms", "Cluster"]);
    expect(new Set(SERVICES.map((d) => d.id)).size).toBe(9);
  });

  it("every service names its status source, or says it has none (Rule 7)", () => {
    for (const d of SERVICES) expect(d.statusSource.length, d.id).toBeGreaterThan(0);
    expect(SERVICES.find((d) => d.id === "embed")?.statusSource).toMatch(/none yet/);
    expect(SERVICES.find((d) => d.id === "cluster")?.statusSource).toMatch(/none yet/);
  });
});

describe("phase and readiness mapping (SPEC-001, D-12)", () => {
  it("maps the supervisor vocabulary onto SPEC-001 phases", () => {
    expect(phaseFrom("stopped")).toBe("stopped");
    expect(phaseFrom("starting")).toBe("starting");
    expect(phaseFrom("running")).toBe("alive");
    expect(phaseFrom("restarting")).toBe("backoff");
    expect(phaseFrom("failed")).toBe("failed");
    // The D-12 phases a later snapshot adds pass through.
    expect(phaseFrom("stopping")).toBe("stopping");
    expect(phaseFrom("quarantined")).toBe("quarantined");
    expect(phaseFrom("Quarantined")).toBe("quarantined");
    // Anything else is unknown, never a guessed phase.
    expect(phaseFrom("weird")).toBeNull();
    expect(phaseFrom(undefined)).toBeNull();
  });

  it("readiness is read only from a D-12 field", () => {
    expect(readinessFrom("NotAssessed")).toBe("notAssessed");
    expect(readinessFrom("ready")).toBe("ready");
    expect(readinessFrom("Stale")).toBe("stale");
    expect(readinessFrom(undefined)).toBeNull();
    expect(readinessFrom("healthy")).toBeNull();
  });
});

describe("readServices", () => {
  it("reads every service through its own command", async () => {
    const rows = await readServices(reads());
    expect(rows.map((r) => r.id)).toEqual(SERVICES.map((d) => d.id));
    expect(row(rows, "node").phase).toBe("alive");
    expect(row(rows, "memory").phase).toBe("starting");
    expect(row(rows, "llama").phase).toBe("backoff");
    expect(row(rows, "hermes").phase).toBe("failed");
    expect(row(rows, "ipfs").phase).toBe("alive");
  });

  it("readiness is '—' (null) when the DTO has none, and the D-12 value when it does", async () => {
    const rows = await readServices(reads({ llama: async () => ({ state: "running", readiness: "Ready" }) }));
    expect(row(rows, "node").readiness).toBeNull();
    expect(row(rows, "llama").readiness).toBe("ready");
    // `healthy` is the supervisor's Running, not a readiness probe: it is never shown as Ready.
    const r2 = await readServices(reads({ llama: async () => ({ state: "running", healthy: true }) }));
    expect(row(r2, "llama").readiness).toBeNull();
  });

  it("services without a process status read unknown with the reason, never Stopped", async () => {
    const rows = await readServices(reads());
    for (const id of ["embed", "cluster", "comms"]) {
      expect(row(rows, id).phase, id).toBeNull();
      expect(row(rows, id).note.length, id).toBeGreaterThan(0);
    }
    expect(row(rows, "comms").note).toContain("connected");
  });

  it("ipfs_status false is not Stopped: the bool can't tell stopped, starting and failed apart", async () => {
    const rows = await readServices(reads({ ipfs: async () => false }));
    expect(row(rows, "ipfs").phase).toBeNull();
    expect(row(rows, "ipfs").note).toMatch(/not running/i);
  });

  it("a failed read is that row's error, and the other rows still read", async () => {
    const rows = await readServices(reads({ memory: async () => Promise.reject("socket not found") }));
    expect(row(rows, "memory").phase).toBeNull();
    expect(row(rows, "memory").error).toContain("socket not found");
    expect(row(rows, "node").phase).toBe("alive");
  });
});

describe("restart", () => {
  function controls(): ServiceControls & { calls: string[] } {
    const calls: string[] = [];
    const c = (id: string) => ({
      stop: vi.fn(async () => void calls.push(id + ".stop")),
      start: vi.fn(async () => void calls.push(id + ".start")),
    });
    return { calls, node: c("node"), agent: c("agent"), memory: c("memory"), llama: c("llama"), ipfs: c("ipfs"), hermes: c("hermes") };
  }

  it("stops, waits until the supervisor reports stopped, then starts", async () => {
    const c = controls();
    const states = ["running", "stopped"];
    const status = vi.fn(async () => phaseFrom(states.shift() ?? "stopped"));
    const out = await restartService("hermes", c, status, { pollMs: 1, timeoutMs: 1000 });
    expect(out.ok).toBe(true);
    expect(c.calls).toEqual(["hermes.stop", "hermes.start"]);
    expect(status).toHaveBeenCalledTimes(2);
  });

  it("does not start a second process while the old one is still owned", async () => {
    const c = controls();
    const status = vi.fn(async () => phaseFrom("running"));
    const out = await restartService("memory", c, status, { pollMs: 1, timeoutMs: 20 });
    expect(out.ok).toBe(false);
    expect(out.message).toMatch(/still/i);
    expect(c.calls).toEqual(["memory.stop"]);
  });

  it("a stopped or failed service is started without a stop", async () => {
    const c = controls();
    const out = await restartService("ipfs", c, async () => "stopped", { pollMs: 1, timeoutMs: 20 }, "failed");
    expect(out.ok).toBe(true);
    expect(c.calls).toEqual(["ipfs.start"]);
  });

  it("services with no stop/start command cannot be restarted, and the reason is named", () => {
    for (const id of ["embed", "comms", "cluster"] as const) {
      const r = canRestart(id, "alive");
      expect(r.ok, id).toBe(false);
      expect(r.why, id).toMatch(/no .*command/i);
    }
  });

  it("Quarantined, Starting and Stopping refuse a restart (SPEC-001: never on a timer, never over an owned process)", () => {
    expect(canRestart("ipfs", "quarantined").ok).toBe(false);
    expect(canRestart("node", "starting").ok).toBe(false);
    expect(canRestart("node", "stopping").ok).toBe(false);
    expect(canRestart("node", "alive").ok).toBe(true);
    expect(canRestart("node", "failed").ok).toBe(true);
    // An unknown phase is not restartable from here: nothing is known about what is owned.
    expect(canRestart("node", null).ok).toBe(false);
  });

  it("a failed start is reported with its reason", async () => {
    const c = controls();
    c.llama.start = vi.fn(async () => Promise.reject("model not verified"));
    const out = await restartService("llama", c, async () => "stopped", { pollMs: 1, timeoutMs: 20 }, "stopped");
    expect(out.ok).toBe(false);
    expect(out.message).toContain("model not verified");
  });
});
