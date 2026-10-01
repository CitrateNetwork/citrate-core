// =====================================================================
// citrate-core — Contract reader pop-out view (HUP-S6.7, US-6.3)
//
// Open any address on chain 40204 (or on the local anvil fork during a hello-mint dry run), see
// CitrateScan's verified source and ABI or paste an ABI, run read calls, ask Hermes to explain a
// function, and send a write to the Signature Ceremony in the main window. Every request goes over
// the contract channel to the main window; this view holds no app commands and signs nothing.
// =====================================================================
import { useEffect, useState, type CSSProperties } from "react";
import { parseEther } from "viem";
import { decodeResult, encodeCall, parseAbi, parseArg, type ReaderFunction } from "../contractReader/abi";
import { describeFunction, explainPrompt } from "../contractReader/explain";
import type { ContractClient, ReaderSource } from "./contractChannel";

const DEFAULT_FORK = "http://127.0.0.1:8545";
const ADDRESS = /^0x[0-9a-fA-F]{40}$/;

const label: CSSProperties = { fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" };
const note: CSSProperties = { fontSize: 11, color: "var(--tx-2)", lineHeight: 1.5 };
const mono: CSSProperties = { fontFamily: "var(--font-mono)", fontSize: 11 };
const input: CSSProperties = { ...mono, width: "100%", boxSizing: "border-box" };

type Origin = "verified" | "partial" | "pasted";
type Outcome = { kind: "ok" | "error" | "info"; text: string };

interface Loaded {
  address: string;
  target: string;
  codeSize: number | null;
  source: ReaderSource | null;
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function statusLine(l: Loaded): string {
  const where = l.target === "citrate" ? "chain 40204" : "the local fork";
  if (l.codeSize === 0) return `There is no contract code at this address on ${where}.`;
  if (l.target !== "citrate") return "Reading from the local fork. CitrateScan only knows chain 40204, so paste the contract's ABI below.";
  const s = l.source;
  if (!s) return "CitrateScan could not be read. Paste the contract's ABI below to read it.";
  if (s.status === "verified") return `Verified on CitrateScan: ${s.contractName ?? "unnamed contract"}${s.compilerVersion ? ` (solc ${s.compilerVersion})` : ""}.`;
  if (s.status === "partial") return `Partial match on CitrateScan: ${s.contractName ?? "unnamed contract"}. Shown for reference; this is not a verified match.`;
  if (s.status === "notContract") return "There is no contract code at this address on chain 40204.";
  return "This contract is not verified on CitrateScan. Paste its ABI below to read it.";
}

function FunctionRow({
  fn,
  canWrite,
  run,
}: {
  fn: ReaderFunction;
  canWrite: boolean;
  run: {
    read(fn: ReaderFunction, args: unknown[]): Promise<Outcome>;
    write(fn: ReaderFunction, args: unknown[], valueWei: string): Promise<Outcome>;
    explain(fn: ReaderFunction): Promise<Outcome>;
  };
}) {
  const [values, setValues] = useState<string[]>(() => fn.inputs.map(() => ""));
  const [salt, setSalt] = useState("");
  const [result, setResult] = useState<Outcome | null>(null);
  const [explanation, setExplanation] = useState<Outcome | null>(null);
  const [busy, setBusy] = useState(false);
  const d = describeFunction(fn);

  const args = (): unknown[] => fn.inputs.map((p, i) => {
    try {
      return parseArg(p, values[i] ?? "");
    } catch (e) {
      throw new Error(`${p.name || `input ${i + 1}`}: ${errText(e)}`);
    }
  });
  const act = async (f: () => Promise<Outcome>, set: (o: Outcome) => void) => {
    setBusy(true);
    try {
      set(await f());
    } catch (e) {
      set({ kind: "error", text: errText(e) });
    } finally {
      setBusy(false);
    }
  };
  const valueWei = (): string => {
    if (!fn.payable || salt.trim() === "") return "0";
    try {
      return parseEther(salt.trim()).toString();
    } catch {
      throw new Error("the SALT amount is not a number");
    }
  };

  return (
    <div data-testid={`cr-fn-${fn.signature}`} style={{ display: "flex", flexDirection: "column", gap: 6, padding: "10px 0", borderBottom: "1px solid var(--line-1)" }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span className="mono" style={{ ...mono, fontSize: 12, color: "var(--tx-1)", flex: 1 }}>{fn.signature}</span>
        <span className="mono" style={{ ...label, color: fn.kind === "read" ? "var(--ok)" : "var(--warn)" }}>{fn.kind === "read" ? "read" : fn.payable ? "write · payable" : "write"}</span>
      </div>
      <span style={note}>{d.facts.join(" ")}</span>
      {d.cautions.map((c) => (
        <span key={c} style={{ ...note, color: "var(--warn)" }}>{c}</span>
      ))}
      {fn.inputs.map((p, i) => (
        <input
          key={i}
          data-testid={`cr-in-${fn.signature}-${i}`}
          className="input"
          style={input}
          placeholder={`${p.name || `input ${i + 1}`} (${p.type})`}
          value={values[i] ?? ""}
          onChange={(e) => setValues((v) => v.map((x, j) => (j === i ? e.target.value : x)))}
        />
      ))}
      {fn.payable && fn.kind === "write" && (
        <input data-testid={`cr-value-${fn.signature}`} className="input" style={input} placeholder="SALT to send with the call (optional)" value={salt} onChange={(e) => setSalt(e.target.value)} />
      )}
      <div style={{ display: "flex", gap: 6 }}>
        {fn.kind === "read" ? (
          <button className="btn btn-sm btn-secondary" data-testid={`cr-read-${fn.signature}`} disabled={busy} onClick={() => void act(() => run.read(fn, args()), setResult)}>
            Read
          </button>
        ) : (
          <button
            className="btn btn-sm btn-secondary"
            data-testid={`cr-write-${fn.signature}`}
            disabled={busy || !canWrite}
            title={canWrite ? "Opens the Signature Ceremony in the main window" : "Writes are proposed only on chain 40204"}
            onClick={() => void act(() => run.write(fn, args(), valueWei()), setResult)}
          >
            Propose in ceremony
          </button>
        )}
        <button className="btn btn-sm btn-ghost" data-testid={`cr-explain-${fn.signature}`} disabled={busy} onClick={() => void act(() => run.explain(fn), setExplanation)}>
          Explain
        </button>
      </div>
      {result && (
        <span data-testid={`cr-result-${fn.signature}`} className="mono" style={{ ...mono, color: result.kind === "error" ? "var(--danger)" : "var(--tx-1)", whiteSpace: "pre-wrap", wordBreak: "break-all" }}>
          {result.text}
        </span>
      )}
      {explanation && (
        <span data-testid={`cr-explanation-${fn.signature}`} style={{ ...note, color: explanation.kind === "error" ? "var(--danger)" : "var(--tx-1)", whiteSpace: "pre-wrap" }}>
          {explanation.text}
        </span>
      )}
    </div>
  );
}

export function ContractReader({ client }: { client: ContractClient }) {
  const [address, setAddress] = useState("");
  const [mode, setMode] = useState<"citrate" | "fork">("citrate");
  const [forkUrl, setForkUrl] = useState(DEFAULT_FORK);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [functions, setFunctions] = useState<ReaderFunction[]>([]);
  const [origin, setOrigin] = useState<Origin | null>(null);
  const [abiText, setAbiText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const target = mode === "citrate" ? "citrate" : forkUrl.trim();

  const load = async (addr: string, tgt: string) => {
    setError(null);
    setLoaded(null);
    setFunctions([]);
    setOrigin(null);
    if (!ADDRESS.test(addr.trim())) {
      setError("Enter a contract address: 0x followed by 40 hex digits.");
      return;
    }
    const a = addr.trim();
    setLoading(true);
    try {
      const codeSize = await client.call("codeSize", { target: tgt, address: a }).catch(() => null);
      let source: ReaderSource | null = null;
      if (tgt === "citrate") {
        try {
          source = await client.call("source", { address: a });
        } catch (e) {
          setError(errText(e));
        }
      }
      setLoaded({ address: a, target: tgt, codeSize, source });
      if (source && (source.status === "verified" || source.status === "partial") && source.abi) {
        const parsed = parseAbi(source.abi);
        if (parsed.ok) {
          setFunctions(parsed.functions);
          setOrigin(source.status);
        } else {
          setError(`CitrateScan's ABI could not be used: ${parsed.error}`);
        }
      }
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    void client
      .call("initial", {})
      .then((f) => {
        if (cancelled || !f) return;
        setAddress(f.address);
        if (f.target !== "citrate") {
          setMode("fork");
          setForkUrl(f.target);
        }
        void load(f.address, f.target);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client]);

  const usePasted = () => {
    const parsed = parseAbi(abiText);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }
    setError(null);
    setFunctions(parsed.functions);
    setOrigin("pasted");
  };

  const run = {
    async read(fn: ReaderFunction, args: unknown[]): Promise<Outcome> {
      if (!loaded) throw new Error("load an address first");
      const data = await client.call("view", { target: loaded.target, address: loaded.address, calldata: encodeCall(fn, args) });
      const vals = decodeResult(fn, data);
      return { kind: "ok", text: vals.length ? vals.join("\n") : "(no return value)" };
    },
    async write(fn: ReaderFunction, args: unknown[], valueWei: string): Promise<Outcome> {
      if (!loaded || loaded.target !== "citrate") throw new Error("writes are proposed only on chain 40204");
      await client.call("write", { address: loaded.address, calldata: encodeCall(fn, args), valueWei, label: fn.signature });
      return { kind: "info", text: "Proposed. Review and approve it in the Signature Ceremony in the main window." };
    },
    async explain(fn: ReaderFunction): Promise<Outcome> {
      if (!loaded) throw new Error("load an address first");
      const prompt = explainPrompt({
        address: loaded.address,
        contractName: loaded.source?.contractName ?? null,
        verified: origin ?? "pasted",
        fn,
        source: origin === "pasted" ? null : loaded.source?.source ?? null,
      });
      const r = await client.call("explain", { prompt });
      return { kind: "ok", text: `Hermes (${r.by}): ${r.text}` };
    },
  };

  const showAbiInput = loaded !== null && loaded.codeSize !== 0 && origin !== "verified" && origin !== "partial";

  return (
    <div data-testid="contract-reader" data-register="instrument" style={{ minHeight: "100vh", boxSizing: "border-box", padding: "14px 16px", background: "var(--srf-0)", color: "var(--tx-1)", fontFamily: "var(--font-sans)", display: "flex", flexDirection: "column", gap: 8 }}>
      <span style={{ fontSize: 14, fontWeight: 500 }}>Contract reader</span>
      <div style={{ display: "flex", gap: 6 }}>
        <button className={"btn btn-sm " + (mode === "citrate" ? "btn-secondary" : "btn-ghost")} data-testid="cr-target-citrate" onClick={() => setMode("citrate")}>
          Chain 40204
        </button>
        <button className={"btn btn-sm " + (mode === "fork" ? "btn-secondary" : "btn-ghost")} data-testid="cr-target-fork" onClick={() => setMode("fork")}>
          Local fork
        </button>
        {mode === "fork" && <input className="input" data-testid="cr-fork-url" style={{ ...input, flex: 1 }} value={forkUrl} onChange={(e) => setForkUrl(e.target.value)} />}
      </div>
      <div style={{ display: "flex", gap: 6 }}>
        <input className="input" data-testid="cr-address" style={{ ...input, flex: 1 }} placeholder="Contract address (0x...)" value={address} onChange={(e) => setAddress(e.target.value)} />
        <button className="btn btn-sm btn-primary" data-testid="cr-load" disabled={loading} onClick={() => void load(address, target)}>
          {loading ? "Loading" : "Open"}
        </button>
      </div>
      {error && (
        <span data-testid="cr-error" role="alert" style={{ ...note, color: "var(--danger)" }}>
          {error}
        </span>
      )}
      {loaded && (
        <span data-testid="cr-status" style={note}>
          {statusLine(loaded)}
        </span>
      )}
      {loaded?.source?.note && <span style={note}>{loaded.source.note}</span>}
      {showAbiInput && (
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <span className="mono" style={label}>ABI</span>
          <textarea data-testid="cr-abi" className="input" style={{ ...input, minHeight: 70, resize: "vertical" }} placeholder="Paste the contract's ABI (JSON array)" value={abiText} onChange={(e) => setAbiText(e.target.value)} />
          <button className="btn btn-sm btn-secondary" data-testid="cr-use-abi" style={{ alignSelf: "flex-start" }} onClick={usePasted}>
            Use this ABI
          </button>
        </div>
      )}
      {origin && (
        <span data-testid="cr-abi-origin" className="mono" style={label}>
          {origin === "verified" ? "ABI: verified on CitrateScan" : origin === "partial" ? "ABI: partial match, not verified" : "ABI: pasted by you, not verified"}
        </span>
      )}
      {loaded && loaded.target !== "citrate" && functions.some((f) => f.kind === "write") && (
        <span style={note}>Writes on the local fork are not proposed here. On chain 40204 a write opens the Signature Ceremony.</span>
      )}
      {loaded && functions.map((fn) => <FunctionRow key={fn.signature} fn={fn} canWrite={loaded.target === "citrate"} run={run} />)}
      {loaded?.source?.source && origin !== "pasted" && (
        <details>
          <summary className="mono" style={label}>Source</summary>
          <pre data-testid="cr-source" style={{ ...mono, whiteSpace: "pre-wrap", maxHeight: 360, overflow: "auto", color: "var(--tx-2)" }}>
            {loaded.source.source}
          </pre>
        </details>
      )}
    </div>
  );
}
