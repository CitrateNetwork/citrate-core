// =====================================================================
// citrate-core: Groups › Rounds › Adapters, the eval gate (FL design board 09)
//
// Everything here is core's (src-tauri fl_rounds.rs): the gate request, its refusals, the
// decision with each metric, load, unload, and the adapter put back after a restart. The view
// names each of the six checks core enforces before the gate, says which one refused, and shows
// the gate table with each metric's change. It says "the round result says" because core checks
// the round result file, not FederatedRoundLedger itself. Adapter download from LoRAFactory is not
// built: the gate takes a local file.
// =====================================================================
import { useEffect, useState } from "react";
import { BRIDGE_MODE } from "../bridge/mode";
import type { FlAdapterGateRecord, FlAdapterGateRequest, FlOverview, FlRoundsDomain } from "../bridge/domains";
import { Card, Chip, Eyebrow, HicBadge, KV, Note, shortHex, Unknown, type Tone } from "./primitives";

export const CHECKS = [
  { id: "sha", t: "File SHA-256 matches", round: false },
  { id: "accepted", t: "The round result says Accepted", round: true },
  { id: "devices", t: "At least three devices took part", round: true },
  { id: "digests", t: "Chain, bundle and replay digests agree", round: true },
  { id: "replay", t: "Replay found no mismatch and checked the merged adapter", round: true },
  { id: "ids", t: "The round result names one round, with well-formed ids", round: true },
] as const;
export type CheckId = (typeof CHECKS)[number]["id"];

/** Which check a core refusal belongs to, or null for a refusal before the checks (a file read). */
export function refusalCheck(message: string): CheckId | null {
  const m = message.toLowerCase();
  if (/sha256 .* does not match|not a gguf|names merged adapter|expected adapter hash/.test(m)) return "sha";
  if (/not accepted|does not say the round was accepted/.test(m)) return "accepted";
  if (/devices; at least/.test(m)) return "devices";
  if (/record digests disagree/.test(m)) return "digests";
  if (/independent replay/.test(m)) return "replay";
  if (/round id|more than one round|ledger is not an address|adapter hash is not|record digest is not/.test(m)) return "ids";
  return null;
}

export type CheckState = "passed" | "failed" | "not-run" | "not-applicable";

/** The order core runs the checks in (fl_rounds.rs): the round result first, the file hash last. */
export const CORE_ORDER: CheckId[] = ["ids", "digests", "replay", "devices", "accepted", "sha"];

/** Each check's state after a run (or none). A refusal stops core at that check: the ones core
 *  ran before it passed, the ones after it did not run. */
export function checkStates(run: { record: FlAdapterGateRecord | null; refusal: string | null; withRound: boolean } | null): Record<CheckId, CheckState> {
  const out = {} as Record<CheckId, CheckState>;
  const failed = run?.refusal ? refusalCheck(run.refusal) : null;
  const at = failed ? CORE_ORDER.indexOf(failed) : -1;
  for (const c of CHECKS) {
    const i = CORE_ORDER.indexOf(c.id);
    if (!run) out[c.id] = "not-run";
    else if (c.round && !run.withRound) out[c.id] = "not-applicable";
    else if (run.record) out[c.id] = "passed";
    else if (!failed) out[c.id] = "not-run";
    else out[c.id] = i < at ? "passed" : i === at ? "failed" : "not-run";
  }
  return out;
}

const GLYPH: Record<CheckState, string> = { passed: "✓", failed: "✕", "not-run": "○", "not-applicable": "–" };
const CHECK_WORD: Record<CheckState, string> = { passed: "passed", failed: "failed", "not-run": "not checked yet", "not-applicable": "not applicable: no round result given" };
const CHECK_COLOR: Record<CheckState, string> = { passed: "var(--ok)", failed: "var(--danger)", "not-run": "var(--tx-3)", "not-applicable": "var(--tx-3)" };

