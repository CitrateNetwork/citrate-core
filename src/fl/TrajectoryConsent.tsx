// =====================================================================
// citrate-core: "Train on my verified conversations" (HUP-S9.3), inside the federated rounds panel
//
// The member's switch for letting verified, redacted Hermes conversations feed a training round.
// Off by default. Everything shown comes from core (fl_trajectories.rs): with the switch off,
// Hermes records nothing and core refuses to assemble a training set. Taking part in a round still
// needs that round's own consent (D-29); this switch alone shares nothing.
// =====================================================================
import { useEffect, useState } from "react";
import type { FlRoundsDomain, FlTrainingSet, FlTrajectoryStatus } from "../bridge/domains";
import { DEFAULT_PROPOSAL } from "./flRounds";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const label: React.CSSProperties = { fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" };
const para: React.CSSProperties = { fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.6, margin: 0 };

export function TrajectoryConsent({ fl, toast }: { fl: FlRoundsDomain; toast: (m: string) => void }) {
  const [st, setSt] = useState<FlTrajectoryStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [deleteRecorded, setDeleteRecorded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [built, setBuilt] = useState<FlTrainingSet | null>(null);

  useEffect(() => {
    fl.trajectoryStatus().then(
      (s) => {
        setSt(s);
        setErr(s.loadError);
      },
      (e) => setErr(msg(e)),
    );
  }, []);

  const on = st?.settings.enabled === true;

  const toggle = async () => {
    setBusy(true);
    setErr(null);
    setBuilt(null);
    try {
      const next = await fl.setTrajectoryConsent(!on, on ? deleteRecorded : false);
      setSt(next);
      toast(
        next.settings.enabled
          ? "Training on verified conversations is on. Hermes records them from its next start."
          : "Training on verified conversations is off. Assembled training sets were deleted.",
      );
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  const build = async () => {
    setBusy(true);
    setErr(null);
    try {
      setBuilt(await fl.buildTrainingSet(DEFAULT_PROPOSAL.maxTrajectories));
      setSt(await fl.trajectoryStatus());
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-testid="fl-trajectories" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <span className="mono" style={label}>Training data</span>
      <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5 }}>
        <input
          type="checkbox"
          data-testid="fl-traj-toggle"
          checked={on}
          disabled={busy || st === null}
          onChange={() => void toggle()}
        />
        Train on my verified conversations
      </label>
      <p style={para} data-testid="fl-traj-explain">
        {on
          ? "On. Hermes keeps a copy of each conversation whose workflow checks all passed, redacted on this device (keys, tokens, seed phrases, emails, wallet addresses and private paths removed). Conversations that read untrusted pages are left out. Nothing leaves this device unless you also join a specific round."
          : "Off (the default). Hermes records nothing for training, and no training set can be built. Turning it on lets verified, redacted conversations feed a round you join; each round still asks for its own consent."}
        {st?.appliesOnRestart ? " Changes to recording apply the next time Hermes starts." : ""}
      </p>
      {st && (
        <p className="mono" style={{ ...para, fontSize: 11 }} data-testid="fl-traj-counts">
          {st.recordedFiles} recorded session file{st.recordedFiles === 1 ? "" : "s"} · {st.datasets} training set{st.datasets === 1 ? "" : "s"}
        </p>
      )}
      {on && (
        <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: "var(--tx-3)" }}>
          <input type="checkbox" data-testid="fl-traj-delete" checked={deleteRecorded} onChange={(e) => setDeleteRecorded(e.target.checked)} />
          When I turn this off, also delete the recorded conversations
        </label>
      )}
      {on && (
        <div style={{ display: "flex", gap: 8 }}>
          <button className="btn btn-secondary" data-testid="fl-traj-build" disabled={busy} onClick={() => void build()}>
            Build training set
          </button>
        </div>
      )}
      {built && (
        <p className="mono" style={{ ...para, fontSize: 11 }} data-testid="fl-traj-built">
          {built.examples} verified conversation{built.examples === 1 ? "" : "s"} in {built.path} (sha256 {built.sha256.slice(0, 12)})
          {built.skippedLines > 0 ? ` · ${built.skippedLines} unusable line${built.skippedLines === 1 ? "" : "s"} left out` : ""}
          {built.overCap > 0 ? ` · ${built.overCap} over the cap left out` : ""}
        </p>
      )}
      {err && (
        <p className="mono" data-testid="fl-traj-error" style={{ fontSize: 11, color: "var(--warn)", margin: 0, lineHeight: 1.6 }}>
          {err}
        </p>
      )}
    </div>
  );
}
