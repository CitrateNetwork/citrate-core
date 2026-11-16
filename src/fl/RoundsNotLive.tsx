// =====================================================================
// citrate-core: the honest Rounds state while group rounds cannot run (FL design, "Today (0.5.4)")
//
// The chip, a sentence and the contract table, all from the network state model. A view that
// needs rounds renders this instead of anything drawn from a round, until the data shows rounds.
// "Coordinator not answering" style failures arrive as phase "unknown" with the reason.
// =====================================================================
import type { ReactNode } from "react";
import { ContractTable } from "./ContractTable";
import { NO_TRAINING_YET, ROUNDS_NOT_LIVE, type FlContractName, type FlNetworkState } from "./networkState";
import { Chip, Note, Unknown } from "./primitives";

export interface RoundsNotLiveProps {
  state: FlNetworkState;
  /** The view's own headline, e.g. "Rounds open on this network after the WorkMarket contract is deployed." */
  headline: string;
  lead?: ReactNode;
  contracts?: FlContractName[];
  actions?: ReactNode;
  testid?: string;
}

export function RoundsNotLive({ state, headline, lead, contracts, actions, testid = "rounds-not-live" }: RoundsNotLiveProps) {
  const unknown = state.phase === "unknown";
  return (
    <section className="fl-main" data-testid={testid} data-phase={state.phase} aria-labelledby={testid + "-h"}>
      {unknown ? (
        <Chip tone="warn" style={{ alignSelf: "flex-start" }}>
          Round contracts: —
        </Chip>
      ) : (
        <Chip tone="warn" style={{ alignSelf: "flex-start" }}>
          {ROUNDS_NOT_LIVE}
        </Chip>
      )}
      <h1 id={testid + "-h"} className="fl-disp" style={{ fontSize: 24, maxWidth: 720 }}>
        {headline}
      </h1>
      {unknown && (
        <p style={{ margin: 0, fontSize: 13 }}>
          Couldn't read whether rounds can run here: <Unknown reason={state.phaseReason ?? "the read failed"} />
        </p>
      )}
      {lead && (
        <div className="fl-muted" style={{ margin: 0, fontSize: 13.5, lineHeight: 1.55, maxWidth: 680 }}>
          {lead}
        </div>
      )}
      <ContractTable state={state} names={contracts} />
      <Note>{NO_TRAINING_YET}: the training worker is not bundled in this release.</Note>
      {actions && <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>{actions}</div>}
    </section>
  );
}
