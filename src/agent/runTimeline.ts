// =====================================================================
// citrate-core: the run timeline of one Hermes reply (0.5.4 FL design, board 01 HermesChat)
//
// Each step Hermes takes in a turn (think, read, tool, draft) with its state and how long it took,
// plus the receipts the answer shows under it (what it read, and for chain reads the block the
// read ran at, as the read itself reports it). Every row is written from a real event of the turn (Rule 1):
//   - think: the provider reported a model step (sidecar `step_start`, or "thinking" from the
//     in-app loop). Its time is the model server's own report (`usage` prompt_ms + generation_ms)
//     when there is one, otherwise the time between the events as this app received them.
//   - read / tool: a tool call. Core-hosted calls are timed by core around the gated handler;
//     sidecar-hosted and refused calls by their `tool_call` and `tool_result` events. A read is a
//     call whose reviewed annotation says it changes nothing (toolAnnotations.ts).
//   - draft: the answer text streaming in, until the turn ends.
// A row never appears before its event, so there are no "queued" rows for steps Hermes has not
// taken. When the start and end of a step arrived in the same event page (the same receive time),
// its duration is unknown and shown as "—" with that reason (never 0).
//
// A step's state comes only from structured outcomes: the sidecar's tool_result status, or core's
// own record of the member's decision on that call. Never from words in the tool's text.
//
// Blocks: a chain fact names the block only when the read reports the block it ran at (node_status:
// this node's own height; contract_view: the eth_call pinned to the RPC's head, with that RPC's
// host; explain_tx: the transaction's own block). Any other chain read shows "—" with the reason.
// =====================================================================
import { annotationFor } from "./toolAnnotations";

export type RunStepKind = "think" | "read" | "tool" | "draft";
export type RunStepState = "running" | "done" | "refused" | "failed" | "stopped";

export interface RunStep {
  /** Unique within the run. */
  id: string;
  kind: RunStepKind;
  /** The tool's name, for read and tool rows. */
  tool?: string;
  /** The call id, for read and tool rows (sidecar ids repeat across model steps). */
  callId?: string;
  /** The model step number, for think rows, when the provider reports one. */
  step?: number;
  state: RunStepState;
  startedAt: number;
  endedAt: number | null;
  /** Measured duration in ms, or null when it is not known. */
  durationMs: number | null;
  /** Where the duration came from. */
  timing?: "core" | "events" | "model";
  /** One plain line: what came back, or why it was refused. */
  detail?: string;
  /** For a finished chain read: the block the read ran at, as the read reports it (null = not
   *  reported, see `blockReason`). Absent on reads that are not chain facts. */
  block?: number | null;
  /** Where the block came from ("this node", "rpc.citrate.ai", "the transaction's block"). */
  blockSource?: string;
  /** Why a chain read has no block. */
  blockReason?: string;
}

export interface RunReceipt {
  tool: string;
  label: string;
  status: "ok" | "refused" | "failed";
  /** For a chain read: the block it ran at (null = not reported, see `blockReason`). */
  block?: number | null;
  blockSource?: string;
  blockReason?: string;
  chain: boolean;
}

export type RunOutcome = "answered" | "failed" | "stopped";

export interface RunRecord {
  startedAt: number;
  endedAt: number | null;
  steps: RunStep[];
  outcome: RunOutcome | null;
}

/** At most this many steps are kept per run (the oldest are dropped). */
export const MAX_RUN_STEPS = 60;
const DETAIL_CHARS = 160;

/** Tools whose results are chain facts (they read the node or contracts on 40204). */
export const CHAIN_READ_TOOLS: ReadonlySet<string> = new Set([
  "node_status",
  "staking_status",
  "contract_view",
  "get_code",
  "identify_contract",
  "address_book_lookup",
  "explain_tx",
]);

/** Why a chain read without a reported block shows "—". */
export const BLOCK_NOT_REPORTED = "this read does not report the block it ran at";

/**
 * The block a chain read ran at, read only from the parts of its result that core itself writes
 * (never from fenced contract or registry values). Null with a reason when the read reports none.
 */
export function blockFromResult(tool: string, content: string): { block: number; source: string } | { block: null; reason: string } {
  const first = content.split("\n", 1)[0] ?? "";
  if (tool === "node_status") {
    try {
      const v: unknown = JSON.parse(content);
      const h = typeof v === "object" && v !== null ? (v as { height?: unknown }).height : undefined;
      if (typeof h === "number" && Number.isSafeInteger(h) && h > 0) return { block: h, source: "this node" };
    } catch {
      /* not the status JSON */
    }
    return { block: null, reason: "this node did not report its height" };
  }
  if (tool === "contract_view") {
    const m = /^contract_view: read .* \((?:chain \d+|the local fork at [^)]*)\) at block (\d+) on ([A-Za-z0-9.:-]+), using /.exec(first);
    if (m) return { block: Number(m[1]), source: m[2] };
    return { block: null, reason: BLOCK_NOT_REPORTED };
  }
  if (tool === "explain_tx") {
    const line = content.split("\n").find((l) => l.startsWith("Status: "));
    const m = line ? /^Status: [a-z_ -]+ in block (\d+)/i.exec(line) : null;
    if (m) return { block: Number(m[1]), source: "the transaction's block" };
    return { block: null, reason: "the transaction has no block yet" };
  }
  return { block: null, reason: BLOCK_NOT_REPORTED };
}

