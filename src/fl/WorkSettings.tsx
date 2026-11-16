// =====================================================================
// citrate-core: Settings › Work (FL design board 11): terms, consent, the HIC-2 envelope, payout
// address
//
// Everything here is core's: terms and the envelope are sealed on this device (fl_work.rs),
// per-round consent is the device worker's consent file (fl_rounds.rs), and "train on my verified
// conversations" is fl_trajectories.rs. Raising authority always needs a confirm card: saving
// terms shows one here; approving an envelope goes through the approval card (HIC-1 to grant
// HIC-2). Lowering authority (withdrawing consent, turning the envelope off) applies at once with
// no card. Offer text can never change the envelope.
// =====================================================================
import { useEffect, useState } from "react";
import { bridge } from "../bridge";
import { useFlNetworkState } from "./useFlNetworkState";
import type { FlOverview, FlRoundsDomain } from "../bridge/domains";
import type { CerSpec } from "../shell/state";
import { approveEnvelope, envelopeOff, readEnvelope, type EnvelopeRecord } from "./authority";
import type { FlNetworkState } from "./networkState";
import { Card, Chip, Eyebrow, HicBadge, Hex, Note, shortHex, Unknown } from "./primitives";
import { TrajectoryConsent } from "./TrajectoryConsent";
import { ALLOWED_DATA_CLASSES, dataClassWords, loadWorkerTerms, saveWorkerTerms, termsProblem, type WorkerTerms } from "./workerTerms";

/** The route lanes link to (Hermes policy card, Matches rail). */
export const SETTINGS_WORK_ROUTE = "settings/work";

export const DEFAULT_TERMS: WorkerTerms = { floorSaltPerUnit: "1.00", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: [] };

type Section = "terms" | "consent" | "env" | "pay";
const SECTIONS: [Section, string][] = [
  ["terms", "Terms"],
  ["consent", "Consent"],
  ["env", "Envelope (HIC-2)"],
  ["pay", "Payout address"],
];

const hhmm = (d: number) => new Date(d).toTimeString().slice(0, 5);
const hoursWord = (t: WorkerTerms) => (t.hours === "any" ? "any time" : `${t.hours.start} to ${t.hours.end}`);

export interface WorkSettingsViewProps {
  state: FlNetworkState;
  fl: FlRoundsDomain;
  wallet: string;
  requestSig: (spec: CerSpec) => Promise<string>;
  toast: (m: string) => void;
  go: (route: string) => void;
  /** Test seams; default to core. */
  terms?: { load: typeof loadWorkerTerms; save: typeof saveWorkerTerms };
  envelope?: { read: typeof readEnvelope; approve: typeof approveEnvelope; off: typeof envelopeOff };
}

