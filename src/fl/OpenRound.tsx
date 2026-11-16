// =====================================================================
// citrate-core: Groups › Rounds › Open a round (FL design board 07)
//
// Four steps: template, budget and rate, who may join and who coordinates, then preview and sign.
// Opening is HIC-1: the poster signs the escrow, which leaves their wallet into WorkMarket.
// Ship default (r1007): "Group rounds not live yet. WorkMarket is not available yet on this
// network, so there is no Open button. You can save this draft." Drafts are kept on this device
// (roundDraft.ts). Only owners and admins on the live roster may open a round (FLM-CORE-1); the
// role comes from the group's roster. The rate hint says there are no comparable rounds (n = 0)
// rather than inventing a number.
// =====================================================================
import { useState } from "react";
import { CeremonySheet, outcomeWords, type TxOutcome } from "./CeremonySheet";
import type { Group } from "../bridge/domains";
import { NOT_AVAILABLE, ROUNDS_NOT_LIVE, type FlNetworkState } from "./networkState";
import { Chip, HicBadge, KV, Note, Unknown } from "./primitives";
import { asSavedByMember, draftProblems, leasesCovered, saveRoundDraft, weiOf, type RoundDraft, type RoundTemplate, type WhoMayJoin } from "./roundDraft";
import { saltExact } from "./format";

export const STEPS = ["Template", "Budget and rate", "Who and coordinator", "Preview and sign"] as const;

export const TEMPLATES: [RoundTemplate, string, string][] = [
  ["tune", "Fine-tune on members' data", "Each member trains a LoRA update on their local data of one class."],
  ["eval", "Evaluate a model", "Members score a model on local data; only scorecards leave."],
  ["data", "Build a dataset", "Members clean and label local data into a shared version."],
];
export const WHO: [WhoMayJoin, string][] = [
  ["members", "This group, role member or above"],
  ["group", "This group, any role except read-only"],
  ["anyone", "Anyone who matches the requirements"],
];

export interface OpenRoundViewProps {
  state: FlNetworkState;
  draft: RoundDraft;
  groups: Group[];
  /** This member's role in a group, or null when the roster was not read. */
  roleOf: (groupId: string | null) => string | null;
  /** The coordinator configured on this device (fl_rounds.rs), if any. */
  coordinatorUrl: string | null;
  /** Open the round through the SignatureCeremony; resolves with the outcome. */
  onOpen: (d: RoundDraft) => Promise<TxOutcome>;
  save?: (d: RoundDraft) => number;
  go: (route: string) => void;
}

const field: React.CSSProperties = { display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 };

/** A typed SALT amount as the exact wei it signs (G-SEC PAY-19), or "—" when it is not one. */
export function moneyWords(typed: string): string {
  const w = weiOf(typed);
  return w === null ? "— (not a SALT amount)" : `${saltExact(w)} SALT (${w} wei)`;
}

