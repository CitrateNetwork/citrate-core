// 0.5.4 FL design, board 04: "Add a machine", on both devices.
//
// This device ("add"): core mints a short-lived, single-use pairing link (`fleet_pair_create`) shown
// as a QR code and text. While the sheet is open the paired-machines roster (`fleet_roster`) is
// re-read, and a machine that joins appears with the six-digit code both screens show. The other
// device ("join"): the member pastes the link, core reads it (`fleet_pair_inspect`, sends nothing),
// and only "Pair with this machine" joins (`fleet_pair_join`); every outcome is said in plain words.
// A group link (`citrate-cluster://…`, from `cluster_group_seed`) pasted on the join side is shown
// for what it is and dials the group's machines (`cluster_add_group_seed`) only on an explicit
// confirm, and not at all while the cross-machine mesh is off; the roster still decides who is
// admitted. Before pairing, core says which six-digit code pairing will show (`fleet_pair_inspect`);
// if the codes on the two machines differ, either side removes the other from its record
// (`fleet_forget`: changes nothing on the other machine and revokes no device key).
// The sheet is a real modal: focus starts on Close (the safe choice) and Escape closes it.
import { useEffect, useRef, useState } from "react";
import { ModalDialog } from "../shell/ModalDialog";
import { QrCode } from "../fleet/FleetWizard";
import { expiresIn, roleLabel } from "../fleet/wizard";
import type { FleetApi, FleetDevice, PairClaim, PairOffer } from "../bridge/tauri/fleet";
import type { ClusterGroupSeed, MeshStatus } from "../bridge/domains";
import { CODE_MISMATCH, isGroupSeed, joinOutcome, joinedThisOffer } from "./clusterBoard";

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
  /** The cross-machine mesh status: a group link can't reach anyone while it is off. */
  mesh?: MeshStatus | null;
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

