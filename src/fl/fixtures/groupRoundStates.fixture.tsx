// TEST-ONLY. The design's round card states, drawn from FederatedRoundLedger's `Status` enum
// (Committed / Rejected / Accepted). No production module may import this file: the chain has no
// open, accepted or rejected group round to read, so these states exist only as labelled fixtures
// (UI truth contract rule 4). groupRoundCards.test.tsx enforces both.

export interface FixtureRoundState {
  status: "committed" | "accepted" | "rejected";
  ordinal: number;
  participants: number;
}

export const FIXTURE_ROUND_STATES: FixtureRoundState[] = [
  { status: "committed", ordinal: 1, participants: 4 },
  { status: "accepted", ordinal: 1, participants: 4 },
  { status: "rejected", ordinal: 2, participants: 3 },
];

const WORDS: Record<FixtureRoundState["status"], { label: string; body: string }> = {
  committed: { label: "Committed · challenge window open", body: "The coordinator committed this round. Anyone can challenge it until the window closes." },
  accepted: { label: "Accepted", body: "The challenge window closed with no successful challenge." },
  rejected: { label: "Rejected", body: "A challenge showed the round was wrong. It is rejected permanently." },
};

export function FixtureRoundCard({ state }: { state: FixtureRoundState }) {
  const w = WORDS[state.status];
  return (
    <div className="surface" data-fixture="true" style={{ padding: "12px 16px", display: "flex", flexDirection: "column", gap: 6 }}>
      <span className="mono" style={{ fontSize: 9.5, textTransform: "uppercase", color: "var(--warn)" }}>fixture · design state, not a chain read</span>
      <span style={{ fontSize: 13, fontWeight: 500 }}>
        Round {state.ordinal} · {w.label}
      </span>
      <span style={{ fontSize: 12, color: "var(--tx-2)" }}>
        {state.participants} participants. {w.body}
      </span>
    </div>
  );
}