export function WorkSettingsView(p: WorkSettingsViewProps) {
  const T = p.terms ?? { load: loadWorkerTerms, save: saveWorkerTerms };
  const E = p.envelope ?? { read: readEnvelope, approve: approveEnvelope, off: envelopeOff };
  const [sec, setSec] = useState<Section>("terms");
  const [terms, setTerms] = useState<WorkerTerms>(DEFAULT_TERMS);
  const [classes, setClasses] = useState("");
  const [termsRead, setTermsRead] = useState<"reading" | "none" | "read" | { failed: string }>("reading");
  const [confirm, setConfirm] = useState(false);
  const [saved, setSaved] = useState<{ ok: boolean; line: string } | null>(null);
  const [env, setEnv] = useState<EnvelopeRecord | null>(null);
  const [envRead, setEnvRead] = useState<"reading" | "read" | { failed: string }>("reading");
  const [caps, setCaps] = useState({ name: "Weeknight rounds", joinsPerWeek: 3, maxHoursPerRound: 6 });
  const [envNote, setEnvNote] = useState("");
  const [ov, setOv] = useState<FlOverview | null>(null);
  const [ovErr, setOvErr] = useState<string | null>(null);

  useEffect(() => {
    T.load().then(
      (t) => {
        if (t) {
          setTerms(t);
          setClasses(t.dataClasses.join(", "));
          setTermsRead("read");
        } else setTermsRead("none");
      },
      (e) => setTermsRead({ failed: e instanceof Error ? e.message : String(e) }),
    );
    E.read().then(
      (e) => {
        setEnv(e);
        if (e) setCaps({ name: e.name, joinsPerWeek: e.joinsPerWeek, maxHoursPerRound: e.maxHoursPerRound });
        setEnvRead("read");
      },
      (e) => setEnvRead({ failed: e instanceof Error ? e.message : String(e) }),
    );
    void refreshOv();
  }, []);

  const refreshOv = async () => {
    try {
      const o = await p.fl.overview();
      setOv(o);
      setOvErr(o.consentError ?? o.storeError);
    } catch (e) {
      setOvErr(e instanceof Error ? e.message : String(e));
    }
  };

  const draft: WorkerTerms = { ...terms, dataClasses: classes.split(",").map((c) => c.trim()).filter(Boolean) };
  const problem = termsProblem(draft);
  const floorLive = p.state.contracts.WorkMarket.kind === "deployed";

  const doSave = async () => {
    setConfirm(false);
    try {
      const r = await T.save(draft);
      setTerms(draft);
      setTermsRead("read");
      setSaved({ ok: true, line: `Saved and sealed at ${hhmm(r.savedAt)} · ${shortHex("0x" + r.termsHash, 8, 4)}` });
    } catch (e) {
      setSaved({ ok: false, line: `Save failed, nothing changed: ${e instanceof Error ? e.message : String(e)}` });
    }
  };

  const signEnvelope = async () => {
    setEnvNote("");
    const spec: CerSpec = {
      origin: "Settings › Work",
      requester: "Settings › Work · HIC-2 envelope",
      title: "Approve an envelope for Hermes",
      chainless: true,
      rows: [
        { k: "Name", v: caps.name },
        { k: "Joins per week", v: String(caps.joinsPerWeek) },
        { k: "Machine hours per round", v: String(caps.maxHoursPerRound) },
        { k: "Once rounds are live", v: "The first join in any new group or with a new poster will need your tap, and the envelope will never go below your floor or outside your hours. No round can be joined on this network yet." },
        { k: "Offer text", v: "Can never change the envelope." },
        { k: "Turn off", v: "Any time, at once, with no card." },
      ],
      cost: "none, no chain transaction",
      sponsor: "your explicit decision (HIC-1 grants HIC-2)",
      sponsorColor: "var(--tx-3)",
    };
    const r = await p.requestSig(spec);
    if (r !== "approved") {
      setEnvNote("Declined. Nothing changed; every join still asks you.");
      return;
    }
    try {
      const e = await E.approve(caps);
      setEnv(e);
      setEnvNote(`Approved at ${hhmm(e.approvedAt)}. Turning it off needs no card.`);
    } catch (e) {
      setEnvNote("Couldn't save the envelope, so it is not on: " + (e instanceof Error ? e.message : String(e)));
    }
  };

  const turnOff = async () => {
    try {
      const e = await E.off();
      setEnv(e);
      setEnvNote("Off. Every join asks you again.");
    } catch (e) {
      setEnvNote("Couldn't record that it is off: " + (e instanceof Error ? e.message : String(e)) + ". Hermes uses HIC-1 on this device until it is read again.");
    }
  };

  const revoke = async (round: string) => {
    try {
      await p.fl.revokeConsent(round);
      p.toast("Consent withdrawn for that round.");
      await refreshOv();
    } catch (e) {
      setOvErr(e instanceof Error ? e.message : String(e));
    }
  };

  const stepper = (label: string, value: number, set: (n: number) => void, min: number, max: number) => (
    <div style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
      <span>{label}</span>
      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
        <button type="button" className="btn btn-ghost btn-sm" aria-label={`Lower ${label.toLowerCase()}`} disabled={value <= min} onClick={() => set(value - 1)}>
          −
        </button>
        <output className="input mono" aria-label={label} style={{ textAlign: "center", width: 70 }}>
          {value}
        </output>
        <button type="button" className="btn btn-ghost btn-sm" aria-label={`Raise ${label.toLowerCase()}`} disabled={value >= max} onClick={() => set(value + 1)}>
          +
        </button>
      </div>
    </div>
  );

  return (
    <div className="fl-cols" data-testid="work-settings">
      <nav aria-label="Work settings" style={{ flex: "0 0 220px", borderRight: "1px solid var(--line-1)", padding: "14px 10px", display: "flex", flexDirection: "column", gap: 2 }}>
        {SECTIONS.map(([id, label]) => (
          <button key={id} type="button" aria-current={sec === id ? "page" : undefined} className={sec === id ? "btn btn-sm btn-secondary" : "btn btn-ghost btn-sm"} style={{ justifyContent: "flex-start", width: "100%", minHeight: 40 }} onClick={() => setSec(id)}>
            {label}
          </button>
        ))}
        <Note style={{ marginTop: "auto" }}>Technical members get every field here. Others can set the same terms by talking to Hermes.</Note>
        <button type="button" className="btn btn-sm btn-ghost" onClick={() => p.go("hermes/chat")}>
          Ask Hermes instead
        </button>
      </nav>

      <section className="fl-main" aria-labelledby="work-h">
        {sec === "terms" && (
          <>
            <h1 id="work-h" className="fl-disp" style={{ fontSize: 24 }}>
              Your terms
            </h1>
            <p className="fl-muted" style={{ margin: 0, fontSize: 13 }}>
              Sealed on this device and never published. Only the floor is enforced on chain.
            </p>
            {!floorLive && <Note>Floor not enforced yet: WorkMarket is not available yet on this network.</Note>}
            {typeof termsRead === "object" && (
              <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--warn)" }}>
                Your saved terms: <Unknown reason={termsRead.failed} />
              </p>
            )}
            {termsRead === "none" && <span className="fl-faint" style={{ fontSize: 12 }}>No terms yet. These are the defaults until you save.</span>}
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: "14px 24px", maxWidth: 820 }}>
              <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
                Floor (SALT per unit)
                <input className="input mono" inputMode="decimal" data-testid="terms-floor" value={terms.floorSaltPerUnit} onChange={(e) => setTerms({ ...terms, floorSaltPerUnit: e.target.value })} />
                <span className="fl-faint" style={{ fontSize: 11.5 }}>No comparable rounds on this network yet (n = 0).</span>
              </label>
              <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
                Hours
                <select className="input" value={terms.hours === "any" ? "any" : "window"} onChange={(e) => setTerms({ ...terms, hours: e.target.value === "any" ? "any" : { start: "23:00", end: "07:00" } })}>
                  <option value="window">A window each day</option>
                  <option value="any">Any time</option>
                </select>
                {terms.hours !== "any" && (
                  <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                    <input className="input mono" type="time" aria-label="From" value={terms.hours.start} onChange={(e) => setTerms({ ...terms, hours: { ...(terms.hours as { start: string; end: string }), start: e.target.value } })} />
                    to
                    <input className="input mono" type="time" aria-label="To" value={terms.hours.end} onChange={(e) => setTerms({ ...terms, hours: { ...(terms.hours as { start: string; end: string }), end: e.target.value } })} />
                  </span>
                )}
              </label>
              <label style={{ display: "flex", gap: 10, alignItems: "center", fontSize: 13.5 }}>
                <input type="checkbox" checked={terms.weekdaysOnly} onChange={(e) => setTerms({ ...terms, weekdaysOnly: e.target.checked })} />
                Weekdays only
              </label>
              <label style={{ display: "flex", gap: 10, alignItems: "center", fontSize: 13.5 }}>
                <input type="checkbox" checked={terms.mainsOnly} onChange={(e) => setTerms({ ...terms, mainsOnly: e.target.checked })} />
                Only on mains power
              </label>
              <fieldset style={{ border: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 6, gridColumn: "1 / -1" }}>
                <legend style={{ fontSize: 12.5, marginBottom: 4 }}>Data classes rounds may read (none checked means none)</legend>
                {Object.entries(ALLOWED_DATA_CLASSES).map(([id, words]) => (
                  <label key={id} style={{ display: "flex", gap: 10, alignItems: "center", fontSize: 13 }}>
                    <input
                      type="checkbox"
                      data-testid={"terms-class-" + id}
                      checked={classes.split(",").map((c) => c.trim()).includes(id)}
                      onChange={(e) => {
                        const cur = classes.split(",").map((c) => c.trim()).filter(Boolean).filter((c) => c !== id);
                        setClasses((e.target.checked ? [...cur, id] : cur).join(", "));
                      }}
                    />
                    {words}
                  </label>
                ))}
              </fieldset>
            </div>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <button type="button" className="btn btn-primary" disabled={!!problem || termsRead === "reading"} onClick={() => setConfirm(true)}>
                Save terms
              </button>
              {problem && <span style={{ fontSize: 12, color: "var(--warn)" }}>{problem}</span>}
              {saved && (
                <span role="status" style={{ fontSize: 12, color: saved.ok ? "var(--ok)" : "var(--danger)" }}>
                  {saved.line}
                </span>
              )}
            </div>
            {confirm && (
              <section className="fl-card-2" aria-labelledby="cf-h" data-testid="terms-confirm" style={{ maxWidth: 820, display: "flex", flexDirection: "column" }}>
                <div style={{ display: "flex", gap: 10, alignItems: "center", padding: "10px 14px", background: "var(--srf-inset)", borderBottom: "1px solid var(--line-1)" }}>
                  <HicBadge level="HIC-1" />
                  <h2 id="cf-h" style={{ margin: 0, fontSize: 14, fontWeight: 600 }}>
                    Confirm your new terms
                  </h2>
                </div>
                <p style={{ margin: 0, padding: "12px 14px", fontSize: 13 }}>
                  Floor {draft.floorSaltPerUnit} SALT · {hoursWord(draft)} · {draft.weekdaysOnly ? "weekdays" : "every day"} · {draft.mainsOnly ? "mains only" : "battery allowed"} · {draft.dataClasses.length ? draft.dataClasses.map(dataClassWords).join(", ") : "no data classes"}.
                </p>
                <div style={{ display: "flex", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--line-1)" }}>
                  <button type="button" className="btn btn-primary btn-sm" onClick={() => void doSave()}>
                    Confirm
                  </button>
                  <button type="button" className="btn btn-ghost btn-sm" onClick={() => setConfirm(false)}>
                    Cancel
                  </button>
                </div>
              </section>
            )}
          </>
        )}

        {sec === "consent" && (
          <>
            <h1 id="work-h" className="fl-disp" style={{ fontSize: 24 }}>
              Consent
            </h1>
            <p className="fl-muted" style={{ margin: 0, fontSize: 13 }}>
              Consent is per round and per dataset. Withdrawing lowers authority, so it applies at once with no card.
            </p>
            {ovErr && (
              <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--warn)" }}>
                {ovErr}
              </p>
            )}
            {ov && ov.consentedRounds.length === 0 && (
              <p className="fl-muted" style={{ margin: 0, fontSize: 13 }} data-testid="consent-none">
                No round consent on this device. Joining a round writes consent for that round only.
              </p>
            )}
            {ov?.consentedRounds.map((r) => (
              <Card key={r} style={{ flexDirection: "row", alignItems: "center", gap: 12, maxWidth: 820 }}>
                <span style={{ flex: 1, display: "flex", flexDirection: "column", gap: 3 }}>
                  <span style={{ fontSize: 13.5, fontWeight: 500 }}>Round {shortHex(r)}</span>
                  <span className="fl-muted" style={{ fontSize: 12 }}>
                    The device training worker takes part in this round only.
                  </span>
                </span>
                <Chip tone="ok">Consented</Chip>
                <button type="button" className="btn btn-sm btn-ghost" onClick={() => void revoke(r)}>
                  Withdraw
                </button>
              </Card>
            ))}
            <Card style={{ maxWidth: 820 }}>
              <TrajectoryConsent fl={p.fl} toast={p.toast} />
            </Card>
            <Note>Training reads only local datasets, in a sandbox with read-only access to one dataset version. Only the signed update is uploaded.</Note>
          </>
        )}

        {sec === "env" && (
          <>
            <h1 id="work-h" className="fl-disp" style={{ fontSize: 24 }}>
              Envelope (HIC-2)
            </h1>
            <p className="fl-muted" style={{ margin: 0, fontSize: 13, maxWidth: 760 }}>
              An envelope lets Hermes join rounds for you inside limits you approve. It is off by default. Outside it, every action asks you (HIC-1).
            </p>
            {typeof envRead === "object" && (
              <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--warn)" }}>
                Your envelope: <Unknown reason={envRead.failed} />. Hermes uses HIC-1 until it is read.
              </p>
            )}
            <Card style={{ flexDirection: "row", alignItems: "center", gap: 12, maxWidth: 820 }} testid="env-state">
              <Chip tone={env?.on ? "ok" : "neutral"}>{env?.on ? "On" : "Off"}</Chip>
              <span style={{ flex: 1, fontSize: 13 }}>
                {env?.on ? `Approved at ${hhmm(env.approvedAt)}. ${env.joinsPerWeek} joins per week (${Math.max(0, env.joinsPerWeek - env.joinsThisWeek)} left this week), up to ${env.maxHoursPerRound} machine hours per round.` : "Every join asks you."}
              </span>
              {env?.on && (
                <button type="button" className="btn btn-sm btn-danger" onClick={() => void turnOff()}>
                  Turn off now
                </button>
              )}
            </Card>
            {!env?.on && (
              <>
                <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5, maxWidth: 400 }}>
                  Name
                  <input className="input" value={caps.name} onChange={(e) => setCaps({ ...caps, name: e.target.value })} />
                </label>
                <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: "14px 24px", maxWidth: 820 }}>
                  {stepper("Joins per week", caps.joinsPerWeek, (n) => setCaps({ ...caps, joinsPerWeek: n }), 1, 50)}
                  {stepper("Max machine hours per round", caps.maxHoursPerRound, (n) => setCaps({ ...caps, maxHoursPerRound: n }), 1, 72)}
                </div>
              </>
            )}
            <p style={{ margin: 0, fontSize: 12.5, maxWidth: 820 }} data-testid="env-rules-when">
              These rules apply once rounds are live on this network. Until then no round can be joined, with or without an envelope.
            </p>
            <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, lineHeight: 1.6, maxWidth: 820 }}>
              <li>
                <b style={{ fontWeight: 500 }}>Always asks:</b> the first join in any new group or with a new poster will need your tap.
              </li>
              <li>
                <b style={{ fontWeight: 500 }}>Fixed:</b> offer text can never change the envelope.
              </li>
              <li>
                <b style={{ fontWeight: 500 }}>Within your terms:</b> the envelope will never go below your floor or outside your hours.
              </li>
            </ul>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              {!env?.on && (
                <button type="button" className="btn btn-primary" disabled={!caps.name.trim() || typeof envRead === "object"} onClick={() => void signEnvelope()}>
                  <HicBadge level="HIC-2" style={{ background: "var(--on-accent)", color: "var(--accent)" }} />
                  Approve this envelope
                </button>
              )}
              <span role="status" className="fl-faint" style={{ fontSize: 12 }}>
                {envNote || (env?.on ? "Turning it off needs no card." : "You approve this on a card; Hermes cannot.")}
              </span>
            </div>
          </>
        )}

        {sec === "pay" && (
          <>
            <h1 id="work-h" className="fl-disp" style={{ fontSize: 24 }}>
              Payout address
            </h1>
            <Card style={{ maxWidth: 620 }}>
              <Eyebrow>Your wallet</Eyebrow>
              <span>{p.wallet ? <Hex value={p.wallet} head={10} tail={6} /> : <Unknown reason="no wallet on this device yet" />}</span>
              <span className="fl-muted" style={{ fontSize: 12.5 }}>
                Default. Payouts land in your WorkMarket pull balance, then you withdraw here.
              </span>
            </Card>
            <button type="button" className="btn btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => p.go("groups/rounds/payouts")}>
              Open payouts
            </button>
          </>
        )}
      </section>
    </div>
  );
}

/** Settings › Work as the app routes it: core's stores and the network state. */
export function WorkSettings({ store, s }: { store: import("../shell/store").Store; s: import("../shell/state").AppState }) {
  const { state } = useFlNetworkState();
  return <WorkSettingsView state={state} fl={bridge.flRounds} wallet={s.walletAddr} requestSig={(spec) => store.requestSig(spec)} toast={(m) => store.toast(m)} go={(r) => store.go(r)} />;
}