export type Verdict = "ACCEPT" | "REJECT" | "Not comparable" | "Refused" | "Scorecards needed";

const NOT_COMPARABLE = /different model|different datasets|not a base run|not stamped with this adapter|give both the base and the candidate/;

export function verdictOf(record: FlAdapterGateRecord | null, refusal: string | null): Verdict {
  if (refusal) return "Refused";
  if (!record) return "Scorecards needed";
  if (record.decision.verdict === "ACCEPT") return "ACCEPT";
  return record.decision.reasons.some((r) => NOT_COMPARABLE.test(r)) ? "Not comparable" : "REJECT";
}

const VERDICT_TONE: Record<Verdict, Tone> = { ACCEPT: "ok", REJECT: "danger", "Not comparable": "warn", Refused: "danger", "Scorecards needed": "neutral" };

/** One gate table row: the change, and whether the metric got worse (core's own reason says so). */
export function metricRow(rec: FlAdapterGateRecord, i: number): { metric: string; base: string; cand: string; change: string; result: "got worse" | "improved" | "no change" | "missing" } {
  const m = rec.decision.metrics[i];
  const f = (v: number | null) => (v === null ? "—" : v.toFixed(3));
  const worse = rec.decision.reasons.some((r) => r.startsWith(`${m.metric} got worse`));
  const missing = rec.decision.reasons.some((r) => r.startsWith(`${m.metric} is missing`));
  const change = m.improvement === null ? "—" : (m.improvement >= 0 ? "+" : "") + m.improvement.toFixed(3);
  const result = missing ? "missing" : worse ? "got worse" : m.improvement !== null && m.improvement > 0 ? "improved" : "no change";
  return { metric: m.metric, base: f(m.base), cand: f(m.candidate), change, result };
}

const RESULT_GLYPH = { "got worse": "▼", improved: "▲", "no change": "=", missing: "—" } as const;

export function gateSummaryLine(rec: FlAdapterGateRecord, v: Verdict): string {
  const d = rec.decision;
  const delta = d.compositeCandidate - d.compositeBase;
  if (v === "ACCEPT") return `ACCEPT: no metric got worse and the mean of shared metrics rose by ${delta.toFixed(3)}.`;
  if (v === "Not comparable") return `Not comparable: ${d.reasons.filter((r) => NOT_COMPARABLE.test(r)).join("; ")}. Re-run both with the same model, dataset version and sample size.`;
  return `REJECT: ${d.reasons.join("; ")}. The adapter is not applied.`;
}

type Fields = { adapter: string; sha: string; round: string; baseTools: string; candTools: string; baseQa: string; candQa: string };
const EMPTY: Fields = { adapter: "", sha: "", round: "", baseTools: "", candTools: "", baseQa: "", candQa: "" };

export function gateRequest(f: Fields): FlAdapterGateRequest {
  return {
    adapterPath: f.adapter.trim(),
    expectedSha256: f.sha.trim(),
    baseToolsPath: f.baseTools.trim(),
    candidateToolsPath: f.candTools.trim(),
    ...(f.baseQa.trim() ? { baseQaPath: f.baseQa.trim() } : {}),
    ...(f.candQa.trim() ? { candidateQaPath: f.candQa.trim() } : {}),
    ...(f.round.trim() ? { roundResultPath: f.round.trim() } : {}),
  };
}

async function pickFile(ext: string[]): Promise<string | null> {
  if (BRIDGE_MODE !== "tauri") return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ multiple: false, directory: false, filters: [{ name: ext.join(", "), extensions: ext }] });
  return typeof picked === "string" ? picked : null;
}

export interface AdaptersViewProps {
  fl: FlRoundsDomain;
  toast: (m: string) => void;
  go: (route: string) => void;
  pick?: (ext: string[]) => Promise<string | null>;
}

