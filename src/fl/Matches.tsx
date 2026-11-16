// =====================================================================
// citrate-core: Groups › Rounds › Matches (FL design board 05) with the join ceremony
//
// Ship default is "Today (0.5.4)": the network state model shows no WorkMarket, so the board says
// "Group rounds not live yet", lists the FL contracts with their sources, and offers what a member
// can do now (check an adapter, ask Hermes). The 0.6.0 states (matches with why, the join
// ceremony, the rail) render only when the network state shows WorkMarket deployed, from core's
// offers read model (market.ts). Advanced holds every offer with why it is hidden, the coordinator
// rounds (fl_rounds.rs) and the patronage line (design Q4).
// =====================================================================
import { useEffect, useState, type ReactNode } from "react";
import { encodeFunctionData } from "viem";
import { ModalDialog } from "../shell/ModalDialog";
import type { Authority } from "./authority";
import { CeremonySheet } from "./CeremonySheet";
import { saltExact, saltFromWei } from "./format";
import { hiddenWords, matchOffer, offersCsv, rankMatches, splitOffers, type MachineFacts, type MarketView, type Match, type OfferView } from "./market";
import { PAYOUTS_DARK, NOT_AVAILABLE, ROUNDS_NOT_LIVE, type FlNetworkState } from "./networkState";
import { Card, Chip, Eyebrow, HicBadge, Hex, KV, Note, Unknown } from "./primitives";
import { RoundsNotLive } from "./RoundsNotLive";
import type { WorkerTerms } from "./workerTerms";

export type MarketRead = { kind: "idle" } | { kind: "reading" } | { kind: "read"; view: MarketView } | { kind: "failed"; reason: string };

export const PRIVACY_SENTENCE = "Model updates can reveal some information about the data they were trained on. Only the signed update leaves this machine.";

const ZERO32 = /^0x0{64}$/i;

/** WorkMarket.join(id, minPrice, allowProof): minPrice is the reviewed rate, so it cannot drop. */
export const WORK_MARKET_JOIN_ABI = [
  {
    type: "function",
    name: "join",
    stateMutability: "payable",
    inputs: [
      { name: "id", type: "uint256" },
      { name: "minPrice", type: "uint128" },
      { name: "allowProof", type: "bytes32[]" },
    ],
    outputs: [],
  },
] as const;
export const JOIN_SIGNATURE = "join(uint256,uint128,bytes32[])";

export function joinCalldata(o: OfferView): string {
  return encodeFunctionData({ abi: WORK_MARKET_JOIN_ABI, functionName: "join", args: [BigInt(o.opened.id), BigInt(o.opened.pricePerUnit), []] });
}

/** The exact join the wallet review signs, or an Error naming why there is none (fail closed). */
export function joinIntent(o: OfferView, state: FlNetworkState): { address: string; calldata: string; valueWei: string; label: string } {
  // G-SEC (PAY-10): no FL money selector signs until the profile opens payouts. The core payouts
  // gate (Rust-enforced) replaces this check once its PR lands.
  if (state.payouts !== "open") throw new Error(JOIN_WAITS_FOR_GSEC);
  const wm = state.contracts.WorkMarket;
  if (wm.kind !== "deployed") throw new Error("WorkMarket is not deployed on this network");
  const blocker = joinBlocker(o);
  if (blocker) throw new Error(blocker);
  // PAY-18: msg.value is the join bond read from getOffer, never a default.
  if (!o.terms || !/^\d+$/.test(o.terms.joinBond)) throw new Error("the offer's join bond was not read, so nothing was sent");
  return { address: wm.address, calldata: joinCalldata(o), valueWei: o.terms.joinBond, label: JOIN_SIGNATURE };
}

/** Why the app cannot build this join, or null. */
export function joinBlocker(o: OfferView): string | null {
  if (!o.terms) return "the offer's terms have not been read";
  if (!ZERO32.test(o.terms.allowlistRoot)) return "this round has an allowlist, and the app cannot build its proof yet";
  return null;
}

const dateOf = (secs: number) => new Date(secs * 1000).toISOString().slice(0, 10);
const posterLabel = (o: OfferView) => (ZERO32.test(o.opened.groupHash) ? "Open round" : "Group round");

/** G-SEC: joining posts a SALT join bond, so it opens only with payouts (the profile flag). */
export const JOIN_WAITS_FOR_GSEC = "Joining posts a SALT join bond, so it opens with payouts after the security review.";

