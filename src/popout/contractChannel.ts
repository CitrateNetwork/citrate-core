// =====================================================================
// citrate-core — the Contract reader's request channel (HUP-S6.7)
//
// The Contract reader pop-out holds no app commands (capabilities/popout.json). It asks the main
// window, which validates every request again and runs it: CitrateScan source lookups, code-size
// checks and view calls (read-only RPC), explanations (Hermes, no tools), and write proposals,
// which open the SignatureCeremony in the main window. Nothing in this channel can sign.
//
// Same transport and event as the monitor bridge (bridge.ts); the message types are disjoint, so
// each side ignores the other's messages. Every message is versioned and checked on receipt.
// =====================================================================
import { MAIN_LABEL, type BridgeTransport } from "./bridge";
import { popoutLabel } from "./kinds";
import type { ContractSourceView } from "../bridge/domains";

export const CONTRACT_OPS = ["initial", "source", "codeSize", "view", "write", "explain"] as const;
export type ContractOp = (typeof CONTRACT_OPS)[number];

/** What CitrateScan says about an address (mirrors Rust `contract_reader::VerifiedSource`). */
export type ReaderSource = ContractSourceView;

export interface ContractOpArgs {
  /** What the main window asked the reader to open, if anything (asked once at start). */
  initial: Record<string, never>;
  source: { address: string };
  codeSize: { target: string; address: string };
  view: { target: string; address: string; calldata: string };
  write: { address: string; calldata: string; valueWei: string; label: string };
  explain: { prompt: string };
}

export interface ContractOpResults {
  initial: { address: string; target: string } | null;
  source: ReaderSource;
  codeSize: number;
  view: string;
  write: { proposed: true };
  explain: { text: string; by: string };
}

/** The main window's implementation of each request. */
export type ContractOps = { [K in ContractOp]: (args: ContractOpArgs[K]) => Promise<ContractOpResults[K]> };

type Request = { v: 1; type: "contract.request"; id: string; op: ContractOp; args: unknown };
type Response = { v: 1; type: "contract.response"; id: string; ok: true; result: unknown } | { v: 1; type: "contract.response"; id: string; ok: false; error: string };
type Focus = { v: 1; type: "contract.focus"; address: string; target: string };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const ADDRESS = /^0x[0-9a-fA-F]{40}$/;
const HEX = /^0x(?:[0-9a-fA-F]{2}){4,65536}$/;
const MAX_TARGET = 200;
const MAX_PROMPT = 40_000;
const MAX_LABEL = 200;
const ID = /^[A-Za-z0-9_-]{1,64}$/;

const str = (v: unknown, max: number): v is string => typeof v === "string" && v.length <= max;

/** Check a request's arguments for its op; null if anything is off. */
export function checkArgs<K extends ContractOp>(op: K, args: unknown): ContractOpArgs[K] | null {
  if (!isObj(args)) return null;
  const a = args;
  switch (op) {
    case "initial":
      return {} as ContractOpArgs[K];
    case "source":
      return typeof a.address === "string" && ADDRESS.test(a.address) ? ({ address: a.address } as ContractOpArgs[K]) : null;
    case "codeSize":
      return str(a.target, MAX_TARGET) && typeof a.address === "string" && ADDRESS.test(a.address)
        ? ({ target: a.target, address: a.address } as ContractOpArgs[K])
        : null;
    case "view":
      return str(a.target, MAX_TARGET) && typeof a.address === "string" && ADDRESS.test(a.address) && typeof a.calldata === "string" && HEX.test(a.calldata)
        ? ({ target: a.target, address: a.address, calldata: a.calldata } as ContractOpArgs[K])
        : null;
    case "write":
      return typeof a.address === "string" &&
        ADDRESS.test(a.address) &&
        typeof a.calldata === "string" &&
        HEX.test(a.calldata) &&
        typeof a.valueWei === "string" &&
        /^\d{1,40}$/.test(a.valueWei) &&
        str(a.label, MAX_LABEL)
        ? ({ address: a.address, calldata: a.calldata, valueWei: a.valueWei, label: a.label } as ContractOpArgs[K])
        : null;
    case "explain":
      return str(a.prompt, MAX_PROMPT) && a.prompt.length > 0 ? ({ prompt: a.prompt } as ContractOpArgs[K]) : null;
    default:
      return null;
  }
}

