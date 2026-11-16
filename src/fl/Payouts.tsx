// =====================================================================
// citrate-core: Groups › Rounds › Payouts and history (FL design board 10)
//
// Receipts: what WorkMarket escrowed and paid, in SALT, never projected. Money ships dark: with
// WorkMarket absent (r1007) the board says "Not available yet on this network"; with WorkMarket
// deployed and payouts not opened by the network profile (G-SEC) it says "Payouts open after the
// security review" and nothing can be signed. Claim, withdraw and dispute render only when the
// profile opens payouts and a settled line is read (WorkMarket.settled + receipt), with each as a
// HIC-1 ceremony.
//
// History: the records on this device (core's fl_rounds.rs: coordinator-round approvals and
// withdrawals, eval gate decisions, the loaded adapter), with filters and CSV export. Nothing here
// signs.
// =====================================================================
import { useEffect, useState } from "react";
import type { FlOverview, FlRoundsDomain } from "../bridge/domains";
import { CeremonySheet } from "./CeremonySheet";
import { saltFromWei } from "./format";
import { NOT_AVAILABLE, PAYOUTS_DARK, type FlNetworkState } from "./networkState";
import { PayoutReceipt, type PayoutLine, type PayoutStatus } from "./PayoutReceipt";
import { Card, Chip, Eyebrow, Hex, KV, Note, shortHex, Unknown } from "./primitives";
import { TOKENOMICS_NOTICE } from "./FlBand";
import { windowWords } from "./chainTime";

export type HistoryKind = "Round" | "Consent" | "Gate" | "Adapter" | "Payout";

export interface HistoryRow {
  at: number;
  kind: HistoryKind;
  what: string;
  record: string;
}

const when = (ms: number) => new Date(ms).toISOString().slice(0, 16).replace("T", " ");

/** The device's FL records as history rows, newest first. */
export function historyRows(ov: FlOverview): HistoryRow[] {
  const out: HistoryRow[] = [];
  for (const s of ov.starts) {
    out.push({ at: s.authorizedAtMs, kind: "Round", what: `Approved a coordinator round plan (HIC-1)${s.roundId ? ` for round ${shortHex(s.roundId)}` : ""}`, record: "plan " + shortHex(s.planHash, 10, 4) });
    if (s.roundId) out.push({ at: s.authorizedAtMs, kind: "Consent", what: `Consented to round ${shortHex(s.roundId)} on this device`, record: "fl consent file" });
    if (s.revokedAtMs) out.push({ at: s.revokedAtMs, kind: "Consent", what: `Withdrew consent${s.roundId ? ` for round ${shortHex(s.roundId)}` : ""}`, record: "fl consent file" });
  }
  for (const g of ov.gates) {
    const d = g.decision;
    const delta = d.compositeCandidate - d.compositeBase;
    out.push({ at: g.decidedAtMs, kind: "Gate", what: d.verdict === "ACCEPT" ? `ACCEPT · no metric worse, mean ${delta >= 0 ? "+" : ""}${delta.toFixed(3)}` : `REJECT · ${d.reasons[0] ?? "the gate refused it"}`, record: "sha " + shortHex(g.adapterSha256, 8, 4) });
  }
  return out.sort((a, b) => b.at - a.at);
}

export const FILTERS: [string, string][] = [
  ["all", "All"],
  ["Round", "Rounds"],
  ["Consent", "Consent"],
  ["Gate", "Gate"],
  ["Payout", "Payouts"],
];

export function filterRows(rows: HistoryRow[], f: string): HistoryRow[] {
  return rows.filter((h) => f === "all" || h.kind === f || (f === "Gate" && h.kind === "Adapter"));
}

export function historyCsv(rows: HistoryRow[]): string {
  const esc = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
  return ["when,kind,what,record", ...rows.map((r) => [when(r.at), r.kind, r.what, r.record].map(esc).join(","))].join("\n") + "\n";
}

