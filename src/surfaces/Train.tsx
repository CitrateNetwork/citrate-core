// =====================================================================
// citrate-core — Train together (CX-S5 redesign, Pass 1)
//
// A group's federated-training round, built 1:1 from design/CitrateCore.dc.html. SETL-S3 is landed:
// status + reward are REAL eth_call reads of PatronageLedger on 40204 (round/phase + the member's
// weight + claimable SALT). There is no write here: V5 (0.5.4) removed the contribute and claim
// stubs, which always failed (recordContribution is SETTLER-only and member SALT crediting is gated
// on G-SEC). Joining and claiming are the WorkMarket round tools, dark until those contracts are
// deployed and payouts open; this card never fabricates a round, balance, or reward (Rule 1).
//
// C1-V V11 (UI truth contract §1, §4): "Group rounds not live yet" is the required wording while
// rounds can't run, in BOTH ledger states: a ledger absent from the book, or both deployed with no
// round able to run. #653 (0.5.4): the book now pins both (r1007 block 0), so the card names the
// deployed state. Rounds still can't run (ROUND_SETTLEMENT_WIRED below), so the surface makes no
// ledger read at all.
// =====================================================================
import { useEffect, useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import type { Group, RewardInfo, RoundPhase, RoundStatus } from "../bridge/domains";
import { FlRoundsPanel } from "../fl/FlRoundsPanel";
import book from "../../src-tauri/addresses/40204.json";
import { CHAIN_ID_TEXT } from "../network/profile";

const ZERO_ADDRESS = /^0x0{40}$/i;
const isDeployed = (a: string | undefined): boolean => !!a && /^0x[0-9a-fA-F]{40}$/.test(a) && !ZERO_ADDRESS.test(a);

const ROUND_LEDGER_NAMES = ["PatronageLedger", "FederatedRoundLedger"] as const;

/** The round ledgers the bundled address book does not pin (empty when it pins both). */
export function missingRoundLedgers(addresses: Record<string, string | undefined>): string[] {
  return ROUND_LEDGER_NAMES.filter((n) => !isDeployed(addresses[n]));
}

/**
 * Whether a group round can be recorded on 40204. PatronageLedger.commitRound and
 * recordContribution are SETTLER_ROLE-only, and on r1007 no account holds SETTLER_ROLE, COOP_ROLE
 * or LEDGER_YEAR_KEEPER_ROLE (the governance timelock is only DEFAULT_ADMIN). The grant is a
 * governance batch through the 48 h timelock (0.6.0 planset 06 §1). Until a release ships with that
 * grant made and the truth contract's sign-off, rounds can't run, so this stays false.
 */
export const ROUND_SETTLEMENT_WIRED = false;

/** Group rounds are live only when BOTH round contracts are in the bundled address book AND
 *  settlement can record a round on them (ROUND_SETTLEMENT_WIRED). */
export function groupRoundsLive(addresses: Record<string, string | undefined>): boolean {
  return ROUND_SETTLEMENT_WIRED && missingRoundLedgers(addresses).length === 0;
}

const BOOK_ADDRESSES = book.addresses as Record<string, string | undefined>;
const ROUNDS_LIVE = groupRoundsLive(BOOK_ADDRESSES);
const MISSING_LEDGERS = missingRoundLedgers(BOOK_ADDRESSES);

// The round lifecycle as a person reads it, mapped from the domain's RoundPhase.
const PHASE_UI: Record<RoundPhase, { label: string; tone: string; blurb: string }> = {
  idle: { label: "No round open", tone: "var(--tx-3)", blurb: "No training round is open for this group right now. When one opens you can opt in — you're never auto-enrolled." },
  open: { label: "Round open", tone: "var(--ok)", blurb: "A round is open. Contributing compute and data is explicit, and counted once per member." },
  aggregating: { label: "Settling", tone: "var(--warn)", blurb: "The barrier sealed the cutoff and the network is aggregating contributions. No new contributions are counted for this round." },
  committed: { label: "Claimable", tone: "var(--accent-text)", blurb: "The round settled. Your reward is claimable — the claim stops at your ceremony as an unsigned settlement intent." },
  settled: { label: "Claimed", tone: "var(--tx-2)", blurb: "This round is fully settled. Rewards were paid to contributors, each contribution counted exactly once." },
};

function shortLabel(g: Group): string {
  return g.name || `${g.id.slice(0, 10)}…`;
}

export function Train({ store }: SurfaceProps) {
  const [groups, setGroups] = useState<Group[]>([]);
  const [groupId, setGroupId] = useState<string>("");
  const [status, setStatus] = useState<RoundStatus | null>(null);
  const [reward, setReward] = useState<RewardInfo | null>(null);
  const [pending, setPending] = useState<string | null>(null); // honest "backend not wired" text

  useEffect(() => {
    if (!ROUNDS_LIVE) return; // nothing to read: the round contracts are not deployed
    void (async () => {
      try {
        const gs = await bridge.groups.list();
        setGroups(gs);
        if (gs.length && !groupId) setGroupId(gs[0].id);
      } catch {
        /* honest-empty: no groups / un-provisioned */
      }
    })();
  }, []);

  const load = async (gid: string) => {
    setStatus(null);
    setReward(null);
    setPending(null);
    try {
      const s = await bridge.training.status(gid);
      setStatus(s);
      if (s.phase === "committed") {
        try {
          setReward(await bridge.training.reward(gid));
        } catch {
          /* reward not available yet — leave null */
        }
      }
    } catch (e) {
      // A read failure (RPC down / address unresolved) — say so plainly, never fabricate a round.
      setPending(e instanceof Error ? e.message : "Couldn't reach 40204 to read the round.");
    }
  };

  useEffect(() => {
    if (ROUNDS_LIVE && groupId) void load(groupId);
  }, [groupId]);

  const phase = status?.phase;
  const ui = phase ? PHASE_UI[phase] : null;

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 900 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Train together</span>
        <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid var(--warn)", color: "var(--warn)", background: "var(--warn-bg)", flexShrink: 0 }}>
          {ROUNDS_LIVE ? "reads live · claims @rule8-gated" : "not live yet"}
        </span>
        {ROUNDS_LIVE && <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>settled through the ceremony · never double-paid</span>}
      </div>

      {!ROUNDS_LIVE ? (
        <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ fontSize: 14, fontWeight: 500 }}>Group rounds not live yet</span>
          {MISSING_LEDGERS.length === 0 ? (
            <p style={{ fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.6, margin: 0, maxWidth: 660 }}>
              Both ledgers group rounds need, PatronageLedger and FederatedRoundLedger, are deployed on {CHAIN_ID_TEXT}, but no round can be recorded on them yet.
              Rounds, contributions and claims appear here once they can. Nothing on this card is read from a round in the meantime.
            </p>
          ) : (
            <p style={{ fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.6, margin: 0, maxWidth: 660 }}>
              Group training rounds need two contracts, PatronageLedger and FederatedRoundLedger, and this build's address book does not pin {MISSING_LEDGERS.join(" or ")}.
              Rounds, contributions and claims appear here once it does. Nothing on this card is read from a round in the meantime.
            </p>
          )}
          <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
            source · address book {CHAIN_ID_TEXT} ({MISSING_LEDGERS.length === 0 ? "both ledgers pinned" : "no " + MISSING_LEDGERS.join(" or ") + " entry"})
          </span>
        </div>
      ) : (
      <p style={{ fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.6, margin: 0, maxWidth: 660 }}>
        Your group trains a model together: each member trains locally and contributes, the network aggregates the result, and contributors are settled in SALT for verified work. The round and your reward below are read live from {CHAIN_ID_TEXT} (PatronageLedger); member SALT claims activate once revenue is credited, past the @rule8 money-surface sign-off (gateSec).
      </p>
      )}

      {!ROUNDS_LIVE ? null : groups.length === 0 ? (
        <div className="surface" style={{ padding: 18, fontSize: 12.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
          No groups yet — create one in Groups first, then open a training round with them.
        </div>
      ) : (
        <>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span className="lbl" style={{ margin: 0 }}>Group</span>
            <select className="input" value={groupId} onChange={(e) => setGroupId(e.target.value)} style={{ minWidth: 220, flex: "0 1 320px" }}>
              {groups.map((g) => (
                <option key={g.id} value={g.id}>{shortLabel(g)}</option>
              ))}
            </select>
          </div>

          {/* live round card */}
          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
              <span style={{ fontSize: 14, fontWeight: 500 }}>Live round</span>
              {ui && <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".08em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid " + ui.tone, color: ui.tone }}>{ui.label}</span>}
              <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>{status ? `round ${status.round}${status.participants > 0 ? ` · ${status.participants} participant${status.participants === 1 ? "" : "s"}` : ""}` : ""}</span>
            </div>

            {pending ? (
              <div style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
                <span style={{ fontSize: 13, fontWeight: 500 }}>Couldn't read the round from {CHAIN_ID_TEXT}.</span>
                <p className="mono" style={{ fontSize: 11, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>{pending}</p>
                <p style={{ fontSize: 12.5, color: "var(--tx-2)", margin: 0, lineHeight: 1.6 }}>
                  Reads are wired to the PatronageLedger on {CHAIN_ID_TEXT}. If this persists, the RPC or the settlement address is unavailable — nothing here is fabricated in the meantime.
                </p>
              </div>
            ) : ui ? (
              <div style={{ padding: 18, display: "flex", flexDirection: "column", gap: 14 }}>
                {/* lifecycle rail */}
                <div style={{ display: "flex", alignItems: "center", gap: 0 }}>
                  {(["open", "aggregating", "committed", "settled"] as RoundPhase[]).map((ph, i) => {
                    const order: RoundPhase[] = ["idle", "open", "aggregating", "committed", "settled"];
                    const here = order.indexOf(phase!);
                    const mine = order.indexOf(ph);
                    const on = here >= mine && here > 0;
                    const label = ph === "open" ? "Open" : ph === "aggregating" ? "Settling" : ph === "committed" ? "Claimable" : "Claimed";
                    return (
                      <div key={ph} style={{ display: "flex", alignItems: "center", flex: i < 3 ? 1 : "0 0 auto" }}>
                        <span style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 5 }}>
                          <span style={{ width: 12, height: 12, borderRadius: 999, background: on ? "var(--accent)" : "transparent", border: "1.5px solid " + (on ? "var(--accent)" : "var(--line-2)"), flexShrink: 0 }}></span>
                          <span className="mono" style={{ fontSize: 8.5, letterSpacing: ".06em", textTransform: "uppercase", color: on ? "var(--tx-1)" : "var(--tx-3)" }}>{label}</span>
                        </span>
                        {i < 3 && <span style={{ flex: 1, height: 1.5, background: here > mine ? "var(--accent)" : "var(--line-2)", margin: "0 6px", marginBottom: 16 }}></span>}
                      </div>
                    );
                  })}
                </div>

                <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>{ui.blurb}</p>

                {reward && phase === "committed" && (
                  <div style={{ display: "flex", gap: 24, padding: "12px 14px", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)" }}>
                    <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                      <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>Your weight</span>
                      <span className="tabular" style={{ fontSize: 16, fontWeight: 500 }}>{reward.weight}</span>
                    </span>
                    <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                      <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>Reward</span>
                      <span className="tabular" style={{ fontSize: 16, fontWeight: 500, color: "var(--accent-text)" }}>{reward.salt} SALT</span>
                    </span>
                  </div>
                )}

                {(phase === "open" || phase === "committed") && (
                  <p style={{ fontSize: 12, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
                    Joining and claiming happen in Rounds, once the round contracts are deployed on {CHAIN_ID_TEXT} and payouts open after the security review.
                  </p>
                )}
              </div>
            ) : (
              <div style={{ padding: 18, fontSize: 12.5, color: "var(--tx-3)" }}>Loading round…</div>
            )}
          </div>
        </>
      )}

      {!ROUNDS_LIVE && (
        <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
          below: local adapter rounds through a coordinator · on-chain group rounds are not live yet
        </span>
      )}
      {/* HUP-S9.4: Hermes plans, explains and starts federated rounds; adapters pass the eval gate. */}
      <FlRoundsPanel fl={bridge.flRounds} requestSig={(spec) => store.requestSig(spec)} toast={(m) => store.toast(m)} />
    </div>
  );
}
