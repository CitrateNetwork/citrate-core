// =====================================================================
// citrate-core — agent slice (CX-S6 / Agent suite redesign, Pass 1)
//
// State for the Agent surface: the Hermes harness (status + skills + pending approvals),
// a session run-log, and the multi-runtime roster the surface is shaped for. Hermes is the
// ONE wired runtime (bridge.agentHarness — keyless; every chain effect stops at the
// Signature Ceremony). OpenClaw / Grok are runtime ADAPTERS the UI exposes; attaching one
// records it honestly as "adapter pending" — Start reports a plain error, never a fabricated
// running agent (Rule 1). Errors from the bridge (honest Unavailable in a packaged build that
// hasn't wired the tauri seam, honest-empty in sim) are CAUGHT into `error`, never thrown at
// render. Owned by lane s6.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type { AgentSkill, AgentApproval, AgentHarnessStatus } from "../../bridge/domains";

export type RuntimeId = "hermes" | "openclaw" | "grok";

export interface RuntimeOption {
  id: RuntimeId;
  label: string;
  glyph: string;
  sub: string;
  /** A wired runtime has a real bridge (hermes). Adapters are UI-shaped, backend pending. */
  wired: boolean;
}

export interface AttachedRuntime extends RuntimeOption {
  /** off → starting → running, or error with a plain-language reason. */
  state: "off" | "starting" | "running" | "error";
  error: string | null;
}

export interface AgentRun {
  id: string;
  name: string;
  detail: string;
  status: "running" | "done" | "awaiting" | "failed";
  ts: number;
}

export interface AgentState {
  /** The Hermes harness status (running / skills count / pending approvals). */
  status: AgentHarnessStatus;
  skills: AgentSkill[];
  approvals: AgentApproval[];
  /** Session run-log (newest first). */
  runs: AgentRun[];
  /** The runtimes attached to this node. Hermes is present by default (bundled). */
  attached: AttachedRuntime[];
  /** The currently-selected runtime row in the roster. */
  selected: RuntimeId;
  /** A harness read is in flight. */
  loading: boolean;
  /** A start/stop is in flight (for the selected wired runtime). */
  busy: boolean;
  /** The last harness error (honest Unavailable / bridge error), or null. */
  error: string | null;
}

/** The runtimes the attach picker offers. Only Hermes is wired in this build. */
export const RUNTIME_OPTIONS: RuntimeOption[] = [
  { id: "hermes", label: "Hermes", glyph: "HE", sub: "bundled · keyless sidecar on your node", wired: true },
  { id: "openclaw", label: "OpenClaw", glyph: "OC", sub: "attaches over MCP · adapter pending", wired: false },
  { id: "grok", label: "Grok bot", glyph: "GK", sub: "xAI key · adapter pending", wired: false },
];

const HERMES: AttachedRuntime = { ...RUNTIME_OPTIONS[0], state: "off", error: null };

const initial: AgentState = {
  status: { running: false, skills: 0, pendingApprovals: 0 },
  skills: [],
  approvals: [],
  runs: [],
  attached: [HERMES],
  selected: "hermes",
  loading: false,
  busy: false,
  error: null,
};

export const agentSlice = createSlice<AgentState>(initial);

const message = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === "string" ? e : String(e);

// A stable, monotonic id without Date.now()/Math.random() in the module body —
// the run-log only needs uniqueness within a session.
let runSeq = 0;
const nextId = (): string => `run-${++runSeq}`;

function setHermes(patch: Partial<AttachedRuntime>): void {
  agentSlice.set((s) => ({
    attached: s.attached.map((r) => (r.id === "hermes" ? { ...r, ...patch } : r)),
  }));
}

/** Load the harness status + skills + pending approvals. Honest on empty/unavailable. */
export async function refreshAgent(): Promise<void> {
  agentSlice.set({ loading: true });
  try {
    const status = await bridge.agentHarness.status();
    const [skills, approvals] = status.running
      ? await Promise.all([bridge.agentHarness.skills(), bridge.agentHarness.pendingApprovals()])
      : [[], []];
    agentSlice.set({ status, skills, approvals, loading: false, error: null });
    setHermes({ state: status.running ? "running" : "off", error: null });
  } catch (e) {
    // Honest: a build that hasn't wired the tauri harness seam throws Unavailable.
    agentSlice.set({ loading: false, error: message(e), skills: [], approvals: [] });
    setHermes({ state: "off", error: message(e) });
  }
}