/** Plain words for the tools members see most; any other tool is named as it is. */
const TOOL_WORDS: Record<string, string> = {
  node_status: "Read this node's status",
  staking_status: "Read your staking position",
  memory_search: "Searched your notes and the Citrate docs",
  memory_recall: "Recalled a note",
  memory_assert: "Saved a note",
  journal_read: "Read your journal",
  journal_append: "Added to your journal",
  groups_list: "Read your groups",
  group_roster: "Read a group's roster",
  group_create: "Created a group",
  group_invite: "Made a group invite",
  directory_find: "Looked in the member directory",
  skills_list: "Read the skill registry",
  skill_run: "Ran a skill",
  skill_write: "Saved a skill",
  models_list: "Read the model registry",
  contract_view: "Read a contract",
  contract_deploy: "Prepared a contract deploy",
  get_code: "Checked for contract code",
  get_verified_source: "Read verified source",
  identify_contract: "Identified a contract",
  address_book_lookup: "Looked up the address book",
  decode_calldata: "Decoded call data",
  decode_log: "Decoded an event log",
  explain_tx: "Read a transaction",
  fl_round_plan: "Read the round coordinator",
  fl_round_start: "Asked to join a round",
  app_navigate: "Opened a place in the app",
  widget_create: "Made a tile",
  schedule_list: "Read your schedule",
  schedule_add: "Added to your schedule",
  calendar_list: "Read your calendar",
  gsheets_read: "Read a sheet",
  gsheets_append: "Added to a sheet",
  belnap_codec: "Encoded Belnap values",
  shell_run: "Ran a command",
  web_search: "Searched the web",
};

/** The row text for a tool call. */
export function toolLabel(tool: string): string {
  if (TOOL_WORDS[tool]) return TOOL_WORDS[tool];
  if (tool.startsWith("mcp__")) {
    const [, server, name] = tool.split("__");
    return `Used ${name || "a tool"} on MCP server ${server || "unknown"}`;
  }
  return `Used ${tool}`;
}

/** Read: a call whose reviewed annotation says it changes nothing. Unknown tools are not reads. */
export function kindForTool(tool: string): "read" | "tool" {
  return annotationFor(tool)?.effect === "none" ? "read" : "tool";
}

/** The row text of any step. */
export function stepLabel(s: RunStep): string {
  if (s.kind === "think") return s.state === "running" ? "Thinking" : "Thought about the next step";
  if (s.kind === "draft") return s.state === "running" ? "Writing the answer" : "Wrote the answer";
  return toolLabel(s.tool ?? "");
}

/** The word a screen reader hears for a state (also the visible word next to the glyph). */
export const STATE_WORD: Record<RunStepState, string> = {
  running: "running",
  done: "done",
  refused: "refused",
  failed: "failed",
  stopped: "stopped",
};

export function newRun(now: number): RunRecord {
  return { startedAt: now, endedAt: null, steps: [], outcome: null };
}

const clip = (t: string) => {
  const one = t.replace(/\s+/g, " ").trim();
  return one.length > DETAIL_CHARS ? one.slice(0, DETAIL_CHARS - 1) + "…" : one;
};

function closeStep(s: RunStep, state: RunStepState, now: number): RunStep {
  if (s.state !== "running") return s;
  const timing = s.timing ?? "events";
  if (timing === "model") return { ...s, state, endedAt: now };
  const measured = Math.max(0, now - s.startedAt);
  // Two events that arrived in the same page carry the same time: their gap is not known.
  return { ...s, state, endedAt: now, durationMs: timing === "events" && measured === 0 ? null : measured, timing };
}

function withSteps(r: RunRecord, steps: RunStep[]): RunRecord {
  return { ...r, steps: steps.slice(-MAX_RUN_STEPS) };
}

/** Close the running think and draft rows (a new step, tool or the end supersedes them). */
function closeOpenText(steps: RunStep[], now: number): RunStep[] {
  return steps.map((s) => (s.state === "running" && (s.kind === "think" || s.kind === "draft") ? closeStep(s, "done", now) : s));
}

let seq = 0;
const nextId = (p: string) => p + ++seq;