export function parseRequest(raw: unknown): Request | null {
  if (!isObj(raw) || raw.v !== 1 || raw.type !== "contract.request") return null;
  if (typeof raw.id !== "string" || !ID.test(raw.id)) return null;
  if (typeof raw.op !== "string" || !(CONTRACT_OPS as readonly string[]).includes(raw.op)) return null;
  return { v: 1, type: "contract.request", id: raw.id, op: raw.op as ContractOp, args: raw.args };
}

export function parseResponse(raw: unknown): Response | null {
  if (!isObj(raw) || raw.v !== 1 || raw.type !== "contract.response") return null;
  if (typeof raw.id !== "string" || !ID.test(raw.id)) return null;
  if (raw.ok === true) return { v: 1, type: "contract.response", id: raw.id, ok: true, result: raw.result };
  if (raw.ok === false && typeof raw.error === "string") return { v: 1, type: "contract.response", id: raw.id, ok: false, error: raw.error.slice(0, 2000) };
  return null;
}

export function parseFocus(raw: unknown): Focus | null {
  if (!isObj(raw) || raw.v !== 1 || raw.type !== "contract.focus") return null;
  if (typeof raw.address !== "string" || !ADDRESS.test(raw.address)) return null;
  if (!str(raw.target, MAX_TARGET)) return null;
  return { v: 1, type: "contract.focus", address: raw.address, target: raw.target };
}

/** The main window's side: answer the reader's requests through `ops`. */
export async function createContractHost(t: BridgeTransport, ops: ContractOps): Promise<{ focus(address: string, target?: string): Promise<void>; close(): void }> {
  let open = true;
  const target = popoutLabel("contract");
  const reply = (msg: Response) => (open ? t.send(target, msg) : Promise.resolve());
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const req = parseRequest(raw);
    if (!req) return;
    const args = checkArgs(req.op, req.args);
    if (!args) {
      void reply({ v: 1, type: "contract.response", id: req.id, ok: false, error: "the request was malformed" }).catch(() => undefined);
      return;
    }
    const run = ops[req.op] as (a: unknown) => Promise<unknown>;
    void run(args)
      .then((result) => reply({ v: 1, type: "contract.response", id: req.id, ok: true, result }))
      .catch((e: unknown) => reply({ v: 1, type: "contract.response", id: req.id, ok: false, error: e instanceof Error ? e.message : String(e) }))
      .catch(() => undefined);
  });
  return {
    focus: (address, tgt = "citrate") => (open ? t.send(target, { v: 1, type: "contract.focus", address, target: tgt } satisfies Focus) : Promise.resolve()),
    close() {
      open = false;
      unlisten();
    },
  };
}

export interface ContractClient {
  call<K extends ContractOp>(op: K, args: ContractOpArgs[K]): Promise<ContractOpResults[K]>;
  close(): void;
}

/** The pop-out's side: send a request, resolve with the main window's answer. */
export async function createContractClient(t: BridgeTransport, onFocus?: (address: string, target: string) => void, timeoutMs = 300_000): Promise<ContractClient> {
  let open = true;
  let seq = 0;
  const pending = new Map<string, { resolve: (v: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const f = parseFocus(raw);
    if (f) {
      onFocus?.(f.address, f.target);
      return;
    }
    const r = parseResponse(raw);
    if (!r) return;
    const p = pending.get(r.id);
    if (!p) return;
    pending.delete(r.id);
    clearTimeout(p.timer);
    if (r.ok) p.resolve(r.result);
    else p.reject(new Error(r.error));
  });
  return {
    call(op, args) {
      if (!open) return Promise.reject(new Error("the Contract reader is closed"));
      const id = `c${Date.now().toString(36)}-${++seq}`;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(id);
          reject(new Error("the main window did not answer"));
        }, timeoutMs);
        pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer });
        const msg: Request = { v: 1, type: "contract.request", id, op, args };
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
        p.reject(new Error("the Contract reader closed"));
      }
      pending.clear();
    },
  };
}