export interface MatchesViewProps {
  state: FlNetworkState;
  band: ReactNode;
  market: MarketRead;
  terms: WorkerTerms | null;
  machine: MachineFacts;
  authority: Authority;
  go: (route: string) => void;
  /** Hand the reviewed join to the wallet review (the SignatureCeremony). */
  onJoin: (o: OfferView) => Promise<void>;
  /** The coordinator rounds panel for Advanced (fl_rounds.rs). */
  coordinator: ReactNode;
  save: (name: string, text: string) => Promise<string>;
}

export function MatchesView(p: MatchesViewProps) {
  const [drawer, setDrawer] = useState<"" | "adv" | { offer: Match }>("");
  const [join, setJoin] = useState<{ m: Match; step: "review" | "signing"; error?: string } | null>(null);
  const live = p.state.phase === "rounds-live";
  const read = p.market.kind === "read" ? p.market.view : null;
  const { shown } = read ? splitOffers(read) : { shown: [] as OfferView[] };
  const matches = rankMatches(shown.map((o) => matchOffer(o, p.terms, p.machine)));

  const doSign = async () => {
    if (!join) return;
    setJoin({ ...join, step: "signing" });
    try {
      // The wallet review is the SignatureCeremony: it signs, sends and reports the outcome in the
      // truth contract's words. This sheet steps aside once the review is open.
      await p.onJoin(join.m.offer);
      setJoin(null);
    } catch (e) {
      setJoin({ ...join, step: "review", error: e instanceof Error ? e.message : String(e) });
    }
  };

  return (
    <>
      {p.band}
      {!live ? (
        <RoundsNotLive
          testid="matches-today"
          state={p.state}
          headline="Rounds open on this network after the WorkMarket contract is deployed."
          lead={<p style={{ margin: 0 }}>Check whether this machine can take part, and check an adapter file against the eval gate.</p>}
          actions={
            <>
              <button type="button" className="btn btn-primary" onClick={() => p.go("groups/rounds/adapters")}>
                Check an adapter file
              </button>
              <button type="button" className="btn btn-ghost" onClick={() => p.go("hermes/chat")}>
                Ask Hermes
              </button>
              <button type="button" className="btn btn-ghost" onClick={() => setDrawer("adv")}>
                Advanced
              </button>
            </>
          }
        />
      ) : (
        <div className="fl-cols" data-testid="matches-live">
          <section className="fl-main" aria-labelledby="matches-h">
            <div style={{ display: "flex", alignItems: "baseline", gap: 10, flexWrap: "wrap" }}>
              <h1 id="matches-h" className="fl-disp" style={{ fontSize: 22 }}>
                Matches for you
              </h1>
              <span className="fl-muted" style={{ fontSize: 12.5 }}>
                {p.terms ? `Checked against your terms: floor ${p.terms.floorSaltPerUnit} SALT${p.terms.dataClasses.length ? ", " + p.terms.dataClasses.join(", ") + " data" : ""}.` : "You have not set terms yet."}
              </span>
              <span style={{ flex: 1 }} />
              <button type="button" className="btn btn-sm btn-ghost" onClick={() => setDrawer("adv")}>
                Advanced
              </button>
            </div>
            {p.market.kind === "failed" && (
              <Card testid="matches-unread" style={{ borderColor: "var(--warn)" }}>
                <span style={{ fontSize: 13 }}>
                  Offers: <Unknown reason={p.market.reason} />
                </span>
              </Card>
            )}
            {(p.market.kind === "reading" || p.market.kind === "idle") && <p className="fl-muted" style={{ margin: 0, fontSize: 13 }}>Reading offers from WorkMarket.</p>}
            {read && !read.available && (
              <Card>
                <span style={{ fontSize: 13 }}>Offers: <Unknown reason={read.reason ?? "the market is not available"} /></span>
              </Card>
            )}
            {read && read.available && matches.length === 0 && (
              <Card testid="matches-empty">
                <span style={{ fontSize: 14, fontWeight: 500 }}>No open rounds right now</span>
                <span className="fl-muted" style={{ fontSize: 12.5 }}>New offers appear here once they are twelve blocks deep. Advanced lists every offer and why it is not shown.</span>
              </Card>
            )}
            {matches.map((m) => (
              <MatchCard key={m.offer.opened.id} m={m} moneyOpen={p.state.payouts === "open"} onJoin={() => setJoin({ m, step: "review" })} onDetails={() => setDrawer({ offer: m })} />
            ))}
          </section>
          <aside className="fl-rail" aria-label="Your work">
            <Card>
              <Eyebrow>Your round</Eyebrow>
              <span style={{ fontSize: 12.5 }}>You are not in a round on this device.</span>
              <button type="button" className="btn btn-sm btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => p.go("groups/rounds/live")}>
                Live round
              </button>
            </Card>
            <Card>
              <Eyebrow>This machine</Eyebrow>
              <span style={{ fontSize: 12.5 }}>{p.machine.memoryGb === null ? <Unknown reason="GPU memory not read" /> : `up to ${p.machine.memoryGb} GB ${p.machine.memoryKind}`}</span>
              <button type="button" className="btn btn-sm btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => p.go("groups/cluster")}>
                Eligibility details
              </button>
            </Card>
            <Card>
              <Eyebrow>Your terms</Eyebrow>
              <span style={{ fontSize: 12.5 }}>{p.terms ? `Floor ${p.terms.floorSaltPerUnit} SALT · ${p.terms.hours === "any" ? "any time" : p.terms.hours.start + " to " + p.terms.hours.end}${p.terms.mainsOnly ? " · mains only" : ""}` : "No terms yet"}</span>
              <span className="fl-muted" style={{ fontSize: 12 }}>
                Authority: {p.authority.level === "HIC-1" ? "HIC-1, each join asks you" : `HIC-2 envelope "${p.authority.name}"`}
              </span>
            </Card>
            <Card>
              <Eyebrow>Payouts</Eyebrow>
              <span style={{ fontSize: 12.5 }}>{p.state.payouts === "open" ? "Payouts are open on this network" : p.state.payouts === "not-deployed" ? NOT_AVAILABLE : PAYOUTS_DARK}</span>
              <button type="button" className="btn btn-sm btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => p.go("groups/rounds/payouts")}>
                Receipts and history
              </button>
            </Card>
          </aside>
        </div>
      )}

      {drawer !== "" && (
        <ModalDialog
          labelledBy="fl-drawer-h"
          focusKey={typeof drawer === "string" ? drawer : "offer-" + drawer.offer.offer.opened.id}
          register="instrument"
          zIndex={55}
          onEscape={() => setDrawer("")}
          panelStyle={{ position: "fixed", top: 0, right: 0, bottom: 0, width: "min(560px, 100%)", background: "var(--srf-1)", color: "var(--tx-1)", borderLeft: "1px solid var(--line-2)", display: "flex", flexDirection: "column" }}
        >
          <div style={{ flex: "none", display: "flex", alignItems: "center", padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
            <h2 id="fl-drawer-h" style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>
              {drawer === "adv" ? "Advanced · all offers" : `Offer ${drawer.offer.offer.opened.id}`}
            </h2>
            <span style={{ flex: 1 }} />
            <button type="button" className="btn btn-sm btn-ghost" data-autofocus onClick={() => setDrawer("")}>
              Close
            </button>
          </div>
          <div style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 14, overflow: "auto" }}>
            {drawer === "adv" ? (
              <AdvancedPanel market={p.market} live={live} coordinator={p.coordinator} save={p.save} />
            ) : (
              <OfferDetails m={drawer.offer} moneyOpen={p.state.payouts === "open"} onJoin={() => {
                const m = drawer.offer;
                setDrawer("");
                setJoin({ m, step: "review" });
              }} />
            )}
          </div>
        </ModalDialog>
      )}

      {join && (
        <CeremonySheet
          id="fl-join"
          title={`Join round ${join.m.offer.opened.id}`}
          hic={p.authority.level}
          step={join.step}
          signed={1}
          signLabel="Sign and join"
          onSign={() => void doSign()}
          onDecline={() => setJoin(null)}
          review={<JoinReview m={join.m} authority={p.authority} error={join.error} />}
        />
      )}
    </>
  );
}

