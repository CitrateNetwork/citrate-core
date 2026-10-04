// =====================================================================
// citrate-core: federated rounds panel (HUP-S9.4), on the Train surface
//
// Coordinator setting, plan + plain-words explanation, HIC-1 start, and the LoRA eval gate.
// Everything shown comes from core (fl_rounds.rs). Without a configured coordinator it says so;
// a start records the member's approval and says plainly that no training runs in this build.
// A plan may name a round (FL_ROUND_V1 round_id): its start writes per-round consent for the
// device worker, listed here with Withdraw. The gate can bind an adapter to its round result, and
// a loaded adapter is put back after a restart.
// =====================================================================
import { useEffect, useState } from "react";
import type { FlAdapterGateRecord, FlOverview, FlRoundPlan, FlRoundsDomain, FlStartReceipt } from "../bridge/domains";
import type { CerSpec } from "../shell/state";
import { approveAndStartRound, DEFAULT_PROPOSAL, gateSummary } from "./flRounds";

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
  const [roundInput, setRoundInput] = useState("");
  const [plan, setPlan] = useState<FlRoundPlan | null>(null);
  const [receipt, setReceipt] = useState<FlStartReceipt | null>(null);
  const [busy, setBusy] = useState(false);
  const [gate, setGate] = useState({ adapter: "", sha: "", round: "", baseTools: "", candTools: "", baseQa: "", candQa: "" });
  const [gateRec, setGateRec] = useState<FlAdapterGateRecord | null>(null);

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
      const roundId = roundInput.trim();
      setPlan(await fl.plan(roundId ? { ...DEFAULT_PROPOSAL, roundId } : undefined));
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
        ...(gate.round.trim() ? { roundResultPath: gate.round.trim() } : {}),
      });
      setGateRec(rec);
      await refresh();
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
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

  const revoke = async (roundId: string) => {
    setErr(null);
    try {
      await fl.revokeConsent(roundId);
      toast("Consent withdrawn for that round.");
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
          <input
            className="input"
            data-testid="fl-round-id"
            value={roundInput}
            placeholder="round id (0x…, optional, as the operator published it)"
            onInput={(e) => setRoundInput((e.target as HTMLInputElement).value)}
            onChange={(e) => setRoundInput(e.target.value)}
          />
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

        <div data-testid="fl-consents" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <span className="mono" style={label}>Round consent</span>
          {ov?.consentError ? (
            <p style={{ ...para, color: "var(--warn)" }}>{ov.consentError}</p>
          ) : ov && ov.consentedRounds.length > 0 ? (
            <>
              <p style={para}>
                This device consented to the rounds below. A device training worker started with CITRATE_FL_CONSENT_FILE set to{" "}
                <span className="mono">{ov.consentFile}</span> takes part in these rounds only.
              </p>
              {ov.consentedRounds.map((r, i) => (
                <div key={r} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)", wordBreak: "break-all" }}>
                    {r}
                  </span>
                  <button className="btn btn-secondary" data-testid={`fl-consent-revoke-${i}`} onClick={() => void revoke(r)}>
                    Withdraw
                  </button>
                </div>
              ))}
            </>
          ) : (
            <p style={para}>No round consent on this device. Joining a plan that names a round writes consent for that round only.</p>
          )}
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span className="mono" style={label}>Round adapter and the eval gate</span>
          <p style={para}>
            Downloading a round's adapter from LoRAFactory is not deployed yet. Give the adapter file and its published sha256 (or the round result the round tool wrote, which names the merged adapter; core checks that the file says Accepted, names at least three devices, and that its chain, bundle and replay digests agree, but does not read the ledger itself), plus the eval scorecards for the base model and for the
            base model with this adapter (scripts/eval-tools.mjs, optionally scripts/eval-qa.mjs, with --model set to the base model's file name and --adapter-sha256). The adapter loads only if nothing got worse and the score improved.
          </p>
          {field("adapter", "fl-gate-adapter", "adapter .gguf path")}
          {field("round", "fl-gate-round", "round result .json from the round tool (optional)")}
          {field("sha", "fl-gate-sha", "expected sha256 (may be empty with a round result)")}
          {field("baseTools", "fl-gate-base-tools", "base tool-call scorecard .json")}
          {field("candTools", "fl-gate-cand-tools", "candidate tool-call scorecard .json")}
          {field("baseQa", "fl-gate-base-qa", "base QA scorecard .json (optional)")}
          {field("candQa", "fl-gate-cand-qa", "candidate QA scorecard .json (optional)")}
          <div>
            <button className="btn btn-secondary" data-testid="fl-gate-run" onClick={() => void runGate()} disabled={busy}>
              Run the eval gate
            </button>
          </div>
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
          {ov?.rememberedAdapter && (
            <p data-testid="fl-remembered" style={para}>
              {ov.restoreError
                ? ov.restoreError
                : `This adapter is put back when the app starts the local model on ${ov.rememberedAdapter.baseModel}, if the eval gate still accepts it there. Unload to stop that.`}
            </p>
          )}
          {ov?.activeAdapter && (
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span data-testid="fl-active-adapter" className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>
                Loaded adapter: {ov.activeAdapter}
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
