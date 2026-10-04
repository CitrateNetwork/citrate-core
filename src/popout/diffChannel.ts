// =====================================================================
// citrate-core — the Code and diff pop-out's request channel (HUP-S5.4)
//
// The pop-out holds no app commands (capabilities/popout.json). It asks the main window, which
// checks every request again and answers from Rust: a session's checkpointed steps
// (`hermes_checkpoints`) and one step's diff (`hermes_checkpoint_diff`). Read-only: nothing in this
// channel changes a file, undoes a step or signs. Undo stays in the chat card and the Activity
// monitor.
//
// Same transport and event as the other pop-out channels (bridge.ts); the message types are
// disjoint, so each side ignores the others' messages. Every message is versioned and checked on
// receipt, and the pop-out validates every answer before it renders it.
// =====================================================================
import { MAIN_LABEL, type BridgeTransport } from "./bridge";
import { popoutLabel } from "./kinds";
import { validSessionId, type CheckpointList, type CheckpointStep } from "../agent/fileChanges";
import { parseStepDiff, type StepDiff } from "./diffModel";

export const DIFF_OPS = ["initial", "steps", "diff"] as const;
export type DiffOpName = (typeof DIFF_OPS)[number];

export interface DiffFocus {
  session: string;
  seq: number | null;
}

export interface DiffOpArgs {
  initial: Record<string, never>;
  steps: { session: string };
  diff: { session: string; seq: number };
}

export interface DiffOpResults {
  initial: DiffFocus | null;
  steps: CheckpointList;
  diff: StepDiff;
}

export type DiffOps = { [K in DiffOpName]: (args: DiffOpArgs[K]) => Promise<DiffOpResults[K]> };

type Request = { v: 1; type: "diff.request"; id: string; op: DiffOpName; args: unknown };
type Response = { v: 1; type: "diff.response"; id: string; ok: true; result: unknown } | { v: 1; type: "diff.response"; id: string; ok: false; error: string };
type Focus = { v: 1; type: "diff.focus"; session: string; seq: number | null };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const ID = /^[A-Za-z0-9_-]{1,64}$/;
const isSeq = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v > 0 && v <= Number.MAX_SAFE_INTEGER;
const MAX_STEPS = 500;
const MAX_PATHS = 200;

export function checkArgs<K extends DiffOpName>(op: K, args: unknown): DiffOpArgs[K] | null {
  if (!isObj(args)) return null;
  switch (op) {
    case "initial":
      return {} as DiffOpArgs[K];
    case "steps":
      return validSessionId(args.session) ? ({ session: args.session } as DiffOpArgs[K]) : null;
    case "diff":
      return validSessionId(args.session) && isSeq(args.seq) ? ({ session: args.session, seq: args.seq } as DiffOpArgs[K]) : null;
    default:
      return null;
  }
}

export function parseRequest(raw: unknown): Request | null {
  if (!isObj(raw) || raw.v !== 1 || raw.type !== "diff.request") return null;
  if (typeof raw.id !== "string" || !ID.test(raw.id)) return null;
  if (typeof raw.op !== "string" || !(DIFF_OPS as readonly string[]).includes(raw.op)) return null;
  return { v: 1, type: "diff.request", id: raw.id, op: raw.op as DiffOpName, args: raw.args };
}

export function parseResponse(raw: unknown): Response | null {
  if (!isObj(raw) || raw.v !== 1 || raw.type !== "diff.response") return null;
  if (typeof raw.id !== "string" || !ID.test(raw.id)) return null;
  if (raw.ok === true) return { v: 1, type: "diff.response", id: raw.id, ok: true, result: raw.result };
  if (raw.ok === false && typeof raw.error === "string") return { v: 1, type: "diff.response", id: raw.id, ok: false, error: raw.error.slice(0, 2000) };
  return null;
}

export function parseFocus(raw: unknown): Focus | null {
  if (!isObj(raw) || raw.v !== 1 || raw.type !== "diff.focus") return null;
  if (!validSessionId(raw.session)) return null;
  if (raw.seq !== null && !isSeq(raw.seq)) return null;
  return { v: 1, type: "diff.focus", session: raw.session, seq: raw.seq };
}

function parseStep(v: unknown): CheckpointStep | null {
  if (!isObj(v) || !isSeq(v.seq) || typeof v.status !== "string" || v.status.length > 20) return null;
  if (!Array.isArray(v.paths) || v.paths.length > MAX_PATHS || !v.paths.every((p) => typeof p === "string" && p.length <= 4096)) return null;
  if (typeof v.root !== "string" || v.root.length > 4096) return null;
  return { seq: v.seq, status: v.status, paths: v.paths as string[], root: v.root };
}