export function OpenRoundView(p: OpenRoundViewProps) {
  const [step, setStep] = useState(0);
  const [d, setD] = useState<RoundDraft>(p.draft);
  const [saved, setSaved] = useState("");
  const [sheet, setSheet] = useState<{ step: "review" | "signing" | "done"; outcome?: TxOutcome } | null>(null);
  const live = p.state.phase === "rounds-live";
  const myRole = p.roleOf(d.groupId);
  const admin = myRole === "owner" || myRole === "admin";
  const leases = leasesCovered(d.budgetSalt, d.rateSalt);
  const problems = draftProblems(d);
  const set = <K extends keyof RoundDraft>(k: K, v: RoundDraft[K]) => setD((x) => ({ ...x, [k]: v }));
  const save = p.save ?? saveRoundDraft;
  const doSave = () => {
    try {
      // PAY-19: money is typed here by the member, so a filled-in draft is the member's.
      const mine = asSavedByMember(d);
      const at = save(mine);
      setD(mine);
      setSaved(`Draft saved on this device at ${new Date(at).toTimeString().slice(0, 5)}`);
    } catch (e) {
      setSaved("Couldn't save the draft: " + (e instanceof Error ? e.message : String(e)));
    }
  };
  const sign = async () => {
    setSheet({ step: "signing" });
    try {
      const outcome = await p.onOpen(asSavedByMember(d));
      setSheet({ step: "done", outcome });
    } catch (e) {
      setSheet({ step: "done", outcome: { kind: "not-sent", reason: e instanceof Error ? e.message : String(e) } });
    }
  };
  const group = p.groups.find((g) => g.id === d.groupId);
  const tpl = TEMPLATES.find((t) => t[0] === d.template)!;
  // G-SEC (PAY-10): opening escrows SALT, so it signs only once the profile opens payouts.
  const moneyOpen = p.state.payouts === "open";
  const canSign = live && admin && moneyOpen && problems.length === 0;

  return (
    <div className="fl-cols" data-testid="open-round" data-live={live}>
      <nav aria-label="Steps" style={{ flex: "0 0 230px", borderRight: "1px solid var(--line-1)", padding: "14px 10px", display: "flex", flexDirection: "column", gap: 4 }}>
        <span className="eyebrow" style={{ padding: "0 6px 6px" }}>
          Open a round{group ? ` · ${group.name}` : ""}
        </span>
        {STEPS.map((label, i) => (
          <button key={label} type="button" aria-current={step === i ? "step" : undefined} className={step === i ? "btn btn-sm btn-secondary" : "btn btn-sm btn-ghost"} style={{ justifyContent: "flex-start", gap: 8, minHeight: 36 }} onClick={() => setStep(i)}>
            <span className="mono">{i + 1}</span>
            {label}
            {i < step ? (<><span aria-hidden="true">✓</span><span className="sr-only">, done</span></>) : null}
          </button>
        ))}
        <Note style={{ marginTop: 10 }}>Opening a round is HIC-1: you sign the escrow. The budget leaves your wallet into WorkMarket escrow.</Note>
      </nav>
      <section className="fl-main" aria-labelledby="open-h">
        {(d.source === "hermes" || d.draftedBy === "hermes") && (
          <div className="fl-card" style={{ padding: "8px 12px", display: "flex", gap: 10, alignItems: "center", fontSize: 12.5 }}>
            <Chip tone="info">Drafted by Hermes</Chip>
            <span>{d.draftedFor ? `For ${d.draftedFor}. ` : ""}Check every field; Hermes cannot sign.</span>
          </div>
        )}
        {!live && (
          <div role="status" data-testid="open-not-live" className="fl-card" style={{ padding: "8px 12px", borderColor: "var(--warn)", background: "var(--warn-bg)", fontSize: 12.5 }}>
            {ROUNDS_NOT_LIVE}. WorkMarket is {NOT_AVAILABLE.charAt(0).toLowerCase() + NOT_AVAILABLE.slice(1)}, so there is no Open button. You can save this draft.
          </div>
        )}
        {live && myRole !== null && !admin && (
          <div role="alert" className="fl-card" style={{ padding: "8px 12px", borderColor: "var(--danger)", background: "var(--danger-bg)", fontSize: 12.5 }}>
            Only owners and admins can open a round. You can read and comment on the draft.
          </div>
        )}
        {live && myRole === null && d.groupId && (
          <p style={{ margin: 0, fontSize: 12.5 }}>
            Your role in this group: <Unknown reason="the roster was not read" />
          </p>
        )}

        {step === 0 && (
          <>
            <h1 id="open-h" className="fl-disp" style={{ fontSize: 24 }}>
              What should the round do?
            </h1>
            <label style={field}>
              Group
              <select className="input" value={d.groupId ?? ""} onChange={(e) => set("groupId", e.target.value || null)}>
                <option value="">{p.groups.length ? "Choose a group" : "No groups yet"}</option>
                {p.groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.name || g.id.slice(0, 10)}
                  </option>
                ))}
              </select>
            </label>
            <div role="radiogroup" aria-label="Template" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))", gap: 8 }}>
              {TEMPLATES.map(([id, t, desc]) => (
                <button key={id} type="button" role="radio" aria-checked={d.template === id} className="fl-card" onClick={() => set("template", id)} style={{ padding: "12px 14px", textAlign: "left", cursor: "pointer", border: d.template === id ? "2px solid var(--accent-deep)" : "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 4, color: "var(--tx-1)" }}>
                  <span style={{ fontWeight: 500, fontSize: 13.5 }}>{t}</span>
                  <span className="fl-muted" style={{ fontSize: 12 }}>
                    {desc}
                  </span>
                </button>
              ))}
            </div>
            <label style={field}>
              Data class members train on
              <input className="input" value={d.dataClass} placeholder="support-tickets" onChange={(e) => set("dataClass", e.target.value)} />
            </label>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: "14px 24px", maxWidth: 720 }}>
              <label style={field}>
                Architecture (lowercase label)
                <input className="input mono" value={d.architecture} placeholder="llama-8b-lora-r8" onChange={(e) => set("architecture", e.target.value)} />
              </label>
              <label style={field}>
                Minimum GPU memory (GB)
                <input className="input mono" type="number" min={1} value={d.minVramGb} onChange={(e) => set("minVramGb", Number(e.target.value))} />
              </label>
              <label style={field}>
                Estimated hours per lease
                <input className="input mono" type="number" min={1} value={d.estHours} onChange={(e) => set("estHours", Number(e.target.value))} />
              </label>
            </div>
            <Note>Members' data stays on their machines. The round reads only datasets each member adds, locally. These requirements are published with the round so each member's app can check its own machine.</Note>
          </>
        )}

        {step === 1 && (
          <>
            <h1 id="open-h" className="fl-disp" style={{ fontSize: 24 }}>
              Budget and rate
            </h1>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: "14px 24px", maxWidth: 720 }}>
              <label style={field}>
                Budget (SALT, held in escrow)
                <input className="input mono" inputMode="decimal" placeholder="enter an amount" value={d.budgetSalt} onChange={(e) => set("budgetSalt", e.target.value)} />
              </label>
              <label style={field}>
                Rate per lease (SALT)
                <input className="input mono" inputMode="decimal" placeholder="enter an amount" value={d.rateSalt} onChange={(e) => set("rateSalt", e.target.value)} />
                <span className="fl-faint" style={{ fontSize: 11.5 }}>
                  No comparable rounds on this network yet (n = 0), so there is no suggested rate.
                </span>
              </label>
              <label style={field}>
                Join bond each worker posts (SALT)
                <input className="input mono" inputMode="decimal" placeholder="0 for none" value={d.joinBondSalt} onChange={(e) => set("joinBondSalt", e.target.value)} />
                <span className="fl-faint" style={{ fontSize: 11.5 }}>Returned to the worker when they settle.</span>
              </label>
              <label style={field}>
                Coordinator bond (SALT)
                <input className="input mono" inputMode="decimal" placeholder="empty: WorkMarket's default" value={d.coordinatorBondSalt} onChange={(e) => set("coordinatorBondSalt", e.target.value)} />
              </label>
              <label style={field}>
                Dispute window (hours, at least 24)
                <input className="input mono" type="number" min={24} value={d.disputeWindowHours} onChange={(e) => set("disputeWindowHours", Number(e.target.value))} />
              </label>
              <label style={field}>
                Max workers
                <input className="input mono" type="number" min={3} value={d.maxWorkers} onChange={(e) => set("maxWorkers", Number(e.target.value))} />
              </label>
              <label style={field}>
                Deadline
                <input className="input mono" type="date" value={d.deadline} onChange={(e) => set("deadline", e.target.value)} />
              </label>
            </div>
            <div className="fl-inset" data-testid="covers" style={{ padding: "10px 12px", fontSize: 13, maxWidth: 720 }}>
              {leases === null ? <>Budget covers: <Unknown reason="budget and rate must be SALT amounts" /></> : <>Budget covers <b className="mono">{leases}</b> leases at this rate.</>}
            </div>
          </>
        )}

        {step === 2 && (
          <>
            <h1 id="open-h" className="fl-disp" style={{ fontSize: 24 }}>
              Who may join, and who coordinates
            </h1>
            <fieldset style={{ border: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 6 }}>
              <legend className="eyebrow">Who may join</legend>
              {WHO.map(([id, t]) => (
                <label key={id} style={{ display: "flex", gap: 8, fontSize: 13, alignItems: "center" }}>
                  <input type="radio" name="fl-who" checked={d.who === id} onChange={() => set("who", id)} />
                  {t}
                </label>
              ))}
            </fieldset>
            <fieldset style={{ border: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 6 }}>
              <legend className="eyebrow">Coordinator</legend>
              {p.coordinatorUrl ? (
                <label style={{ display: "flex", gap: 8, fontSize: 13, alignItems: "center" }}>
                  <input type="radio" name="fl-coord" checked={d.coordinator === p.coordinatorUrl} onChange={() => set("coordinator", p.coordinatorUrl)} />
                  <span className="mono">{p.coordinatorUrl}</span>
                </label>
              ) : (
                <span className="fl-muted" style={{ fontSize: 12.5 }}>
                  No coordinator is configured on this device. An operator runs one; set it under Matches › Advanced.
                </span>
              )}
            </fieldset>
            <Note>Members only see group offers from an owner or admin on the live roster (FLM-CORE-1).</Note>
          </>
        )}

        {step === 3 && (
          <>
            <h1 id="open-h" className="fl-disp" style={{ fontSize: 24 }}>
              Preview
            </h1>
            <KV
              style={{ maxWidth: 720 }}
              rows={[
                { k: "Group", v: group ? group.name : <Unknown reason="no group chosen" /> },
                { k: "Template", v: `${tpl[1]}${d.dataClass ? " · " + d.dataClass : ""}` },
                { k: "Budget, into escrow", v: moneyWords(d.budgetSalt), mono: true },
                { k: "Rate", v: moneyWords(d.rateSalt) + " per lease", mono: true },
                { k: "Join bond", v: moneyWords(d.joinBondSalt), mono: true },
                { k: "Coordinator bond", v: d.coordinatorBondSalt.trim() === "" ? "WorkMarket's default" : moneyWords(d.coordinatorBondSalt), mono: true },
                { k: "Dispute window", v: `${d.disputeWindowHours} hours` },
                { k: "Covers", v: leases === null ? <Unknown reason="not a SALT amount" /> : `${leases} leases · up to ${d.maxWorkers} workers` },
                { k: "Who may join", v: WHO.find((w) => w[0] === d.who)![1] },
                { k: "Coordinator", v: d.coordinator ?? <Unknown reason="none chosen" /> },
                { k: "Deadline", v: d.deadline || <Unknown reason="not set" />, mono: true },
                { k: "Refund", v: "Unspent escrow is refundable to you after Closed or Expired" },
              ]}
            />
            {problems.length > 0 && (
              <ul data-testid="open-problems" style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, color: "var(--warn)" }}>
                {problems.map((x) => (
                  <li key={x}>{x}</li>
                ))}
              </ul>
            )}
          </>
        )}

        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginTop: 6 }}>
          <button type="button" className="btn btn-ghost" disabled={step === 0} onClick={() => setStep(Math.max(0, step - 1))}>
            Back
          </button>
          <button type="button" className="btn btn-ghost" onClick={doSave}>
            Save draft
          </button>
          <span role="status" className="fl-faint" style={{ fontSize: 12 }}>
            {saved}
          </span>
          <span style={{ flex: 1 }} />
          {step < 3 && (
            <button type="button" className="btn btn-primary" onClick={() => setStep(step + 1)}>
              Continue
            </button>
          )}
          {step === 3 && live && admin && !moneyOpen && (
            <span className="fl-faint" style={{ fontSize: 12 }} data-testid="open-waits-gsec">
              Opening escrows SALT, so it opens with payouts after the security review. You can save this draft.
            </span>
          )}
          {step === 3 && live && admin && moneyOpen && (
            <button type="button" className="btn btn-primary" disabled={!canSign} onClick={() => setSheet({ step: "review" })}>
              <HicBadge level="HIC-1" style={{ background: "var(--on-accent)", color: "var(--accent)" }} />
              Sign escrow and open
            </button>
          )}
        </div>
      </section>

      {sheet && (
        <CeremonySheet
          id="fl-open"
          title={`Open a round${group ? " · " + group.name : ""}`}
          hic="HIC-1"
          step={sheet.step}
          signed={2}
          signLabel="Sign escrow and open"
          onSign={() => void sign()}
          onDecline={() => setSheet(null)}
          outcome={sheet.outcome}
          successLine={sheet.outcome ? `The round is open. Escrow holds ${moneyWords(d.budgetSalt)}.` : undefined}
          review={
            <KV
              rows={[
                { k: "Escrow", v: `${moneyWords(d.budgetSalt)} leaves your wallet into WorkMarket`, mono: true },
                { k: "Rate", v: `${moneyWords(d.rateSalt)} per lease, ${leases ?? "—"} leases`, mono: true },
                { k: "Join bond", v: moneyWords(d.joinBondSalt), mono: true },
                { k: "Network fee", v: <Unknown reason="the wallet review reads gas before you sign" /> },
              ]}
            />
          }
          outcomeActions={
            sheet.outcome && outcomeWords(sheet.outcome).tone === "ok" ? (
              <button type="button" className="btn btn-primary" onClick={() => p.go("groups/rounds")}>
                See it in Matches
              </button>
            ) : undefined
          }
        />
      )}
    </div>
  );
}