/** Start the Hermes harness. Adapters that aren't wired report an honest error instead. */
export async function startRuntime(id: RuntimeId): Promise<void> {
  const rt = agentSlice.get().attached.find((r) => r.id === id);
  if (!rt) return;
  if (!rt.wired) {
    agentSlice.set((s) => ({
      attached: s.attached.map((r) =>
        r.id === id ? { ...r, state: "error", error: `The ${rt.label} adapter isn't available in this build yet — Hermes is the wired runtime. This is where its error + Retry would live.` } : r,
      ),
    }));
    return;
  }
  agentSlice.set({ busy: true });
  setHermes({ state: "starting", error: null });
  try {
    await bridge.agentHarness.start();
    agentSlice.set({ busy: false });
    await refreshAgent();
  } catch (e) {
    agentSlice.set({ busy: false, error: message(e) });
    setHermes({ state: "error", error: message(e) });
  }
}

/** Stop the Hermes harness. Pending approvals stay unexecuted (surfaced by the UI). */
export async function stopRuntime(id: RuntimeId): Promise<void> {
  const rt = agentSlice.get().attached.find((r) => r.id === id);
  if (!rt || !rt.wired) {
    agentSlice.set((s) => ({ attached: s.attached.map((r) => (r.id === id ? { ...r, state: "off", error: null } : r)) }));
    return;
  }
  agentSlice.set({ busy: true });
  try {
    await bridge.agentHarness.stop();
    agentSlice.set({ busy: false });
    await refreshAgent();
  } catch (e) {
    agentSlice.set({ busy: false, error: message(e) });
  }
}

/** Attach a runtime to the node (adds a roster row). Duplicate-attach is guarded. */
export function attachRuntime(id: RuntimeId): void {
  const opt = RUNTIME_OPTIONS.find((o) => o.id === id);
  if (!opt) return;
  agentSlice.set((s) => (s.attached.some((r) => r.id === id) ? {} : { attached: [...s.attached, { ...opt, state: "off", error: null }], selected: id }));
}

/** Select a roster row. */
export function selectRuntime(id: RuntimeId): void {
  agentSlice.set({ selected: id });
}

/**
 * Run a skill behind the mandatory approval flow. The bridge returns {ok}; anything that
 * touches the chain, code, or shell surfaces as a pending approval (refetched here), never
 * silently executed.
 */
export async function runAgentSkill(name: string, argsJson: string): Promise<void> {
  const id = nextId();
  const run: AgentRun = { id, name, detail: argsJson || "no arguments", status: "running", ts: 0 };
  agentSlice.set((s) => ({ runs: [run, ...s.runs].slice(0, 12) }));
  try {
    const { ok } = await bridge.agentHarness.runSkill(name, argsJson);
    const status = await bridge.agentHarness.status();
    const approvals = status.running ? await bridge.agentHarness.pendingApprovals() : [];
    const awaiting = approvals.length > agentSlice.get().approvals.length;
    agentSlice.set((s) => ({
      status,
      approvals,
      runs: s.runs.map((r) => (r.id === id ? { ...r, status: awaiting ? "awaiting" : ok ? "done" : "failed" } : r)),
    }));
  } catch (e) {
    agentSlice.set((s) => ({
      error: message(e),
      runs: s.runs.map((r) => (r.id === id ? { ...r, status: "failed", detail: message(e) } : r)),
    }));
  }
}

/** Drop a resolved approval from the local list after the human acts at the ceremony. */
export function clearApproval(approvalId: string): void {
  agentSlice.set((s) => ({ approvals: s.approvals.filter((a) => a.id !== approvalId) }));
}

/** Record a session run row (e.g. a contract deploy handed to the ceremony). */
export function noteRun(name: string, detail: string, status: AgentRun["status"]): void {
  agentSlice.set((s) => ({ runs: [{ id: nextId(), name, detail, status, ts: 0 }, ...s.runs].slice(0, 12) }));
}