export function AdaptersView({ fl, toast, go, pick = pickFile }: AdaptersViewProps) {
  const [ov, setOv] = useState<FlOverview | null>(null);
  const [ovErr, setOvErr] = useState<string | null>(null);
  const [from, setFrom] = useState<"round" | "file">("round");
  const [f, setF] = useState<Fields>(EMPTY);
  const [run, setRun] = useState<{ record: FlAdapterGateRecord | null; refusal: string | null; withRound: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [applyErr, setApplyErr] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const o = await fl.overview();
      setOv(o);
      setOvErr(o.storeError);
    } catch (e) {
      setOvErr(e instanceof Error ? e.message : String(e));
    }
  };
  useEffect(() => {
    void refresh();
  }, []);

  const withRound = from === "round";
  const runGate = async () => {
    setBusy(true);
    setApplyErr(null);
    try {
      const rec = await fl.gateAdapter(gateRequest(withRound ? f : { ...f, round: "" }));
      setRun({ record: rec, refusal: null, withRound });
      await refresh();
    } catch (e) {
      setRun({ record: null, refusal: e instanceof Error ? e.message : String(e), withRound });
    } finally {
      setBusy(false);
    }
  };

  const load = async (sha: string) => {
    setApplyErr(null);
    try {
      await fl.loadAdapter(sha);
      toast("Adapter loaded into the local model.");
      await refresh();
    } catch (e) {
      setApplyErr(e instanceof Error ? e.message : String(e));
    }
  };
  const unload = async () => {
    setApplyErr(null);
    try {
      await fl.unloadAdapter();
      toast("Adapter unloaded; the base model serves alone.");
      await refresh();
    } catch (e) {
      setApplyErr(e instanceof Error ? e.message : String(e));
    }
  };

  const rec = run?.record ?? null;
  const verdict = verdictOf(rec, run?.refusal ?? null);
  const checks = checkStates(run);
  const refusedCheck = run?.refusal ? refusalCheck(run.refusal) : null;
  const accepted = verdict === "ACCEPT";
  const field = (k: keyof Fields, label: string, ext: string[], optional = false) => (
    <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
      <span>
        {label}
        {optional ? <span className="fl-faint"> (optional)</span> : null}
      </span>
      <span style={{ display: "flex", gap: 6 }}>
        <input className="input mono" data-testid={"gate-" + k} value={f[k]} onChange={(e) => setF({ ...f, [k]: e.target.value })} style={{ fontSize: 12 }} />
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          aria-label={"Choose " + label}
          onClick={() =>
            void pick(ext).then((p) => {
              if (p) setF((x) => ({ ...x, [k]: p }));
            })
          }
        >
          Choose
        </button>
      </span>
    </label>
  );

  return (
    <div className="fl-cols" data-testid="adapters" style={{ alignItems: "stretch" }}>
      <aside aria-label="Adapter and checks" style={{ flex: "0 0 300px", borderRight: "1px solid var(--line-1)", padding: 16, display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
        <Eyebrow>Adapter</Eyebrow>
        <div role="radiogroup" aria-label="Source" style={{ display: "flex", gap: 4 }}>
          {(
            [
              ["round", "A round result"],
              ["file", "A local file"],
            ] as const
          ).map(([id, label]) => (
            <button key={id} type="button" role="radio" aria-checked={from === id} className={from === id ? "btn btn-sm btn-secondary" : "btn btn-sm btn-ghost"} onClick={() => setFrom(id)}>
              {label}
            </button>
          ))}
        </div>
        {field("adapter", "Adapter file (.gguf)", ["gguf"])}
        {withRound ? field("round", "Round result file (.json)", ["json"]) : null}
        <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
          <span>
            Expected SHA-256{withRound ? <span className="fl-faint"> (optional with a round result)</span> : null}
          </span>
          <input className="input mono" data-testid="gate-sha" value={f.sha} onChange={(e) => setF({ ...f, sha: e.target.value })} style={{ fontSize: 12 }} />
        </label>
        <Eyebrow>Checks the app enforces</Eyebrow>
        <ol aria-label="Checks" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 6 }}>
          {CHECKS.map((c) => (
            <li key={c.id} data-check={c.id} data-state={checks[c.id]} style={{ display: "flex", gap: 8, fontSize: 12.5, alignItems: "baseline" }}>
              <span aria-hidden="true" style={{ color: CHECK_COLOR[checks[c.id]], width: 12 }}>
                {GLYPH[checks[c.id]]}
              </span>
              <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                <span>{c.t}</span>
                <span className="sr-only">{CHECK_WORD[checks[c.id]]}</span>
                {checks[c.id] === "failed" && run?.refusal && <span style={{ color: "var(--danger)", fontSize: 11.5 }}>{run.refusal}</span>}
                {checks[c.id] === "not-applicable" && <span className="fl-faint" style={{ fontSize: 11.5 }}>no round result given</span>}
              </span>
            </li>
          ))}
        </ol>
        <Note>Wording says "the round result says" because core checks the round result file, not FederatedRoundLedger.</Note>
        <Note>Adapter download from LoRAFactory is not built: the gate takes a local file.</Note>
      </aside>

      <section className="fl-main" aria-labelledby="gate-h">
        <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          <h1 id="gate-h" className="fl-disp" style={{ fontSize: 24 }}>
            Eval gate
          </h1>
          <Chip tone={VERDICT_TONE[verdict]}>{verdict}</Chip>
          <Chip>thresholds pending owner sign-off</Chip>
        </div>
        {ovErr && (
          <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--warn)" }}>
            Couldn't read the gate records: {ovErr}
          </p>
        )}
        {run?.refusal && (
          <div role="alert" className="fl-card" data-testid="gate-refused" style={{ padding: "10px 14px", borderColor: "var(--danger)", background: "var(--danger-bg)", fontSize: 13 }}>
            <b style={{ fontWeight: 600 }}>Refused before the gate:</b> {run.refusal}. {refusedCheck ? "" : "No check ran. "}The adapter cannot be loaded.
          </div>
        )}
        <p className="fl-muted" style={{ margin: 0, fontSize: 13, lineHeight: 1.55, maxWidth: 720 }}>
          ACCEPT only if no metric got worse and the mean of the shared metrics strictly improved. Base and candidate must use the same model, dataset version and sample size.
        </p>
        {!rec && (
          <Card testid="scorecards-needed">
            <b style={{ fontWeight: 500, fontSize: 13.5 }}>Scorecards needed</b>
            <span className="fl-muted" style={{ fontSize: 12.5 }}>
              The packaged app cannot run the eval tools yet. Run scripts/eval-tools.mjs (and optionally scripts/eval-qa.mjs) for the base model and for the base model with this adapter (--adapter-sha256), then add both scorecards.
            </span>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 10 }}>
              {field("baseTools", "Base tool-call scorecard", ["json"])}
              {field("candTools", "Candidate tool-call scorecard", ["json"])}
              {field("baseQa", "Base QA scorecard", ["json"], true)}
              {field("candQa", "Candidate QA scorecard", ["json"], true)}
            </div>
            <div>
              <button type="button" className="btn btn-primary" data-testid="gate-run" disabled={busy || !f.adapter.trim() || !f.baseTools.trim() || !f.candTools.trim()} onClick={() => void runGate()}>
                {busy ? "Comparing" : "Run the eval gate"}
              </button>
            </div>
          </Card>
        )}
        {rec && (
          <>
            <table className="fl-tbl fl-card" data-testid="gate-table">
              <thead>
                <tr>
                  <th>Metric</th>
                  <th>Base</th>
                  <th>Candidate</th>
                  <th>Change</th>
                  <th>Result</th>
                </tr>
              </thead>
              <tbody>
                {rec.decision.metrics.map((_, i) => {
                  const r = metricRow(rec, i);
                  return (
                    <tr key={r.metric} data-result={r.result}>
                      <td>{r.metric}</td>
                      <td className="mono">{r.base}</td>
                      <td className="mono">{r.cand}</td>
                      <td className="mono">{r.change}</td>
                      <td style={{ color: r.result === "got worse" ? "var(--danger)" : r.result === "improved" ? "var(--ok)" : "var(--tx-2)" }}>
                        <span aria-hidden="true">{RESULT_GLYPH[r.result]} </span>
                        {r.result}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <div className="fl-inset" style={{ padding: "10px 12px", fontSize: 13 }}>
              {gateSummaryLine(rec, verdict)}
            </div>
            <KV
              rows={[
                { k: "Round", v: rec.round ? `the round result says Accepted: round ${shortHex(rec.round.roundId)}, ${rec.round.participants} devices, chain ${rec.round.chainId}` : "No round result was given, so this adapter is not tied to a round." },
                { k: "Candidate stamped with", v: shortHex(rec.adapterSha256, 8, 6), mono: true },
                { k: "Base model", v: rec.baseModel },
                { k: "Gate record", v: "kept on this device, keyed by adapter hash; the latest decision wins" },
              ]}
            />
            <div>
              <button type="button" className="btn btn-ghost btn-sm" onClick={() => setRun(null)}>
                Run again
              </button>
            </div>
          </>
        )}
      </section>

      <aside className="fl-rail" aria-label="Apply">
        <Eyebrow>On your model</Eyebrow>
        <ApplyCard ov={ov} />
        {applyErr && (
          <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--danger)" }}>
            {applyErr}
          </p>
        )}
        {ov?.activeAdapter ? (
          <button type="button" className="btn btn-ghost" onClick={() => void unload()}>
            Unload
          </button>
        ) : (
          <>
            <button type="button" className="btn btn-primary" data-testid="gate-load" disabled={!accepted || !rec} onClick={() => rec && void load(rec.adapterSha256)}>
              <HicBadge level="HIC-1" style={{ background: "var(--on-accent)", color: "var(--accent)" }} />
              Load adapter
            </button>
            <Note>{accepted ? "You approve the load; the local model restarts with the adapter." : "Load is enabled only on ACCEPT with every check passing."}</Note>
          </>
        )}
        <Note>Re-applied after a restart when the eval gate still accepts it on the same base model (on by default, pending owner sign-off). A failed re-apply never blocks the model start.</Note>
        <button type="button" className="btn btn-sm btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => go("groups/rounds/live")}>
          Back to the live round
        </button>
      </aside>
    </div>
  );
}

function ApplyCard({ ov }: { ov: FlOverview | null }) {
  if (!ov) return <Card raised><Unknown reason="reading the adapter state" /></Card>;
  if (ov.restoreError)
    return (
      <Card raised testid="apply-state">
        <b style={{ fontWeight: 500 }}>Re-apply failed</b>
        <span className="fl-muted" style={{ fontSize: 12.5 }}>
          The model started without the adapter: {ov.restoreError}
        </span>
      </Card>
    );
  if (ov.activeAdapter)
    return (
      <Card raised testid="apply-state">
        <b style={{ fontWeight: 500 }}>Loaded{ov.rememberedAdapter ? ` on ${ov.rememberedAdapter.baseModel}` : ""}</b>
        <span className="fl-muted mono" style={{ fontSize: 12 }}>
          {ov.activeAdapter}
        </span>
        {ov.rememberedAdapter && <span className="fl-muted" style={{ fontSize: 12 }}>Put back after a restart while the gate still accepts it.</span>}
      </Card>
    );
  return (
    <Card raised testid="apply-state">
      <b style={{ fontWeight: 500 }}>Not loaded</b>
      <span className="fl-muted" style={{ fontSize: 12.5 }}>Your model runs without an adapter.</span>
    </Card>
  );
}