/** "Remove this machine": forget a paired machine on this side, after a code mismatch. */
function RemoveMachine({ api, id, label, onRemoved }: { api: FleetApi; id: string; label: string; onRemoved?: () => void }) {
  const [state, setState] = useState<{ kind: "idle" } | { kind: "busy" } | { kind: "done" } | { kind: "error"; text: string }>({ kind: "idle" });
  if (state.kind === "done") return <span role="status" data-testid="machine-removed" style={note}>{label} is removed from this machine's paired machines. Its device key, if it has one, stays until you remove it under Your devices.</span>;
  return (
    <span style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
      <button
        type="button"
        className="btn btn-sm btn-danger"
        disabled={state.kind === "busy"}
        onClick={async () => {
          setState({ kind: "busy" });
          try {
            await api.forget(id);
            setState({ kind: "done" });
            onRemoved?.();
          } catch (e) {
            setState({ kind: "error", text: msg(e) });
          }
        }}
      >
        Remove this machine
      </button>
      {state.kind === "error" && <span role="alert" style={{ fontSize: 12, color: "var(--danger)" }}>Couldn't remove it: {state.text}</span>}
    </span>
  );
}

function AddSide({ api, groupSeed, onPaired, mesh, nowSecs = () => Date.now() / 1000 }: AddMachineSheetProps) {
  const [offer, setOffer] = useState<{ o: PairOffer; issuedAt: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [joined, setJoined] = useState<FleetDevice[]>([]);
  const [copied, setCopied] = useState(false);
  const [seed, setSeed] = useState<ClusterGroupSeed | null>(null);
  const [seedErr, setSeedErr] = useState<string | null>(null);
  const [round, setRound] = useState(0);
  const before = useRef<Set<string> | null>(null);
  const clock = useRef(nowSecs);
  clock.current = nowSecs;
  const [, tick] = useState(0);

  useEffect(() => {
    let live = true;
    setOffer(null);
    setErr(null);
    setJoined([]);
    setCopied(false);
    void (async () => {
      try {
        before.current = new Set((await api.roster()).map((d) => d.id));
      } catch {
        before.current = new Set();
      }
      try {
        // The offer's issue time bounds which new roster entry redeemed it. Taken a few seconds
        // early so clock rounding never drops the machine that really used it.
        const issuedAt = Math.floor(clock.current()) - 5;
        const o = await api.createLink();
        if (live) setOffer({ o, issuedAt });
      } catch (e) {
        if (live) setErr(msg(e));
      }
    })();
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, round]);

  const expired = !!offer && offer.o.expiresAt <= nowSecs();

  // Waiting for the other machine: re-read the roster; the machine that used THIS offer is "joined".
  useEffect(() => {
    if (!offer || joined.length > 0) return;
    const t = setInterval(() => {
      tick((n) => n + 1);
      if (offer.o.expiresAt <= clock.current()) return;
      void api
        .roster()
        .then((now) => {
          const fresh = before.current ? joinedThisOffer(before.current, now, { issuedAt: offer.issuedAt, expiresAt: offer.o.expiresAt }) : [];
          if (fresh.length) {
            setJoined(fresh);
            onPaired?.();
          }
        })
        .catch(() => {});
    }, ROSTER_POLL_MS);
    return () => clearInterval(t);
  }, [offer, joined.length, api, onPaired]);

  return (
    <>
      {err && <div role="alert" style={{ fontSize: 12.5, color: "var(--danger)", lineHeight: 1.5 }}>Couldn't make a pairing link: {err}</div>}
      {!offer && !err && <p role="status" style={note}>Making a pairing link…</p>}
      {offer && !expired && joined.length === 0 && (
        <div style={{ display: "flex", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <QrCode qr={offer.o.qr} px={3} testId="add-machine-qr" label="QR code with the pairing link" />
          <div style={{ flex: "1 1 260px", minWidth: 0, display: "flex", flexDirection: "column", gap: 10 }}>
            <ol style={{ margin: 0, paddingLeft: 18, fontSize: 13, lineHeight: 1.6 }}>
              <li>On the other machine, open Citrate Core › Groups › Cluster.</li>
              <li>Choose Join with a link and scan this code, or paste the link.</li>
              <li>Pair on the other machine. Both machines then show the same six-digit code; check they match.</li>
            </ol>
            <div className="mono" data-testid="add-machine-link" style={{ fontSize: 11, wordBreak: "break-all", background: "var(--srf-inset)", padding: "6px 8px", borderRadius: "var(--r-1)" }}>{offer.o.link}</div>
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <button
                type="button"
                className="btn btn-sm"
                onClick={async () => {
                  try {
                    await navigator.clipboard?.writeText(offer.o.link);
                    setCopied(true);
                  } catch {
                    setCopied(false);
                  }
                }}
              >
                {copied ? "Link copied" : "Copy link"}
              </button>
              <span style={note}>Single use, {expiresIn(offer.o.expiresAt, nowSecs())}.</span>
            </div>
          </div>
        </div>
      )}
      {offer && !expired && joined.length === 0 && (
        <p role="status" data-testid="add-machine-waiting" style={note}>Waiting for the other machine. It appears here when it pairs.</p>
      )}
      {offer && expired && joined.length === 0 && (
        <div role="status" data-testid="add-machine-expired" style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          <span style={{ fontSize: 13 }}>This link has expired, so no machine can pair with it now.</span>
          <button type="button" className="btn btn-primary btn-sm" onClick={() => setRound((n) => n + 1)}>Make a new link</button>
        </div>
      )}
      {joined.map((d) => (
        <div key={d.id} role="status" data-testid="add-machine-joined" className="surface" style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 6 }}>
          <b style={{ fontWeight: 500 }}>{d.label} joined</b>
          <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>
            {d.tier ? `Tier ${d.tier}` : "Tier not reported"} · {roleLabel(d.role)}
          </span>
          {d.code ? (
            <span data-testid="add-machine-code" style={{ fontSize: 13 }}>
              This machine shows code <b className="mono">{d.code}</b>. Codes match when the other machine shows the same six digits.
            </span>
          ) : (
            <span style={note}>This pairing carries no code to compare (the other machine runs an older Citrate Core).</span>
          )}
          <span style={note}>{CODE_MISMATCH}</span>
          <RemoveMachine api={api} id={d.id} label={d.label} onRemoved={onPaired} />
        </div>
      ))}
      {groupSeed && (
        <details>
          <summary style={{ fontSize: 12.5, cursor: "pointer" }}>A member's machine, not yours: share this group's link</summary>
          <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingTop: 8 }}>
            <p style={note}>The group link says where to reach this machine. Each machine is still admitted only by the group's roster.</p>
            {mesh && !mesh.on && (
              <p role="status" data-testid="group-seed-mesh-off" style={{ margin: 0, fontSize: 12.5, lineHeight: 1.5 }}>
                {MESH_NOT_AVAILABLE}
              </p>
            )}
            {!seed && !(mesh && !mesh.on) && (
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

/**
 * What a group link says while the cross-machine mesh is off (how this release ships). The same
 * words as core's `cluster::MESH_NOT_AVAILABLE`, which refuses the link commands while it is off.
 */
export const MESH_NOT_AVAILABLE =
  "Mesh is not available yet. The cross-machine mesh is off in this release until its security review is signed off, so machines can't reach each other directly. Your groups and linked machines keep working through the relay.";

/** Group links can't reach other machines while the cross-machine mesh is off on this machine. */
export const GROUP_LINK_MESH_OFF = "The cross-machine mesh is off on this machine, so a group link can't reach other machines.";

type JoinStage =
  | { kind: "idle" }
  | { kind: "reading" }
  /** A pairing link read and verified by core; `link` is exactly the text that was read. */
  | { kind: "read"; link: string; claim: PairClaim }
  | { kind: "pairing"; link: string; claim: PairClaim }
  /** A group link: shown for what it is; dials only on confirm. */
  | { kind: "seed"; link: string }
  | { kind: "dialing"; link: string }
  | { kind: "done"; ok: boolean; text: string; issuer?: { id: string; label: string } };

function JoinSide({ api, addGroupSeed, onPaired, mesh, nowSecs = () => Date.now() / 1000 }: AddMachineSheetProps) {
  const [link, setLink] = useState("");
  const [stage, setStage] = useState<JoinStage>({ kind: "idle" });
  const [err, setErr] = useState<string | null>(null);
  const meshOff = !!mesh && !mesh.on;

  const read = async () => {
    const l = link.trim();
    if (!l) return;
    setErr(null);
    if (isGroupSeed(l)) {
      if (!addGroupSeed) {
        setErr("Pick a group first: a group link joins that group's machines.");
        return;
      }
      setStage({ kind: "seed", link: l });
      return;
    }
    setStage({ kind: "reading" });
    try {
      setStage({ kind: "read", link: l, claim: await api.inspectLink(l) });
    } catch (e) {
      setStage({ kind: "idle" });
      setErr(`That link could not be read: ${msg(e)}`);
    }
  };

  const dial = async (l: string) => {
    if (!addGroupSeed || meshOff) return;
    setStage({ kind: "dialing", link: l });
    try {
      const n = await addGroupSeed(l);
      setStage({ kind: "done", ok: true, text: `Reaching ${n} address${n === 1 ? "" : "es"} from the group link. Each machine is still admitted only by the group's roster.` });
    } catch (e) {
      setStage({ kind: "done", ok: false, text: `The group link could not be used: ${msg(e)}` });
    }
  };

  // Pair with exactly the link that was read and shown, never whatever the field holds now.
  const pair = async (read: Extract<JoinStage, { kind: "read" }>) => {
    setStage({ kind: "pairing", link: read.link, claim: read.claim });
    try {
      const r = await api.joinLink(read.link);
      const o = joinOutcome(r);
      setStage({ kind: "done", ...o, ...(o.ok && r.device ? { issuer: { id: r.device.id, label: r.device.label } } : {}) });
      if (o.ok) onPaired?.();
    } catch (e) {
      setStage({ kind: "done", ok: false, text: `The pairing did not finish (${msg(e)}).` });
    }
  };

  return (
    <>
      <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
        Pairing link or group link
        <input
          className="input mono"
          value={link}
          placeholder="citrate://pair?…"
          onChange={(e) => {
            setLink(e.target.value);
            // A changed link is a new link: what was read no longer applies.
            if (stage.kind !== "pairing" && stage.kind !== "dialing" && stage.kind !== "reading") setStage({ kind: "idle" });
          }}
          onKeyDown={(e) => e.key === "Enter" && void read()}
        />
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
          {stage.claim.code ? (
            <span data-testid="join-code" style={{ fontSize: 13 }}>
              Pairing will show code <b className="mono">{stage.claim.code}</b> on both machines. Check the other machine shows the same code after you pair.
            </span>
          ) : (
            <span style={note}>After you pair, both machines show a six-digit code; check they match.</span>
          )}
          <span style={{ display: "flex", gap: 8 }}>
            <button type="button" className="btn btn-primary btn-sm" disabled={stage.kind === "pairing"} onClick={() => stage.kind === "read" && void pair(stage)}>
              {stage.kind === "pairing" ? "Pairing…" : "Pair with this machine"}
            </button>
            <button type="button" className="btn btn-sm" disabled={stage.kind === "pairing"} onClick={() => setStage({ kind: "idle" })}>Decline</button>
          </span>
        </div>
      )}
      {(stage.kind === "seed" || stage.kind === "dialing") && (
        <div role="status" data-testid="join-seed" className="surface" style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
          <b style={{ fontWeight: 500 }}>This is a group link</b>
          <span style={{ fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.5 }}>
            It says where a member's machines can be reached. Reaching them pairs nothing: each machine is still admitted only by the group's roster.
          </span>
          {meshOff ? (
            <span data-testid="join-seed-mesh-off" style={{ fontSize: 12.5, lineHeight: 1.5 }}>
              {mesh?.signedOff
                ? `${GROUP_LINK_MESH_OFF} ${mesh.note} Turn on Private mesh on the cluster screen first.`
                : MESH_NOT_AVAILABLE}
            </span>
          ) : (
            <span style={{ display: "flex", gap: 8 }}>
              <button type="button" className="btn btn-primary btn-sm" disabled={stage.kind === "dialing"} onClick={() => void dial(stage.link)}>
                {stage.kind === "dialing" ? "Reaching…" : "Reach this group's machines"}
              </button>
              <button type="button" className="btn btn-sm" disabled={stage.kind === "dialing"} onClick={() => setStage({ kind: "idle" })}>Cancel</button>
            </span>
          )}
        </div>
      )}
      {stage.kind === "done" && (
        <div role={stage.ok ? "status" : "alert"} data-testid="join-outcome" data-ok={stage.ok ? "yes" : "no"} style={{ fontSize: 13, lineHeight: 1.55, color: stage.ok ? "var(--tx-1)" : "var(--danger)", display: "flex", flexDirection: "column", gap: 8 }}>
          <span>{stage.text}</span>
          {stage.issuer && (
            <>
              <span style={note}>{CODE_MISMATCH}</span>
              <RemoveMachine api={api} id={stage.issuer.id} label={stage.issuer.label} onRemoved={onPaired} />
            </>
          )}
        </div>
      )}
    </>
  );
}
