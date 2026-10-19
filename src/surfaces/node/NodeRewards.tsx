// =====================================================================
// citrate-core — Node › Rewards (core plan K3; design slice 2 "Node rewards"; contract §1 Rewards)
//
// Validator rewards for this node's key, read from the registry and nothing else:
//   node_validator_earnings → ValidatorRegistry.rewardsOf(proposerPubkey) (node.rs, validator.rs)
//     Validator rewards = total, Claimable now = claimableNow, Maturing = total − claimableNow.
// rewardsOf also returns zeros for a key that isn't registered, so a zero read says so; once the
// validator word is read (C1-V), "Not a validator" and "Bond locked · not staked" replace the
// numbers. Pin and round rewards are not live (contract §4). No projections, per-day figures or
// claim button: there is no validator-claim command yet.
// =====================================================================
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { bridge } from "../../bridge";
import { BRIDGE_MODE } from "../../bridge/mode";
import { PAGE, Panel, Row, StateCard, errText } from "./parts";

export interface ValidatorRewardsRead {
  totalWei: string;
  claimableWei: string;
  proposerPubkey: string;
}

/** SALT with two decimals from a wei string, without floating point; null when it isn't a number. */
export function saltFromWei2(wei: string): string | null {
  let n: bigint;
  try {
    n = BigInt(wei);
  } catch {
    return null;
  }
  if (n < 0n) return null;
  const cents = (n + 5n * 10n ** 15n) / 10n ** 16n; // round to 0.01 SALT
  const whole = cents / 100n;
  const frac = String(cents % 100n).padStart(2, "0");
  return whole.toLocaleString("en-US") + "." + frac + " SALT";
}

/** A read error that means "no proposer key yet" (validator.rs: "proposer key not minted yet") rather than a failed chain read. */
const NO_KEY = /proposer.?key/i;

const NOT_VALIDATOR_WORDS = ["Not a validator", "Bond locked · not staked"];

export function NodeRewards({
  mode = BRIDGE_MODE,
  read,
  validatorWord = null,
}: {
  mode?: "tauri" | "sim";
  read?: () => Promise<ValidatorRewardsRead>;
  validatorWord?: string | null;
}) {
  const [r] = useState(() => read ?? (() => bridge.node.validatorEarnings()));
  const [data, setData] = useState<ValidatorRewardsRead | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setData(await r());
      setError(null);
    } catch (e) {
      setData(null);
      setError(errText(e));
    } finally {
      setLoading(false);
    }
  }, [r]);

  useEffect(() => {
    if (mode === "tauri") void load();
  }, [mode, load]);

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
        body="Rewards are read for this node's proposer key, which lives in the node's data directory. The web preview has none, so nothing is shown."
        why="node.validatorEarnings() → Unavailable (BRIDGE_MODE = sim)"
      />,
    );
  }
  if (error && NO_KEY.test(error)) {
    return shell(
      <StateCard
        kind="unavailable"
        title="Rewards need this node's key"
        body="Rewards are read for this node's proposer key, which only exists once the block producer has been armed on a synced node. Activate validator on the Status tab does that."
        why={"node_validator_earnings → " + error}
      />,
    );
  }
  if (error) {
    return shell(
      <StateCard
        kind="error"
        title="Couldn't read rewards"
        body="The registry read failed, so rewards are unknown, not zero."
        why={"node_validator_earnings → error: " + error}
        cta={{ label: "Try again", go: () => void load() }}
      />,
    );
  }

  const notValidator = validatorWord != null && NOT_VALIDATOR_WORDS.includes(validatorWord);
  let rows: ReactNode;
  let note: string;
  if (loading || !data) {
    rows = <Row k="Validator rewards" v="— · reading" vColor="var(--tx-3)" src="ValidatorRegistry.rewardsOf(pubkey).total" />;
    note = "Reading the registry for this node's key.";
  } else if (notValidator) {
    rows = <Row k="Validator rewards" testid="reward-total" v={<span data-reward="total">{validatorWord}</span>} src="no registered stake for this node's key, so there is nothing to read" />;
    note = "Rewards are paid to validators that produce blocks. " + (validatorWord === "Not a validator" ? "This node is not a validator." : "Your bond is locked but not staked; activating it registers you as a validator.");
  } else {
    const total = BigInt(data.totalWei || "0");
    const claimable = BigInt(data.claimableWei || "0");
    const maturing = total > claimable ? total - claimable : 0n;
    rows = (
      <>
        <Row k="Validator rewards" v={<span data-reward="total">{saltFromWei2(data.totalWei) ?? "—"}</span>} src="ValidatorRegistry.rewardsOf(pubkey).total" />
        <Row k="Claimable now" v={<span data-reward="claimable">{saltFromWei2(data.claimableWei) ?? "—"}</span>} src="rewardsOf(pubkey).claimableNow" />
        <Row k="Maturing" v={<span data-reward="maturing">{saltFromWei2(maturing.toString()) ?? "—"}</span>} src="total − claimableNow; slashable for 4 epochs" />
      </>
    );
    note =
      total === 0n && validatorWord == null
        ? "No rewards recorded for this key. rewardsOf also reads 0 for a key that isn't registered, so 0 here doesn't say whether this node is a validator."
        : "Rewards are paid to the validator that produces each block. Claiming validator rewards from the app is not available in this build.";
  }

  return shell(
    <Panel testid="validator-rewards" title="Validator rewards" caption={data ? "key " + data.proposerPubkey.slice(0, 10) + "…" + data.proposerPubkey.slice(-6) : undefined}>
      <span style={{ fontSize: 12.5, lineHeight: 1.5, color: "var(--tx-2)", padding: "10px 18px 4px" }}>Paid for producing blocks, read from the validator registry for this node's key.</span>
      {rows}
      <span style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", padding: "12px 18px" }}>{note}</span>
    </Panel>,
  );
}

/** Pin and round rewards: not live on 40204 (contract §4), with the required wording. */
function NotLive() {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
      <div className="surface" style={{ padding: "14px 18px", display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Pin rewards</span>
        <span className="mono" style={{ fontSize: 11, color: "var(--warn)" }}>
          Pin bonds not live yet
        </span>
        <span style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)" }}>The pin-bond contract is not funded on 40204. Nothing is paid for pins until it is.</span>
      </div>
      <div className="surface" style={{ padding: "14px 18px", display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Round rewards</span>
        <span className="mono" style={{ fontSize: 11, color: "var(--warn)" }}>
          Group rounds not live yet
        </span>
        <span style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)" }}>The round ledgers are not deployed on 40204. Rounds go live after mainnet.</span>
      </div>
    </div>
  );
}