/** A model step began. A repeated report for the step already running changes nothing. */
export function runThink(r: RunRecord, now: number, step?: number): RunRecord {
  if (r.outcome) return r;
  const running = r.steps.find((s) => s.kind === "think" && s.state === "running");
  if (running && (step === undefined || running.step === undefined || running.step === step)) {
    return running.step === undefined && step !== undefined ? withSteps(r, r.steps.map((s) => (s === running ? { ...s, step } : s))) : r;
  }
  const steps = closeOpenText(r.steps, now);
  return withSteps(r, steps.concat([{ id: nextId("k"), kind: "think", step, state: "running", startedAt: now, endedAt: null, durationMs: null }]));
}

/** The model server reported its own time for a model call: the latest think row takes it. */
export function runModelTime(r: RunRecord, ms: number | null): RunRecord {
  if (ms === null || !Number.isFinite(ms) || ms < 0) return r;
  const idx = [...r.steps].reverse().findIndex((s) => s.kind === "think" && s.timing !== "model");
  if (idx < 0) return r;
  const at = r.steps.length - 1 - idx;
  return withSteps(r, r.steps.map((s, i) => (i === at ? { ...s, durationMs: ms, timing: "model" as const } : s)));
}

/** The answer text began streaming. */
export function runDraft(r: RunRecord, now: number): RunRecord {
  if (r.outcome || r.steps.some((s) => s.kind === "draft" && s.state === "running")) return r;
  const steps = r.steps.map((s) => (s.state === "running" && s.kind === "think" ? closeStep(s, "done", now) : s));
  return withSteps(r, steps.concat([{ id: nextId("d"), kind: "draft", state: "running", startedAt: now, endedAt: null, durationMs: null }]));
}

/** A tool call began. `timing` says who measures it. */
export function runToolStart(r: RunRecord, callId: string, tool: string, now: number, timing: "core" | "events"): RunRecord {
  if (r.outcome) return r;
  const open = r.steps.some((s) => s.callId === callId && s.tool === tool && s.state === "running");
  if (open) return r;
  const steps = closeOpenText(r.steps, now);
  return withSteps(r, steps.concat([{ id: nextId("t"), kind: kindForTool(tool), tool, callId, state: "running", startedAt: now, endedAt: null, durationMs: null, timing }]));
}

/** A row state from a structured tool status only ("denied" refused, "error" failed). */
export function toolStateOf(status: string): RunStepState {
  if (status === "denied") return "refused";
  if (status === "error") return "failed";
  return "done";
}

/**
 * A tool call ended. The most recent running row with this call id takes the result. A result
 * with no running row (the start was not seen) adds a finished row with no duration.
 */
export function runToolEnd(
  r: RunRecord,
  callId: string,
  tool: string,
  status: string,
  content: string,
  now: number,
): RunRecord {
  const state = toolStateOf(status);
  const detail = state === "done" ? undefined : clip(content.replace(/^(declined|tool error):\s*/i, ""));
  let hit = -1;
  for (let i = r.steps.length - 1; i >= 0; i--) {
    const s = r.steps[i];
    if (s.callId === callId && s.state === "running") {
      hit = i;
      break;
    }
  }
  const b = CHAIN_READ_TOOLS.has(tool) && state === "done" ? blockFromResult(tool, content) : null;
  const blockPart: Partial<RunStep> = !b ? {} : b.block !== null ? { block: b.block, blockSource: b.source } : { block: null, blockReason: b.reason };
  const extra = (s: RunStep): RunStep => ({ ...s, ...(detail ? { detail } : {}), ...blockPart });
  if (hit < 0) {
    // Its start was not seen, so it has no timing at all.
    return withSteps(r, r.steps.concat([extra({ id: nextId("t"), kind: kindForTool(tool), tool, callId, state, startedAt: now, endedAt: now, durationMs: null })]));
  }
  return withSteps(r, r.steps.map((s, i) => (i === hit ? extra(closeStep(s, state, now)) : s)));
}

/** What a tool or read row says when the turn ended before its result arrived. */
export const NO_RESULT = "No result arrived before the reply ended.";

/**
 * The turn ended. Only think and draft rows close as done (an answer ends them); a tool or read
 * row still running had no result, so it is stopped with no duration, and says so.
 */
export function runEnd(r: RunRecord, outcome: RunOutcome, now: number): RunRecord {
  if (r.outcome) return r;
  const steps = r.steps.map((s) => {
    if (s.state !== "running") return s;
    if (outcome === "answered" && (s.kind === "think" || s.kind === "draft")) return closeStep(s, "done", now);
    // A step cut off by Stop, a failure or a missing result has no meaningful duration.
    const cut = { ...closeStep(s, "stopped", now), durationMs: null };
    return s.kind === "read" || s.kind === "tool" ? { ...cut, detail: NO_RESULT } : cut;
  });
  return { ...r, steps, outcome, endedAt: now };
}