/** Check an answer for its op before the pop-out renders it. */
export function checkResult<K extends DiffOpName>(op: K, raw: unknown): { ok: true; value: DiffOpResults[K] } | { ok: false } {
  const v = checkValue(op, raw);
  return v === BAD ? { ok: false } : { ok: true, value: v as DiffOpResults[K] };
}

const BAD = Symbol("bad");

function checkValue(op: DiffOpName, raw: unknown): unknown {
  switch (op) {
    case "initial": {
      if (raw === null) return null;
      if (!isObj(raw) || !validSessionId(raw.session) || (raw.seq !== null && !isSeq(raw.seq))) return BAD;
      return { session: raw.session, seq: raw.seq };
    }
    case "steps": {
      if (!isObj(raw) || !validSessionId(raw.session) || typeof raw.enabled !== "boolean") return BAD;
      if (!Array.isArray(raw.steps) || raw.steps.length > MAX_STEPS) return BAD;
      const steps: CheckpointStep[] = [];
      for (const s of raw.steps) {
        const p = parseStep(s);
        if (!p) return BAD;
        steps.push(p);
      }
      const note = typeof raw.note === "string" ? raw.note.slice(0, 500) : null;
      const list: CheckpointList = { session: raw.session, enabled: raw.enabled, steps, note };
      return list;
    }
    case "diff":
      return parseStepDiff(raw) ?? BAD;
    default:
      return BAD;
  }
}

/** The main window's side: answer the pop-out's requests through `ops`. */
export async function createDiffHost(t: BridgeTransport, ops: DiffOps): Promise<{ focus(session: string, seq: number | null): Promise<void>; close(): void }> {
  let open = true;
  const target = popoutLabel("diff");
  const reply = (msg: Response) => (open ? t.send(target, msg) : Promise.resolve());
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const req = parseRequest(raw);
    if (!req) return;
    const args = checkArgs(req.op, req.args);
    if (!args) {
      void reply({ v: 1, type: "diff.response", id: req.id, ok: false, error: "the request was malformed" }).catch(() => undefined);
      return;
    }
    const run = ops[req.op] as (a: unknown) => Promise<unknown>;
    void run(args)
      .then((result) => reply({ v: 1, type: "diff.response", id: req.id, ok: true, result }))
      .catch((e: unknown) => reply({ v: 1, type: "diff.response", id: req.id, ok: false, error: e instanceof Error ? e.message : String(e) }))
      .catch(() => undefined);
  });
  return {
    focus: (session, seq) => (open ? t.send(target, { v: 1, type: "diff.focus", session, seq } satisfies Focus) : Promise.resolve()),
    close() {
      open = false;
      unlisten();
    },
  };
}

export interface DiffClient {
  call<K extends DiffOpName>(op: K, args: DiffOpArgs[K]): Promise<DiffOpResults[K]>;
  close(): void;
}

/** The pop-out's side: send a request, resolve with the main window's checked answer. */
export async function createDiffClient(t: BridgeTransport, onFocus?: (focus: DiffFocus) => void, timeoutMs = 60_000): Promise<DiffClient> {
  let open = true;
  let seq = 0;
  const pending = new Map<string, { op: DiffOpName; resolve: (v: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const f = parseFocus(raw);
    if (f) {
      onFocus?.({ session: f.session, seq: f.seq });
      return;
    }
    const r = parseResponse(raw);
    if (!r) return;
    const p = pending.get(r.id);
    if (!p) return;
    pending.delete(r.id);
    clearTimeout(p.timer);
    if (!r.ok) {
      p.reject(new Error(r.error));
      return;
    }
    const checked = checkResult(p.op, r.result);
    if (checked.ok) p.resolve(checked.value);
    else p.reject(new Error("the main window sent an answer that could not be read"));
  });
  return {
    call(op, args) {
      if (!open) return Promise.reject(new Error("the Code and diff window is closed"));
      const id = `d${Date.now().toString(36)}-${++seq}`;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(id);
          reject(new Error("the main window did not answer"));
        }, timeoutMs);
        pending.set(id, { op, resolve: resolve as (v: unknown) => void, reject, timer });
        const msg: Request = { v: 1, type: "diff.request", id, op, args };
        t.send(MAIN_LABEL, msg).catch((e: unknown) => {
          pending.delete(id);
          clearTimeout(timer);
          reject(e instanceof Error ? e : new Error(String(e)));
        });
      });
    },
    close() {
      open = false;
      unlisten();
      for (const [, p] of pending) {
        clearTimeout(p.timer);
        p.reject(new Error("the Code and diff window closed"));
      }
      pending.clear();
    },
  };
}
