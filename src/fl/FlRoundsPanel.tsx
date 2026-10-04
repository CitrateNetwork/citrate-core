// =====================================================================
// citrate-core: federated rounds panel (HUP-S9.4), on the Train surface
//
// Coordinator setting, plan + plain-words explanation, HIC-1 start, and the LoRA eval gate.
// n5: get a round's adapter (its FL_ROUND_V1 bundle + merged adapter, from files or https), run
// the eval in the app on the local model, and an accepted adapter that was loaded comes back
// after a restart (core checks it again first).
// Everything shown comes from core (fl_rounds.rs). Without a configured coordinator it says so;
// a start records the member's approval and says plainly that no training runs in this build.
// =====================================================================
import { useEffect, useState } from "react";
import type { FlAdapterGateRecord, FlOverview, FlRoundPlan, FlRoundProvenance, FlRoundsDomain, FlStartReceipt } from "../bridge/domains";
import type { CerSpec } from "../shell/state";
import { approveAndStartRound, gateSummary, roundSummary } from "./flRounds";
import { runInAppEval, type EvalProgress } from "./flEval";

export interface FlRoundsPanelProps {
  fl: FlRoundsDomain;
  requestSig: (spec: CerSpec) => Promise<string>;
  toast: (msg: string) => void;
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

const label: React.CSSProperties = { fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" };
const para: React.CSSProperties = { fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.6, margin: 0 };

export function FlRoundsPanel({ fl, requestSig, toast }: FlRoundsPanelProps) {
  const [ov, setOv] = useState<FlOverview | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [coordInput, setCoordInput] = useState("");
  const [plan, setPlan] = useState<FlRoundPlan | null>(null);
  const [receipt, setReceipt] = useState<FlStartReceipt | null>(null);
  const [busy, setBusy] = useState(false);
  const [gate, setGate] = useState({ adapter: "", sha: "", baseTools: "", candTools: "", baseQa: "", candQa: "" });
  const [gateRec, setGateRec] = useState<FlAdapterGateRecord | null>(null);
  const [round, setRound] = useState({ bundle: "", adapter: "" });
  const [roundRec, setRoundRec] = useState<FlRoundProvenance | null>(null);
  const [adapterUrl, setAdapterUrl] = useState("");
  const [evalProgress, setEvalProgress] = useState<EvalProgress | null>(null);

  const refresh = async () => {
    try {
      const o = await fl.overview();
      setOv(o);
      setErr(o.storeError);
    } catch (e) {
      setErr(msg(e));
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const saveCoordinator = async () => {
    setErr(null);
    try {
      await fl.setCoordinator(coordInput.trim() === "" ? null : coordInput.trim());
      toast("Coordinator saved.");
      await refresh();
    } catch (e) {
      setErr(msg(e));
    }
  };

  const doPlan = async () => {
    setErr(null);
    setReceipt(null);
    setBusy(true);
    try {
      setPlan(await fl.plan());
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  const doStart = async () => {
    if (!plan) return;
    setBusy(true);
    try {
      const out = await approveAndStartRound({ requestSig, start: (h) => fl.start(h) }, plan, "Train surface");
      if (out.receipt) {
        setReceipt(out.receipt);
        await refresh();
      } else if (out.status !== "declined") {
        setErr(out.message);
      }
    } finally {
      setBusy(false);
    }
  };

  const runGate = async () => {
    setErr(null);
    setGateRec(null);
    setBusy(true);
    try {
      const rec = await fl.gateAdapter({
        adapterPath: gate.adapter.trim(),
        expectedSha256: gate.sha.trim(),
        baseToolsPath: gate.baseTools.trim(),
        candidateToolsPath: gate.candTools.trim(),
        ...(gate.baseQa.trim() ? { baseQaPath: gate.baseQa.trim() } : {}),
        ...(gate.candQa.trim() ? { candidateQaPath: gate.candQa.trim() } : {}),
      });
      setGateRec(rec);
      await refresh();
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  const isUrl = (v: string) => /^https?:\/\//i.test(v.trim());

  const getRound = async () => {
    setErr(null);
    setRoundRec(null);
    const b = round.bundle.trim();
    const a = round.adapter.trim();
    if (!b || !a) {
      setErr("Give both the round bundle and the round's adapter (two file paths, or two https links).");
      return;
    }
    if (isUrl(b) !== isUrl(a)) {
      setErr("Give two file paths or two links, not one of each.");
      return;
    }
    setBusy(true);
    try {
      const rec = isUrl(b) ? await fl.fetchRound(b, a) : await fl.importRound(b, a);
      setRoundRec(rec);
      setGate((g) => ({ ...g, adapter: rec.adapterPath, sha: rec.adapterSha256 }));
      await refresh();
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  const getAdapter = async () => {
    setErr(null);
    setBusy(true);
    try {
      const path = await fl.fetchAdapter(adapterUrl.trim(), gate.sha.trim());
      setGate((g) => ({ ...g, adapter: path }));
      toast("Adapter downloaded and its sha256 checked.");
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  const runEvalInApp = async () => {
    setErr(null);
    setGateRec(null);
    setBusy(true);
    setEvalProgress(null);
    try {
      const rec = await runInAppEval(fl, gate.adapter.trim(), gate.sha.trim(), setEvalProgress);
      setGateRec(rec);
      await refresh();
    } catch (e) {
      // Refresh first (the run in core is over), then show why: refresh resets the error line.
      await refresh();
      setErr(msg(e));
    } finally {
      setBusy(false);
      setEvalProgress(null);
    }
  };

  const load = async (sha: string) => {
    setErr(null);
    try {
      await fl.loadAdapter(sha);
      toast("Adapter loaded into the local model.");
      await refresh();
    } catch (e) {
      setErr(msg(e));
    }
  };

  const unload = async () => {
    setErr(null);
    try {
      await fl.unloadAdapter();
      toast("Adapter unloaded; the base model serves alone.");
      await refresh();
    } catch (e) {
      setErr(msg(e));
    }
  };

  const cfg = ov?.config;
  const field = (key: keyof typeof gate, testid: string, placeholder: string) => (
    <input
      className="input"
      data-testid={testid}
      value={gate[key]}
      placeholder={placeholder}
      onInput={(e) => setGate((g) => ({ ...g, [key]: (e.target as HTMLInputElement).value }))}
      onChange={(e) => setGate((g) => ({ ...g, [key]: e.target.value }))}
    />
  );
  const summary = gateRec ? gateSummary(gateRec) : null;
  const roundLines = roundRec ? roundSummary(roundRec) : null;

  return (
    <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Federated rounds with Hermes</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>you approve every start · adapters load only after the eval gate</span>
      </div>
      <div style={{ padding: 18, display: "flex", flexDirection: "column", gap: 16 }}>
        {err && (
          <p className="mono" data-testid="fl-error" style={{ fontSize: 11, color: "var(--warn)", margin: 0, lineHeight: 1.6 }}>
            {err}
          </p>
        )}

        <div data-testid="fl-coordinator" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span className="mono" style={label}>Coordinator</span>
          {cfg?.url ? (
            <p style={para}>
              Reading <span className="mono">{cfg.url}</span>
              {cfg.source === "env" ? " (set by the operator)" : ""}.{cfg.note ? " " + cfg.note + "." : ""}
            </p>
          ) : (
            <p style={para}>
              No training coordinator is configured. Live rounds need one: an operator runs it (HUP-S9.1/S9.2), then you enter its address here.
              {cfg?.note ? " " + cfg.note + "." : ""}
            </p>
          )}
          <div style={{ display: "flex", gap: 8 }}>
            <input
              className="input"
              data-testid="fl-coordinator-input"
              value={coordInput}
              placeholder="https://coordinator.example.org"
              onInput={(e) => setCoordInput((e.target as HTMLInputElement).value)}
              onChange={(e) => setCoordInput(e.target.value)}
              style={{ flex: 1 }}
            />
            <button className="btn btn-secondary" data-testid="fl-coordinator-save" onClick={() => void saveCoordinator()}>
              Save
            </button>
          </div>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ display: "flex", gap: 10 }}>
            <button className="btn btn-secondary" data-testid="fl-plan" onClick={() => void doPlan()} disabled={busy}>
              Plan a round
            </button>
            <button className="btn btn-primary" data-testid="fl-start" onClick={() => void doStart()} disabled={busy || !plan || !plan.canStart}>
              Join this round
            </button>
          </div>
          {plan && (
            <div data-testid="fl-plan-explain" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <p style={para}>{plan.explain.status}</p>
              {(
                [
                  ["Data", plan.explain.data],
                  ["Compute", plan.explain.compute],
                  ["Reward", plan.explain.reward],
                  ["Privacy", plan.explain.privacy],
                ] as const
              ).map(([k, v]) => (
                <p key={k} style={para}>
                  <strong>{k}.</strong> {v}
                </p>
              ))}
              <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0 }}>plan {plan.planHash.slice(0, 16)}…</p>
            </div>
          )}
          {plan && !plan.canStart && (
            <ul data-testid="fl-blockers" style={{ ...para, paddingLeft: 18 }}>
              {plan.blockers.map((b) => (
                <li key={b}>{b}</li>
              ))}
            </ul>
          )}
          {receipt && (
            <p data-testid="fl-receipt" style={para}>
              {receipt.note}
            </p>
          )}
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span className="mono" style={label}>A round's adapter</span>
          <p style={para}>
            A finished round publishes its bundle (who took part and how the adapter was built) and the merged adapter. Give both, as files or as https links: the app checks the adapter
            against the bundle and against the model you serve. Looking the adapter up in LoRAFactory is not deployed yet, and the round's on-chain record is not checked by this build.
          </p>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              className="input"
              data-testid="fl-round-bundle"
              value={round.bundle}
              placeholder="round bundle .json (path or https link)"
              onInput={(e) => setRound((r) => ({ ...r, bundle: (e.target as HTMLInputElement).value }))}
              onChange={(e) => setRound((r) => ({ ...r, bundle: e.target.value }))}
              style={{ flex: 1 }}
            />
            <input
              className="input"
              data-testid="fl-round-adapter"
              value={round.adapter}
              placeholder="merged adapter .gguf (path or https link)"
              onInput={(e) => setRound((r) => ({ ...r, adapter: (e.target as HTMLInputElement).value }))}
              onChange={(e) => setRound((r) => ({ ...r, adapter: e.target.value }))}
              style={{ flex: 1 }}
            />
            <button className="btn btn-secondary" data-testid="fl-round-get" onClick={() => void getRound()} disabled={busy}>
              Check round
            </button>
          </div>
          {roundRec && roundLines && (
            <div data-testid="fl-round-summary" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              {roundLines.map((l) => (
                <span key={l} style={{ ...para, fontSize: 11.5 }}>
                  {l}
                </span>
              ))}
            </div>
          )}
          <div style={{ display: "flex", gap: 8 }}>
            <input
              className="input"
              data-testid="fl-adapter-url"
              value={adapterUrl}
              placeholder="or only an adapter link (https), with its published sha256 below"
              onInput={(e) => setAdapterUrl((e.target as HTMLInputElement).value)}
              onChange={(e) => setAdapterUrl(e.target.value)}
              style={{ flex: 1 }}
            />
            <button className="btn btn-secondary" data-testid="fl-adapter-fetch" onClick={() => void getAdapter()} disabled={busy || !adapterUrl.trim()}>
              Download
            </button>
          </div>
          <span className="mono" style={label}>The eval gate</span>
          <p style={para}>
            Run the eval in the app: the local model answers the tool-call and injection sets once without the adapter and once with it (your chats keep using the model without it while
            this runs), and the adapter can load only if nothing got worse and the score improved. Or give scorecards from the eval CLIs (scripts/eval-tools.mjs, optionally
            scripts/eval-qa.mjs, with --model set to the base model's file name and --adapter-sha256).
          </p>
          {field("adapter", "fl-gate-adapter", "adapter .gguf path")}
          {field("sha", "fl-gate-sha", "expected sha256")}
          {field("baseTools", "fl-gate-base-tools", "base tool-call scorecard .json")}
          {field("candTools", "fl-gate-cand-tools", "candidate tool-call scorecard .json")}
          {field("baseQa", "fl-gate-base-qa", "base QA scorecard .json (optional)")}
          {field("candQa", "fl-gate-cand-qa", "candidate QA scorecard .json (optional)")}
          <div style={{ display: "flex", gap: 10 }}>
            <button className="btn btn-primary" data-testid="fl-eval-run" onClick={() => void runEvalInApp()} disabled={busy || !gate.adapter.trim() || !gate.sha.trim()}>
              Run the eval in the app
            </button>
            <button className="btn btn-secondary" data-testid="fl-gate-run" onClick={() => void runGate()} disabled={busy}>
              Use these scorecards
            </button>
          </div>
          {evalProgress && (
            <p data-testid="fl-eval-progress" role="status" style={para}>
              {evalProgress.arm === "base" ? "Without the adapter" : "With the adapter"}: {evalProgress.done} of {evalProgress.total}
            </p>
          )}
          {gateRec && summary && (
            <div data-testid="fl-gate-verdict" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={{ fontSize: 13, fontWeight: 500, color: gateRec.decision.verdict === "ACCEPT" ? "var(--ok)" : "var(--warn)" }}>{summary.headline}</span>
              {summary.lines.map((l) => (
                <span key={l} className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>
                  {l}
                </span>
              ))}
              {gateRec.decision.verdict === "ACCEPT" && (
                <div>
                  <button className="btn btn-primary" data-testid="fl-gate-load" onClick={() => void load(gateRec.adapterSha256)}>
                    Load this adapter
                  </button>
                </div>
              )}
            </div>
          )}
          {ov?.activeAdapter && (
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span data-testid="fl-active-adapter" className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>
                Loaded adapter: {ov.activeAdapter}
                {ov.remembered ? " (comes back after a restart, checked again first)" : ""}
              </span>
              <button className="btn btn-secondary" data-testid="fl-unload" onClick={() => void unload()}>
                Unload
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
