// 0.5.4 FL design, board 04: "Add a machine", on both devices.
//
// This device ("add"): core mints a short-lived, single-use pairing link (`fleet_pair_create`) shown
// as a QR code and text. While the sheet is open the paired-machines roster (`fleet_roster`) is
// re-read, and a machine that joins appears with the six-digit code both screens show. The other
// device ("join"): the member pastes the link, core reads it (`fleet_pair_inspect`, sends nothing),
// and only "Pair with this machine" joins (`fleet_pair_join`); every outcome is said in plain words.
// A group link (`citrate-cluster://…`, from `cluster_group_seed`) pasted on the join side dials the
// group's machines instead (`cluster_add_group_seed`); the roster still decides who is admitted.
// The sheet is a real modal: focus starts on Close (the safe choice) and Escape closes it.
import { useEffect, useRef, useState } from "react";
import { ModalDialog } from "../shell/ModalDialog";
import { QrCode } from "../fleet/FleetWizard";
import { expiresIn, roleLabel } from "../fleet/wizard";
import type { FleetApi, FleetDevice, PairClaim, PairOffer } from "../bridge/tauri/fleet";
import type { ClusterGroupSeed } from "../bridge/domains";
import { isGroupSeed, joinOutcome, newlyPaired } from "./clusterBoard";

/** How often the roster is re-read while this device waits for the other machine. */
export const ROSTER_POLL_MS = 3000;

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e ?? ""));
const note = { fontSize: 12, color: "var(--tx-3)", lineHeight: 1.55, margin: 0 } as const;

export interface AddMachineSheetProps {
  mode: "add" | "join";
  api: FleetApi;
  groupLabel: string | null;
  /** This group's seed link (needs the cross-machine mesh); omitted when no group is selected. */
  groupSeed?: () => Promise<ClusterGroupSeed>;
  /** Dial the machines in a pasted group link; resolves to how many addresses are dialed. */
  addGroupSeed?: (seed: string) => Promise<number>;
  onClose: () => void;
  /** A machine paired (either side): the board re-reads its roster. */
  onPaired?: () => void;
  /** Seconds clock (tests pin it). */
  nowSecs?: () => number;
}