function MatchCard({ m, onJoin, onDetails, moneyOpen }: { m: Match; onJoin: () => void; onDetails: () => void; moneyOpen: boolean }) {
  const o = m.offer;
  const req = o.requirements.status === "verified" ? o.requirements.requirements : null;
  const blocker = joinBlocker(o) ?? (moneyOpen ? null : JOIN_WAITS_FOR_GSEC.replace(/\.$/, "").replace(/^J/, "j"));
  return (
    <article className="fl-card" data-testid="match-card" data-offer={o.opened.id} data-joinable={m.joinable && !blocker} aria-label={`Offer ${o.opened.id}, ${m.chip}`} style={{ padding: "12px 14px", display: "grid", gridTemplateColumns: "minmax(0,1.3fr) minmax(0,1fr) auto", gap: "6px 18px", alignItems: "start" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }}>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          <Eyebrow>
            {posterLabel(o)} · <Hex value={o.opened.poster} />
          </Eyebrow>
          <Chip tone={m.tone}>{m.chip}</Chip>
        </div>
        <h2 style={{ margin: 0, fontSize: 14.5, fontWeight: 500 }}>Round {o.opened.id}{req ? `: ${req.dataClasses.join(", ") || "no data class"}` : ""}</h2>
        <p className="fl-muted" style={{ margin: 0, fontSize: 12, lineHeight: 1.45 }}>
          <b style={{ fontWeight: 500, color: "var(--tx-1)" }}>{m.whyLabel}</b> {m.why}
          {m.joinable && blocker ? ` Can't join from the app: ${blocker}.` : ""}
        </p>
      </div>
      <KV
        rows={[
          { k: "Rate", v: `${saltFromWei(BigInt(o.opened.pricePerUnit))} SALT per unit`, mono: true },
          { k: "This machine", v: req ? `about ${req.estHours} h` : <Unknown reason="requirements not read" /> },
          { k: "Data", v: req ? `${req.dataClasses.join(", ") || "none"}, stays here` : <Unknown reason="requirements not read" /> },
          { k: "Deadline", v: dateOf(o.opened.deadline), mono: true },
        ]}
      />
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {m.joinable && !blocker ? (
          <button type="button" className="btn btn-primary" onClick={onJoin}>
            Join
          </button>
        ) : (
          <button type="button" className="btn" disabled>
            Can't join
          </button>
        )}
        <button type="button" className="btn btn-ghost btn-sm" onClick={onDetails}>
          Details
        </button>
      </div>
    </article>
  );
}

