// =====================================================================
// citrate-core: group round cards (0.5.2 FL, first cut, behind VITE_FL_ROUND_CARDS)
//
// One card, two places: Groups › Rounds ("tab") and the top of a group's chat ("chat"). It shows
// only what the address book and the chain say about the two round ledgers (groupRoundsLedger.ts):
// not live, deployed and dormant, or unknown. It shows no round, no amounts and no projections:
// rounds go live after mainnet, and until then there is nothing to join here.
// =====================================================================
import { useEffect, useState } from "react";
import { bridge } from "../bridge";
import realBook from "../../src-tauri/addresses/40204.json";
import { bookOnlyReading, readGroupRounds, type GroupRoundsReading, type RoundLedgerBook } from "./groupRoundsLedger";

const SOURCE = "source · address book 40204.json · eth_getCode on 40204";

const chip = (tone: string): React.CSSProperties => ({
  fontSize: 9.5,
  letterSpacing: ".08em",
  textTransform: "uppercase",
  padding: "2px 9px",
  borderRadius: 999,
  border: "1px solid " + tone,
  color: tone,
  flexShrink: 0,
});

const list = (names: string[]) => names.join(" and ");
const short = (a: string) => `${a.slice(0, 8)}…${a.slice(-4)}`;

function copyFor(r: GroupRoundsReading): { chip: string; tone: string; title: string; body: string } {
  switch (r.kind) {
    case "loading":
      return { chip: "Reading", tone: "var(--tx-3)", title: "Group rounds", body: "Reading the round ledgers named in the address book." };
    case "not-live":
      return {
        chip: "Not live",
        tone: "var(--tx-3)",
        title: "Group rounds not live yet",
        body: `Group rounds need the FederatedRoundLedger and PatronageLedger contracts. This build's address book does not list ${list(r.missing)}, so there is no round to join here.`,
      };
    case "no-code":
      return {
        chip: "Not live",
        tone: "var(--tx-3)",
        title: "Group rounds not live yet",
        body: `The address book lists ${list(r.empty)}, but there is no contract code at that address on 40204, so there is no round to join here.`,
      };
    case "unavailable":
      return {
        chip: "—",
        tone: "var(--warn)",
        title: "Round ledgers: —",
        body: `Couldn't read the round ledgers on 40204 (${r.reason}). Their state is unknown until the read succeeds.`,
      };
    case "dormant":
      return {
        chip: "Deployed · dormant",
        tone: "var(--tx-2)",
        title: "Group rounds are not open yet",
        body: "The round ledgers are deployed on 40204 and dormant while they are audited. Rounds open after mainnet; until then there is no round to join here.",
      };
  }
}

export function GroupRoundsView({ reading, variant }: { reading: GroupRoundsReading; variant: "chat" | "tab" }) {
  const c = copyFor(reading);
  const card = (
    <div data-testid="group-rounds-card" data-reading={reading.kind} className="surface" style={{ padding: variant === "chat" ? "10px 14px" : "16px 18px", display: "flex", flexDirection: "column", gap: 8, maxWidth: 660 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        <span style={{ fontSize: variant === "chat" ? 12.5 : 14, fontWeight: 500 }}>{c.title}</span>
        <span className="mono" style={chip(c.tone)}>{c.chip}</span>
      </div>
      <p style={{ fontSize: variant === "chat" ? 12 : 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>{c.body}</p>
      {reading.kind === "dormant" && variant === "tab" && (
        <ul className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", margin: 0, paddingLeft: 16, lineHeight: 1.7 }}>
          {reading.ledgers.map((l) => (
            <li key={l.name}>
              {l.name} · {short(l.address)} · {l.codeSize.toLocaleString("en-US")} bytes of code
            </li>
          ))}
        </ul>
      )}
      <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)" }}>{SOURCE}</span>
    </div>
  );
  if (variant === "chat") return card;
  return (
    <div style={{ padding: "20px 26px 0", display: "flex", flexDirection: "column", gap: 12, maxWidth: 900 }}>
      <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Rounds</span>
      {card}
    </div>
  );
}

const chainCodeSize = (address: string) => bridge.contracts.codeSize("citrate", address);

export interface GroupRoundsCardProps {
  variant: "chat" | "tab";
  /** Defaults to the generated address book; tests pass a book with pins. */
  book?: RoundLedgerBook;
  /** Defaults to core's `contract_code_size` on 40204. */
  codeSize?: (address: string) => Promise<number>;
}

export function GroupRoundsCard({ variant, book = realBook, codeSize = chainCodeSize }: GroupRoundsCardProps) {
  const [reading, setReading] = useState<GroupRoundsReading>(() => bookOnlyReading(book));
  useEffect(() => {
    if (bookOnlyReading(book).kind === "not-live") return;
    let live = true;
    void readGroupRounds(book, codeSize).then((r) => {
      if (live) setReading(r);
    });
    return () => {
      live = false;
    };
  }, [book, codeSize]);
  return <GroupRoundsView reading={reading} variant={variant} />;
}