/** A settled line as WorkMarket reports it, with what may be done with it. */
export interface Receipt {
  line: PayoutLine;
  status: Exclude<PayoutStatus, "dark" | "not-deployed">;
  /** When the dispute window ends, in chain seconds: the later of payoutPostedAt + disputeWindow
   *  and answerGraceUntil (WorkMarket getOffer). */
  disputeEndsAt?: number;
}

export interface PayoutsViewProps {
  state: FlNetworkState;
  /** WorkMarket settled lines for this member; null when not read (no read model in this build). */
  receipts: Receipt[] | null;
  receiptsReason?: string;
  /** WorkMarket pull balance in wei; null when not read. */
  pullWei: bigint | null;
  pullReason?: string;
  wallet: string;
  /** The latest block's timestamp (seconds); null when not read. */
  chainNow: number | null;
  ov: FlOverview | null;
  ovError: string | null;
  go: (route: string) => void;
  save: (name: string, text: string) => Promise<string>;
  /** Hand a claim, withdraw or dispute to the wallet review. Only reachable when payouts are open. */
  act: (kind: "claim" | "withdraw" | "dispute", r?: Receipt) => Promise<void>;
}

export function PayoutsView(p: PayoutsViewProps) {
  const [pane, setPane] = useState<"receipts" | "history">("receipts");
  const [filter, setFilter] = useState("all");
  const [saved, setSaved] = useState("");
  const [sheet, setSheet] = useState<{ kind: "claim" | "withdraw" | "dispute"; r?: Receipt; error?: string; signing?: boolean } | null>(null);
  const open = p.state.payouts === "open";
  const nd = p.state.payouts === "not-deployed";
  const rows = p.ov ? historyRows(p.ov) : [];
  const shown = filterRows(rows, filter);

  const sign = async () => {
    // PAY-15: one hand-off per review. The sheet leaves the review step at once, so a second
    // press has no button; the store refuses a second proposal while one is in flight.
    if (!sheet || sheet.signing) return;
    const s0 = sheet;
    setSheet({ ...s0, signing: true, error: undefined });
    try {
      await p.act(s0.kind, s0.r);
      setSheet(null);
    } catch (e) {
      setSheet({ ...s0, signing: false, error: e instanceof Error ? e.message : String(e) });
    }
  };

  return (
    <div data-testid="payouts" data-payouts={p.state.payouts} style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
      <div style={{ padding: "18px 26px 0", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 12, flexWrap: "wrap" }}>
          <h1 className="fl-disp" style={{ fontSize: 24 }}>
            Payouts
          </h1>
          <span className="fl-muted" style={{ fontSize: 13 }}>
            SALT the WorkMarket contract escrowed and paid, as the contract reports it.
          </span>
          <span style={{ flex: 1 }} />
          <span className="fl-faint" style={{ fontSize: 11.5 }}>
            {TOKENOMICS_NOTICE}
          </span>
        </div>
        <div role="tablist" aria-label="Payouts" style={{ display: "flex", gap: 4, borderBottom: "1px solid var(--line-1)" }}>
          {(
            [
              ["receipts", "Receipts"],
              ["history", "History"],
            ] as const
          ).map(([id, label]) => (
            <button key={id} type="button" role="tab" aria-selected={pane === id} className="btn btn-sm btn-ghost" style={{ border: "none", borderBottom: pane === id ? "2px solid var(--accent)" : "2px solid transparent", borderRadius: 0 }} onClick={() => setPane(id)}>
              {label}
            </button>
          ))}
        </div>
      </div>

      {pane === "receipts" ? (
        <div className="fl-cols">
          <section className="fl-main" aria-label="Receipts">
            {nd && (
              <Card testid="payouts-nd">
                <Chip tone="warn" style={{ alignSelf: "flex-start" }}>
                  {NOT_AVAILABLE}
                </Chip>
                <span className="fl-muted" style={{ fontSize: 12.5 }}>
                  WorkMarket is not deployed on this network, so there are no payouts to show.
                </span>
              </Card>
            )}
            {p.state.payouts === "dark" && (
              <Card testid="payouts-dark">
                <Chip style={{ alignSelf: "flex-start" }}>{PAYOUTS_DARK}</Chip>
                <span className="fl-muted" style={{ fontSize: 12.5 }}>
                  Claim turns on with a network profile update after the security review. Nothing here can be signed yet.
                </span>
                {p.state.payoutsReason && (
                  <span className="fl-faint" style={{ fontSize: 12 }} data-testid="payouts-reason">
                    {p.state.payoutsReason}
                  </span>
                )}
              </Card>
            )}
            {p.state.payouts === "unknown" && (
              <Card>
                <span style={{ fontSize: 13 }}>
                  Payouts: <Unknown reason={p.state.phaseReason ?? "the market read failed"} />
                </span>
              </Card>
            )}
            {!nd &&
              (p.receipts === null ? (
                <span style={{ fontSize: 13 }}>
                  Receipts: <Unknown reason={p.receiptsReason ?? "this build has no settled-lines read model yet"} />
                </span>
              ) : p.receipts.length === 0 ? (
                <PayoutReceipt status="nothing" />
              ) : (
                p.receipts.map((r) => (
                  <PayoutReceipt
                    key={r.line.round}
                    status={open ? r.status : "dark"}
                    line={r.line}
                    actions={
                      open ? (
                        <>
                          {r.status === "claimable" && (
                            <button type="button" className="btn btn-primary btn-sm" onClick={() => setSheet({ kind: "claim", r })}>
                              Claim
                            </button>
                          )}
                          {r.status === "waiting" && (
                            <button type="button" className="btn btn-danger btn-sm" onClick={() => setSheet({ kind: "dispute", r })}>
                              Dispute
                            </button>
                          )}
                        </>
                      ) : undefined
                    }
                  />
                ))
              ))}
          </section>
          <aside className="fl-rail" aria-label="Balance">
            <Card raised>
              <Eyebrow>Claimed, ready to withdraw</Eyebrow>
              <span className="mono" style={{ fontSize: 15 }}>
                {nd ? <Unknown reason="WorkMarket is not on this network" /> : p.pullWei === null ? <Unknown reason={p.pullReason ?? "the WorkMarket pull balance was not read"} /> : `${saltFromWei(p.pullWei)} SALT`}
              </span>
              <button type="button" className="btn btn-sm btn-ghost" disabled={!open || p.pullWei === null || p.pullWei <= 0n} onClick={() => setSheet({ kind: "withdraw" })}>
                Withdraw to wallet
              </button>
            </Card>
            <Note>Auto-claim after each dispute window runs only inside a signed envelope, and only once payouts open. It is off.</Note>
            <Card raised>
              <Eyebrow>Payout address</Eyebrow>
              <span>{p.wallet ? <Hex value={p.wallet} /> : <Unknown reason="no wallet on this device yet" />}</span>
              <span className="fl-muted" style={{ fontSize: 12 }}>
                Payouts land in your WorkMarket pull balance, then you withdraw to this wallet.
              </span>
            </Card>
          </aside>
        </div>
      ) : (
        <section className="fl-main" aria-label="History">
          <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
            <div role="radiogroup" aria-label="Filter" style={{ display: "flex", gap: 4 }}>
              {FILTERS.map(([id, label]) => (
                <button key={id} type="button" role="radio" aria-checked={filter === id} className={filter === id ? "btn btn-sm btn-secondary" : "btn btn-sm btn-ghost"} onClick={() => setFilter(id)}>
                  {label}
                </button>
              ))}
            </div>
            <span style={{ flex: 1 }} />
            <button type="button" className="btn btn-sm btn-ghost" disabled={shown.length === 0} onClick={() => void p.save("citrate-fl-history.csv", historyCsv(shown)).then(setSaved, (e) => setSaved("Couldn't save: " + (e instanceof Error ? e.message : String(e))))}>
              Export as CSV
            </button>
            <span role="status" className="fl-faint" style={{ fontSize: 11.5 }}>
              {saved}
            </span>
          </div>
          {p.ovError && (
            <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--warn)" }}>
              Couldn't read the records on this device: {p.ovError}
            </p>
          )}
          {shown.length === 0 ? (
            <p className="fl-muted" style={{ margin: 0, fontSize: 13 }} data-testid="history-empty">
              No records on this device yet.
            </p>
          ) : (
            <table className="fl-tbl fl-card" data-testid="history">
              <thead>
                <tr>
                  <th style={{ width: "18%" }}>When</th>
                  <th style={{ width: "12%" }}>Kind</th>
                  <th>What happened</th>
                  <th style={{ width: "22%" }}>Record</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((h, i) => (
                  <tr key={i} data-kind={h.kind}>
                    <td className="mono">{when(h.at)}</td>
                    <td>
                      <Chip>{h.kind}</Chip>
                    </td>
                    <td>{h.what}</td>
                    <td className="mono fl-faint">{h.record}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <Note>Records kept on this device by core (coordinator rounds, consent, gate decisions). Nothing here signs.</Note>
        </section>
      )}

      {sheet && (
        <CeremonySheet
          id="fl-pay"
          title={sheet.kind === "claim" ? `Claim round ${sheet.r?.line.round}` : sheet.kind === "withdraw" ? "Withdraw to your wallet" : `Dispute the round ${sheet.r?.line.round} payout list`}
          hic="HIC-1"
          step={sheet.signing ? "signing" : "review"}
          signed={1}
          signLabel="Sign"
          onSign={() => void sign()}
          onDecline={() => setSheet(null)}
          review={
            <>
              {sheet.kind === "claim" && sheet.r && (
                <KV
                  rows={[
                    { k: "Round", v: sheet.r.line.round },
                    { k: "Amount", v: `${sheet.r.line.units} units × ${saltFromWei(sheet.r.line.rateWei)} SALT = ${saltFromWei(sheet.r.line.amountWei)} SALT`, mono: true },
                    { k: "Goes to", v: "Your WorkMarket pull balance, with your join bond released, then withdraw to your wallet" },
                    { k: "Network fee", v: <Unknown reason="the wallet review reads gas before you sign" /> },
                  ]}
                />
              )}
              {sheet.kind === "withdraw" && (
                <KV
                  rows={[
                    { k: "Amount", v: p.pullWei !== null ? `${saltFromWei(p.pullWei)} SALT` : <Unknown reason="not read" />, mono: true },
                    { k: "To", v: <Hex value={p.wallet} /> },
                    { k: "Network fee", v: <Unknown reason="the wallet review reads gas before you sign" /> },
                  ]}
                />
              )}
              {sheet.kind === "dispute" && (
                <KV
                  rows={[
                    { k: "Effect", v: "The round's payout freezes. If your receipt holds, the coordinator bond can be slashed. A dispute pays the disputer nothing." },
                    { k: "Window", v: sheet.r?.disputeEndsAt !== undefined ? windowWords(sheet.r.disputeEndsAt, p.chainNow) : <Unknown reason="not read" /> },
                  ]}
                />
              )}
              {sheet.error && (
                <p role="alert" style={{ margin: 0, fontSize: 12.5, color: "var(--danger)" }}>
                  Not sent: {sheet.error}
                </p>
              )}
            </>
          }
        />
      )}
    </div>
  );
}

/** The route: real network state and device records; no settled-lines read model in this build. */
export function usePayoutsRecords(fl: FlRoundsDomain): { ov: FlOverview | null; error: string | null } {
  const [ov, setOv] = useState<FlOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    fl.overview().then(
      (o) => {
        setOv(o);
        setError(o.storeError);
      },
      (e) => setError(e instanceof Error ? e.message : String(e)),
    );
  }, [fl]);
  return { ov, error };
}