/** What the answer read: one receipt per finished tool row, in order, repeats folded. */
export function receiptsOf(r: RunRecord | undefined): RunReceipt[] {
  if (!r) return [];
  const out: RunReceipt[] = [];
  for (const s of r.steps) {
    if ((s.kind !== "read" && s.kind !== "tool") || !s.tool || s.state === "running") continue;
    const status: RunReceipt["status"] = s.state === "refused" ? "refused" : s.state === "done" ? "ok" : "failed";
    const chain = CHAIN_READ_TOOLS.has(s.tool);
    const prev = out.find((x) => x.tool === s.tool && x.status === status && (x.block ?? null) === (s.block ?? null) && x.blockSource === s.blockSource);
    if (prev) continue;
    const blockPart =
      chain && status === "ok"
        ? s.block !== undefined && s.block !== null
          ? { block: s.block, ...(s.blockSource ? { blockSource: s.blockSource } : {}) }
          : { block: null, blockReason: s.blockReason ?? BLOCK_NOT_REPORTED }
        : {};
    out.push({ tool: s.tool, label: toolLabel(s.tool), status, chain, ...blockPart });
  }
  return out;
}

/** Why a finished step has no duration (shown with "—"); null when it has one or is running. */
export function durationReason(s: RunStep): string | null {
  if (s.state === "running" || (s.durationMs !== null && Number.isFinite(s.durationMs))) return null;
  if (s.state === "stopped") return "cut off before it finished";
  if (s.timing === "events") return "start and end arrived together";
  return "its start was not seen";
}

/** "1.2 s", "340 ms", "1 min 4 s"; "" when unknown. */
export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "";
  if (ms < 1) return "under 1 ms";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s < 10 ? s.toFixed(1) : Math.round(s)} s`;
  const m = Math.floor(s / 60);
  return `${m} min ${Math.round(s - m * 60)} s`;
}

/** The header of a finished run: "Worked for 6.1 s", with counts. */
export function runSummary(r: RunRecord): { title: string; meta: string } {
  const reads = r.steps.filter((s) => s.kind === "read").length;
  const tools = r.steps.filter((s) => s.kind === "tool").length;
  const steps = r.steps.length;
  const meta = `${steps} ${steps === 1 ? "step" : "steps"} · ${reads} ${reads === 1 ? "read" : "reads"} · ${tools} ${tools === 1 ? "action" : "actions"}`;
  if (!r.outcome) {
    const running = r.steps.findIndex((s) => s.state === "running");
    return { title: "Working", meta: running >= 0 ? `step ${running + 1} of ${steps} so far` : meta };
  }
  const total = r.endedAt !== null ? formatDuration(r.endedAt - r.startedAt) : "";
  const verb = r.outcome === "stopped" ? "Stopped after" : r.outcome === "failed" ? "Failed after" : "Worked for";
  return { title: total ? `${verb} ${total}` : verb.replace(/ (after|for)$/, ""), meta };
}

/**
 * The one sentence a screen reader hears when the run changes step (announced once per step,
 * never per token). Empty when nothing new is running.
 */
export function stepAnnouncement(r: RunRecord | undefined): string {
  if (!r) return "";
  if (r.outcome) {
    const { title } = runSummary(r);
    return `Hermes ${title.charAt(0).toLowerCase()}${title.slice(1)}.`;
  }
  const s = [...r.steps].reverse().find((x) => x.state === "running");
  return s ? `${stepLabel(s)}.` : "";
}

/** Keep only well-formed steps from a saved run (sealed history is read back through this). */
export function parseRun(raw: unknown): RunRecord | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const r = raw as Record<string, unknown>;
  if (typeof r.startedAt !== "number" || !Array.isArray(r.steps)) return undefined;
  const kinds = new Set(["think", "read", "tool", "draft"]);
  const states = new Set(["running", "done", "refused", "failed", "stopped"]);
  const steps = r.steps
    .filter((s): s is RunStep => typeof s === "object" && s !== null && kinds.has((s as RunStep).kind) && states.has((s as RunStep).state) && typeof (s as RunStep).startedAt === "number")
    .slice(-MAX_RUN_STEPS)
    // A run saved mid-step did not finish that step.
    .map((s) => (s.state === "running" ? { ...s, state: "stopped" as const, durationMs: null } : s));
  const outcome = r.outcome === "answered" || r.outcome === "failed" || r.outcome === "stopped" ? r.outcome : "stopped";
  return { startedAt: r.startedAt, endedAt: typeof r.endedAt === "number" ? r.endedAt : null, steps, outcome };
}