function OfferDetails({ m, onJoin, moneyOpen }: { m: Match; onJoin: () => void; moneyOpen: boolean }) {
  const o = m.offer;
  const req = o.requirements.status === "verified" ? o.requirements.requirements : null;
  const blocker = joinBlocker(o) ?? (moneyOpen ? null : JOIN_WAITS_FOR_GSEC);
  return (
    <>
      <Eyebrow>{posterLabel(o)}</Eyebrow>
      <KV
        rows={[
          { k: "Offer id", v: String(o.opened.id), mono: true },
          { k: "Poster", v: <Hex value={o.opened.poster} /> },
          { k: "Rate", v: `${saltFromWei(BigInt(o.opened.pricePerUnit))} SALT per unit`, mono: true },
          { k: "Escrow", v: `${saltFromWei(BigInt(o.opened.budget))} SALT`, mono: true },
          { k: "Min memory", v: req ? `${req.minVramGb} GB` : <Unknown reason="requirements not read" /> },
          { k: "Data classes", v: req ? req.dataClasses.join(", ") || "none" : <Unknown reason="requirements not read" /> },
          { k: "Architectures", v: req ? req.architectures.join(", ") || "any" : <Unknown reason="requirements not read" /> },
          { k: "Workers", v: o.terms ? `${o.terms.workers} of ${o.terms.maxWorkers}` : <Unknown reason="terms not read" /> },
          { k: "Coordinator", v: o.terms ? <Hex value={o.terms.coordinator} /> : <Unknown reason="terms not read" /> },
          { k: "requirementsCid", v: `${o.opened.requirementsCid}${o.requirements.status === "verified" ? " · hash checked" : ""}`, mono: true },
        ]}
      />
      <pre className="fl-inset mono" style={{ margin: 0, padding: 10, fontSize: 11, color: "var(--tx-2)", overflow: "auto" }}>
        {JSON.stringify({ opened: o.opened, terms: o.terms, requirements: o.requirements }, null, 1)}
      </pre>
      {m.joinable && !blocker && (
        <button type="button" className="btn btn-primary" style={{ alignSelf: "flex-start" }} onClick={onJoin}>
          Join this round
        </button>
      )}
    </>
  );
}