export function AddMachineSheet(p: AddMachineSheetProps) {
  const title = p.mode === "add" ? `Add a machine${p.groupLabel ? ` to ${p.groupLabel}` : ""}` : "Join with a link";
  return (
    <ModalDialog
      labelledBy="add-machine-h"
      onEscape={p.onClose}
      focusKey={p.mode}
      register="charter"
      zIndex={60}
      panelStyle={{ background: "var(--srf-1)", color: "var(--tx-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", width: "min(640px, 100%)", maxHeight: "90vh", overflow: "auto", padding: "18px 20px", display: "flex", flexDirection: "column", gap: 14 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <h2 id="add-machine-h" style={{ margin: 0, fontSize: 18, fontWeight: 500 }}>{title}</h2>
        <span style={{ flex: 1 }} />
        <button type="button" className="btn btn-ghost btn-sm" data-autofocus onClick={p.onClose}>Close</button>
      </div>
      {p.mode === "add" ? <AddSide {...p} /> : <JoinSide {...p} />}
    </ModalDialog>
  );
}

function AddSide({ api, groupSeed, onPaired, nowSecs = () => Date.now() / 1000 }: AddMachineSheetProps) {
  const [offer, setOffer] = useState<PairOffer | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [joined, setJoined] = useState<FleetDevice[]>([]);
  const [copied, setCopied] = useState(false);
  const [seed, setSeed] = useState<ClusterGroupSeed | null>(null);
  const [seedErr, setSeedErr] = useState<string | null>(null);
  const before = useRef<Set<string> | null>(null);
  const [, tick] = useState(0);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        before.current = new Set((await api.roster()).map((d) => d.id));
      } catch {
        before.current = new Set();
      }
      try {
        const o = await api.createLink();
        if (live) setOffer(o);
      } catch (e) {
        if (live) setErr(msg(e));
      }
    })();
    return () => {
      live = false;
    };
  }, [api]);

  // Waiting for the other machine: re-read the roster; a new machine is the "joined" outcome.
  useEffect(() => {
    if (!offer) return;
    const t = setInterval(() => {
      tick((n) => n + 1);
      void api
        .roster()
        .then((now) => {
          const fresh = before.current ? newlyPaired(before.current, now) : [];
          if (fresh.length) {
            setJoined(fresh);
            onPaired?.();
          }
        })
        .catch(() => {});
    }, ROSTER_POLL_MS);
    return () => clearInterval(t);
  }, [offer, api, onPaired]);

  return (
    <>
      {err && <div role="alert" style={{ fontSize: 12.5, color: "var(--danger)", lineHeight: 1.5 }}>Couldn't make a pairing link: {err}</div>}
      {!offer && !err && <p role="status" style={note}>Making a pairing link…</p>}
      {offer && (
        <div style={{ display: "flex", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <QrCode qr={offer.qr} px={3} testId="add-machine-qr" label="QR code with the pairing link" />
          <div style={{ flex: "1 1 260px", minWidth: 0, display: "flex", flexDirection: "column", gap: 10 }}>
            <ol style={{ margin: 0, paddingLeft: 18, fontSize: 13, lineHeight: 1.6 }}>
              <li>On the other machine, open Citrate Core › Groups › Cluster.</li>
              <li>Choose Join with a link and scan this code, or paste the link.</li>
              <li>Pair on the other machine. Both machines then show the same six-digit code; check they match.</li>
            </ol>
            <div className="mono" data-testid="add-machine-link" style={{ fontSize: 11, wordBreak: "break-all", background: "var(--srf-inset)", padding: "6px 8px", borderRadius: "var(--r-1)" }}>{offer.link}</div>
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <button
                type="button"
                className="btn btn-sm"
                onClick={async () => {
                  try {
                    await navigator.clipboard?.writeText(offer.link);
                    setCopied(true);
                  } catch {
                    setCopied(false);
                  }
                }}
              >
                {copied ? "Link copied" : "Copy link"}
              </button>
              <span style={note}>Single use, {expiresIn(offer.expiresAt, nowSecs())}.</span>
            </div>
          </div>
        </div>
      )}
      {offer && joined.length === 0 && (
        <p role="status" data-testid="add-machine-waiting" style={note}>Waiting for the other machine. It appears here when it pairs.</p>
      )}
      {joined.map((d) => (
        <div key={d.id} role="status" data-testid="add-machine-joined" className="surface" style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 4 }}>
          <b style={{ fontWeight: 500 }}>{d.label} joined</b>
          <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>
            {d.tier ? `Tier ${d.tier}` : "Tier not reported"} · {roleLabel(d.role)}
            {d.code ? ` · code ${d.code}` : ""}
          </span>
        </div>
      ))}
      {groupSeed && (
        <details>
          <summary style={{ fontSize: 12.5, cursor: "pointer" }}>A member's machine, not yours: share this group's link</summary>
          <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingTop: 8 }}>
            <p style={note}>The group link says where to reach this machine. Each machine is still admitted only by the group's roster.</p>
            {!seed && (
              <span>
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={async () => {
                    setSeedErr(null);
                    try {
                      setSeed(await groupSeed());
                    } catch (e) {
                      setSeedErr(msg(e));
                    }
                  }}
                >
                  Show this group's link
                </button>
              </span>
            )}
            {seed && <div className="mono" data-testid="group-seed-link" style={{ fontSize: 11, wordBreak: "break-all" }}>{seed.link}</div>}
            {seedErr && <div role="alert" style={{ fontSize: 12, color: "var(--danger)" }}>{seedErr}</div>}
          </div>
        </details>
      )}
    </>
  );
}

