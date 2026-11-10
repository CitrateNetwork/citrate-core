// =====================================================================
// citrate-core: Node › Rewards (core plan K3; design slice 2 "Node rewards"; contract §1 Rewards)
//
// Validator rewards for the member's validator key, read from the registry and nothing else:
//   Store.refreshEarnings → node_validator_earnings → ValidatorRegistry.rewardsOf(pubkey), where the
//   key is MemberBond.pubkey() once the bond has activated, else this node's proposer key, and an
//   answer for any other key is refused (#391, #416). Folded into `s.rewards` / `s.rewardsError`.
//   Validator rewards = total, Claimable now = claimableNow, Maturing = total − claimableNow.
// The read runs only when the rewards belong to this member (`readsValidatorRewards`: registered
// stake, or a bond whose MemberBond.activated() reads true, which outlives a full exit or a slash,
// #348), and again when that changes while the tab is open. Claim runs Store.claimRewards (the
// signature ceremony; through the bond for an activated bond). On the bond path Claim is offered
// only once MemberBond.isKycVerified() reads true (`bondClaimBlocked`, contract v1.2 §1).
// The validator word comes from the same store reads as Node › Status (C1-V). Pin and round rewards
// are not live (contract §4). No projections, estimates or per-day figures.
// =====================================================================
import { useEffect, type ReactNode } from "react";
import { BRIDGE_MODE } from "../../bridge/mode";
import { bondClaimBlocked, memberKeyRuleKey, type Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import { readsValidatorRewards, validatorDash, validatorWord } from "../../shell/chainTruth";
import { bondFlags } from "./nodeWords";
import { PAGE, Panel, Row, StateCard } from "./parts";

/** SALT with two decimals from a wei string, without floating point; null when it isn't a number. */
export function saltFromWei2(wei: string): string | null {
  if (!/^[0-9]+$/.test(wei)) return null;
  const n = BigInt(wei);
  const cents = (n + 5n * 10n ** 15n) / 10n ** 16n; // round to 0.01 SALT
  const whole = cents / 100n;
  const frac = String(cents % 100n).padStart(2, "0");
  return whole.toLocaleString("en-US") + "." + frac + " SALT";
}

/** Claims below this batch up rather than spend gas on dust (the 0.5.1 Earning tab's rule). */
export const CLAIM_DUST_WEI = 5n * 10n ** 18n;

/** A read error that means "no proposer key yet" (validator.rs: "proposer key not minted yet") rather than a failed chain read. */
const NO_KEY = /proposer.?key/i;

/** The validator words that mean this member has no rewards to read. */
const NOT_VALIDATOR_WORDS = ["Not a validator", "Bond locked · not staked", "Unlocked · not activated"];

export function NodeRewards({ store, s, mode = BRIDGE_MODE }: { store: Store; s: AppState; mode?: "tauri" | "sim" }) {
  const reads = readsValidatorRewards(s.bondedStake, s.bondActivated);
  // fxEARN (Codex P2): the key the rewards are read for (memberKeyRule); a key that resolves after
  // mount re-reads them.
  const rewardsKey = memberKeyRuleKey(s);
  // Read on open, and again when the reward eligibility changes while the tab is open (#348).
  useEffect(() => {
    if (mode === "tauri") void store.refreshEarnings();
  }, [store, mode, reads, rewardsKey]);

  const shell = (body: ReactNode) => (
    <div data-node-tab="rewards" style={{ ...PAGE, maxWidth: 980 }}>
      {body}
      <NotLive />
      <p style={{ fontSize: 11.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>No projections, estimates or per-day figures: none has an on-chain source.</p>
    </div>
  );

  if (mode !== "tauri") {
    return shell(
      <StateCard
        kind="unavailable"
        title="Rewards need the desktop app"
        body="Rewards are read for the member's validator key through the app's node commands. The web preview has none, so nothing is shown."
        why="node_validator_earnings → Unavailable (BRIDGE_MODE = sim)"
      />,
    );
  }
  const error = reads ? s.rewardsError : null;
  if (error && NO_KEY.test(error)) {
    return shell(
      <StateCard
        kind="unavailable"
        title="Rewards need this node's key"
        body="Rewards are read for this node's proposer key until the bond has activated. The key appears once this node has synced and armed block production."
        why={"node_validator_earnings → " + error}
      />,
    );
  }
  if (error) {
    return shell(
      <StateCard
        kind="error"
        title="Couldn't read rewards"
        body="The registry read failed, answered for a key that isn't the member's validator key, or answered a figure this page can't read, so rewards are unknown, not zero."
        why={"node_validator_earnings → " + error}
        cta={{ label: "Try again", go: () => void store.refreshEarnings() }}
      />,
    );
  }

  const bond = bondFlags(s);
  const vWord = validatorWord(s.bondedStake, s.validatorActivation, s.hasGrant, bond.activated, bond.unlocked);
  const data = reads ? s.rewards : null;
  let rows: ReactNode;
  let note: string;
  if (!reads) {
    // Not this member's rewards to read: say the word, or "—" with why it isn't known (never 0).
    const known = vWord != null && NOT_VALIDATOR_WORDS.includes(vWord);
    rows = (
      <Row
        k="Validator rewards"
        testid="reward-total"
        v={<span data-reward="total">{known ? vWord : validatorDash(s.bondedStake, s.validatorActivation, s.hasGrant, bond.activated, bond.unlocked)}</span>}
        vColor={known ? undefined : "var(--tx-3)"}
        src="no registered stake and no activated bond for this member, so rewardsOf is not read"
      />
    );
    note =
      "Rewards are paid to validators that produce blocks. " +
      (vWord === "Not a validator"
        ? "This member is not a validator."
        : vWord === "Bond locked · not staked"
          ? "Your bond is locked but not staked; activating it on Node › Status registers you as a validator."
          : vWord === "Unlocked · not activated"
            ? "Your bond was not activated, so it has no validator rewards."
            : "This page reads them once the registry and bond reads say they are yours.");
  } else if (!data) {
    rows = <Row k="Validator rewards" testid="reward-total" v={<span data-reward="total">— · not read yet</span>} vColor="var(--tx-3)" src="ValidatorRegistry.rewardsOf(pubkey).total" />;
    note = "Reading the registry for the member's validator key.";
  } else {
    const total = BigInt(data.totalWei);
    const claimable = BigInt(data.claimableWei);
    const maturing = total > claimable ? total - claimable : 0n;
    const dust = claimable < CLAIM_DUST_WEI;
    // Contract v1.2 §1: on the bond path claimRewards() requires MemberBond.isKycVerified() on chain;
    // the identity provider's kyc_status claim is never this gate.
    const kycBlock = bondClaimBlocked(s);
    rows = (
      <>
        <Row k="Validator rewards" v={<span data-reward="total">{saltFromWei2(data.totalWei) ?? "—"}</span>} src="ValidatorRegistry.rewardsOf(pubkey).total" />
        <Row k="Claimable now" v={<span data-reward="claimable">{saltFromWei2(data.claimableWei) ?? "—"}</span>} src="rewardsOf(pubkey).claimableNow" />
        <Row k="Maturing" v={<span data-reward="maturing">{saltFromWei2(maturing.toString()) ?? "—"}</span>} src="total − claimableNow; slashable for 4 epochs" />
        <span style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 18px 0", flexWrap: "wrap" }}>
          {kycBlock ? (
            <span data-testid="claim-blocked" style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.45 }}>
              {kycBlock}
            </span>
          ) : (
            <>
              <button className="btn btn-primary btn-sm" data-testid="claim" disabled={dust} onClick={() => void store.claimRewards()}>
                Claim
              </button>
              <span style={{ fontSize: 12, color: "var(--tx-2)" }}>
                {dust ? "Claims batch until ≥ 5 SALT to avoid dust gas: " + (saltFromWei2((CLAIM_DUST_WEI - claimable).toString()) ?? "—") + " to go." : "Claimable now is at or above the 5 SALT dust threshold."}
              </span>
            </>
          )}
        </span>
      </>
    );
    note =
      (total === 0n && vWord == null
        ? "No rewards recorded for this key. rewardsOf also reads 0 for a key that isn't registered, so 0 here doesn't say whether this member is a validator. "
        : "Rewards are paid to the validator that produces each block. ") +
      "Claiming signs the unsigned claimRewards() request through the signature ceremony; the balance changes only once that transaction is included.";
  }

  return shell(
    <Panel testid="validator-rewards" title="Validator rewards" caption={s.validatorPubkey ? "key " + s.validatorPubkey.slice(0, 10) + "…" + s.validatorPubkey.slice(-6) : undefined}>
      <span style={{ fontSize: 12.5, lineHeight: 1.5, color: "var(--tx-2)", padding: "10px 18px 4px" }}>Paid for producing blocks, read from ValidatorRegistry.rewardsOf for the member's validator key.</span>
      {rows}
      <span style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", padding: "12px 18px" }}>{note}</span>
    </Panel>,
  );
}

/** Pin and round rewards: not live (contract §4), with the required wording. */
function NotLive() {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))", gap: 14 }}>
      <div className="surface" data-testid="pin-rewards" style={{ padding: "14px 18px", display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Pin rewards</span>
        <span className="mono" style={{ fontSize: 11, color: "var(--warn)" }}>
          Pin bonds not live yet
        </span>
        <span style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)" }}>
          The pin bond contract (IPFSIncentivesV3) is deployed but not funded, and no pins exist. Nothing is bonded or paid for pins until it is. Local pins are on Storage › Pins.
        </span>
      </div>
      <div className="surface" data-testid="round-rewards" style={{ padding: "14px 18px", display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Round rewards</span>
        <span className="mono" style={{ fontSize: 11, color: "var(--warn)" }}>
          Group rounds not live yet
        </span>
        <span style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)" }}>The round ledgers are not deployed. Rounds go live after mainnet.</span>
      </div>
    </div>
  );
}