function AdvancedPanel({ market, live, coordinator, save }: { market: MarketRead; live: boolean; coordinator: ReactNode; save: MatchesViewProps["save"] }) {
  const [saved, setSaved] = useState("");
  const read = market.kind === "read" ? market.view : null;
  return (
    <>
      {!live ? (
        <p style={{ margin: 0, fontSize: 13 }}>
          {ROUNDS_NOT_LIVE}. WorkMarket is not on this network, so there are no offers to list.
        </p>
      ) : read ? (
        <>
          <table className="fl-tbl">
            <thead>
              <tr>
                <th style={{ width: "14%" }}>Offer</th>
                <th style={{ width: "30%" }}>Poster</th>
                <th>Why not shown or joinable</th>
              </tr>
            </thead>
            <tbody>
              {read.offers.map((o) => (
                <tr key={o.opened.id} data-offer={o.opened.id}>
                  <td className="mono">{o.opened.id}</td>
                  <td>
                    <Hex value={o.opened.poster} />
                  </td>
                  <td className="fl-muted">{o.hidden ? "hidden · " + hiddenWords(o.hidden) : "shown"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
            <button type="button" className="btn btn-sm btn-ghost" onClick={() => void save("offers.csv", offersCsv(read)).then(setSaved, (e) => setSaved("Couldn't save: " + (e instanceof Error ? e.message : String(e))))}>
              Export as CSV
            </button>
            <span className="fl-faint" style={{ fontSize: 11.5 }} role="status">
              {saved}
            </span>
          </div>
        </>
      ) : (
        <p style={{ margin: 0, fontSize: 13 }}>
          Offers: <Unknown reason={market.kind === "failed" ? market.reason : "not read yet"} />
        </p>
      )}
      <KV
        rows={[
          { k: "Patronage units", v: <>Recorded for the cooperative's books; payment is WorkMarket's. <Unknown reason="no operator holds the settler role yet" /></> },
        ]}
      />
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <Eyebrow>Coordinator rounds</Eyebrow>
        <Note>A coordinator an operator runs can plan a round with you directly. You approve every start (HIC-1).</Note>
        {coordinator}
      </div>
    </>
  );
}

function JoinReview({ m, authority, error }: { m: Match; authority: Authority; error?: string }) {
  const o = m.offer;
  const req = o.requirements.status === "verified" ? o.requirements.requirements : null;
  return (
    <>
      {authority.level === "HIC-2" && (
        <div className="fl-card" style={{ padding: "8px 12px", borderColor: "var(--warn)", background: "var(--warn-bg)", fontSize: 12.5 }}>
          <b style={{ fontWeight: 600 }}>Your envelope is on, and this needs your tap.</b> The first join with any new group or poster always asks you.
        </div>
      )}
      <p className="fl-muted" style={{ margin: 0, fontSize: 13 }}>
        This is exactly what you sign.
      </p>
      <KV
        style={{ fontSize: 13 }}
        rows={[
          { k: "Offer", v: `WorkMarket offer ${o.opened.id}`, mono: true },
          { k: "Rate", v: `${saltExact(BigInt(o.opened.pricePerUnit))} SALT per unit (${o.opened.pricePerUnit} wei). The join refuses a lower rate.` },
          { k: "Data read", v: req ? `${req.dataClasses.join(", ") || "none"} on this device. It is never uploaded.` : <Unknown reason="requirements not read" /> },
          { k: "Uploaded", v: "One signed model update" },
          { k: "Join bond", v: o.terms ? `${saltExact(BigInt(o.terms.joinBond))} SALT (${o.terms.joinBond} wei), returned when you settle` : <Unknown reason="terms not read" />, mono: true },
          { k: "Network fee", v: <Unknown reason="the wallet review reads gas before you sign" /> },
        ]}
      />
      <div className="fl-inset" style={{ padding: "10px 12px", fontSize: 12.5, lineHeight: 1.5 }}>
        {PRIVACY_SENTENCE}
      </div>
      <Note>No training starts from the app until the training worker ships. Joining reserves your place.</Note>
      <span className="fl-faint" style={{ fontSize: 11.5, display: "flex", gap: 6, alignItems: "center" }}>
        <HicBadge level="HIC-1" /> Signing opens the wallet review, where you approve on this device.
      </span>
      {error && (
        <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--danger)" }}>
          Not sent: {error}
        </p>
      )}
    </>
  );
}

/** Read offers once the network state shows WorkMarket. */
export function useMarket(state: FlNetworkState, read: () => Promise<MarketView>): MarketRead {
  const [m, setM] = useState<MarketRead>({ kind: "idle" });
  const live = state.phase === "rounds-live";
  useEffect(() => {
    if (!live) return;
    let on = true;
    setM({ kind: "reading" });
    read().then(
      (view) => on && setM({ kind: "read", view }),
      (e) => on && setM({ kind: "failed", reason: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      on = false;
    };
  }, [live, read]);
  return m;
}