type JoinStage =
  | { kind: "idle" }
  | { kind: "reading" }
  | { kind: "read"; claim: PairClaim }
  | { kind: "pairing"; claim: PairClaim }
  | { kind: "done"; ok: boolean; text: string };

function JoinSide({ api, addGroupSeed, onPaired, nowSecs = () => Date.now() / 1000 }: AddMachineSheetProps) {
  const [link, setLink] = useState("");
  const [stage, setStage] = useState<JoinStage>({ kind: "idle" });
  const [err, setErr] = useState<string | null>(null);

  const read = async () => {
    const l = link.trim();
    if (!l) return;
    setErr(null);
    if (isGroupSeed(l)) {
      if (!addGroupSeed) {
        setErr("Pick a group first: a group link joins that group's machines.");
        return;
      }
      setStage({ kind: "reading" });
      try {
        const n = await addGroupSeed(l);
        setStage({ kind: "done", ok: true, text: `Reaching ${n} address${n === 1 ? "" : "es"} from the group link. Each machine is still admitted only by the group's roster.` });
      } catch (e) {
        setStage({ kind: "done", ok: false, text: `The group link could not be used: ${msg(e)}` });
      }
      return;
    }
    setStage({ kind: "reading" });
    try {
      setStage({ kind: "read", claim: await api.inspectLink(l) });
    } catch (e) {
      setStage({ kind: "idle" });
      setErr(`That link could not be read: ${msg(e)}`);
    }
  };

  const pair = async (claim: PairClaim) => {
    setStage({ kind: "pairing", claim });
    try {
      const r = await api.joinLink(link.trim());
      const o = joinOutcome(r);
      setStage({ kind: "done", ...o });
      if (o.ok) onPaired?.();
    } catch (e) {
      setStage({ kind: "done", ok: false, text: `The pairing did not finish (${msg(e)}).` });
    }
  };

  return (
    <>
      <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
        Pairing link or group link
        <input className="input mono" value={link} placeholder="citrate://pair?…" onChange={(e) => setLink(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void read()} />
      </label>
      {(stage.kind === "idle" || stage.kind === "reading") && (
        <span>
          <button type="button" className="btn btn-sm" disabled={!link.trim() || stage.kind === "reading"} onClick={() => void read()}>
            {stage.kind === "reading" ? "Reading…" : "Read the link"}
          </button>
        </span>
      )}
      {err && <div role="alert" style={{ fontSize: 12.5, color: "var(--danger)", lineHeight: 1.5 }}>{err}</div>}
      {(stage.kind === "read" || stage.kind === "pairing") && (
        <div role="status" data-testid="join-claim" className="surface" style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
          <b style={{ fontWeight: 500 }}>{stage.claim.issuerLabel || "A machine"} wants to pair</b>
          <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>
            {stage.claim.issuerTier ? `Tier ${stage.claim.issuerTier}` : "Tier not reported"} · link {expiresIn(stage.claim.expiresAt, nowSecs())}
          </span>
          <span style={{ display: "flex", gap: 8 }}>
            <button type="button" className="btn btn-primary btn-sm" disabled={stage.kind === "pairing"} onClick={() => void pair(stage.claim)}>
              {stage.kind === "pairing" ? "Pairing…" : "Pair with this machine"}
            </button>
            <button type="button" className="btn btn-sm" disabled={stage.kind === "pairing"} onClick={() => setStage({ kind: "idle" })}>Decline</button>
          </span>
        </div>
      )}
      {stage.kind === "done" && (
        <div role={stage.ok ? "status" : "alert"} data-testid="join-outcome" data-ok={stage.ok ? "yes" : "no"} style={{ fontSize: 13, lineHeight: 1.55, color: stage.ok ? "var(--tx-1)" : "var(--danger)" }}>
          {stage.text}
        </div>
      )}
    </>
  );
}
