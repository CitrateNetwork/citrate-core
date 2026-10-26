// FL round cards (0.5.2, first cut): what the round ledgers say, read ONLY through the address book.
//
// Data source (Rule 7): the generated address book `src-tauri/addresses/40204.json` for the two
// ledger addresses, then `eth_getCode` on 40204 (core's `contract_code_size`) for each. Group rounds
// need both FederatedRoundLedger and PatronageLedger. They deploy dormant at genesis (audit phase A)
// and rounds go live after mainnet, so the most this card can say is "deployed, dormant". Until the
// book pins both, it says "Group rounds not live yet" and reads nothing from the chain.

export const ROUND_LEDGERS = ["FederatedRoundLedger", "PatronageLedger"] as const;
export type RoundLedgerName = (typeof ROUND_LEDGERS)[number];

/** The slice of the address book this module reads. */
export interface RoundLedgerBook {
  addresses: Record<string, string | undefined>;
}

export interface RoundLedgerPin {
  name: RoundLedgerName;
  /** Lowercase 0x address, or null when the book has no usable entry. */
  address: string | null;
}

export type GroupRoundsReading =
  | { kind: "loading" }
  /** The book does not pin these ledgers. */
  | { kind: "not-live"; missing: RoundLedgerName[] }
  /** The book pins them, but there is no contract code at these addresses. */
  | { kind: "no-code"; empty: RoundLedgerName[] }
  /** The code read failed: unknown, not zero. */
  | { kind: "unavailable"; reason: string }
  /** Both ledgers have code. Rounds are not open before mainnet. */
  | { kind: "dormant"; ledgers: { name: RoundLedgerName; address: string; codeSize: number }[] };

const ADDRESS = /^0x[0-9a-f]{40}$/;
const ZERO = "0x" + "0".repeat(40);

export function roundLedgerPins(book: RoundLedgerBook): RoundLedgerPin[] {
  return ROUND_LEDGERS.map((name) => {
    const raw = (book.addresses[name] ?? "").trim().toLowerCase();
    return { name, address: ADDRESS.test(raw) && raw !== ZERO ? raw : null };
  });
}

/** The reading available without touching the chain: "not live" when a pin is missing. */
export function bookOnlyReading(book: RoundLedgerBook): GroupRoundsReading {
  const missing = roundLedgerPins(book)
    .filter((p) => p.address === null)
    .map((p) => p.name);
  return missing.length ? { kind: "not-live", missing } : { kind: "loading" };
}

export async function readGroupRounds(book: RoundLedgerBook, codeSize: (address: string) => Promise<number>): Promise<GroupRoundsReading> {
  const first = bookOnlyReading(book);
  if (first.kind === "not-live") return first;
  const ledgers: { name: RoundLedgerName; address: string; codeSize: number }[] = [];
  try {
    for (const p of roundLedgerPins(book)) {
      if (p.address === null) return bookOnlyReading(book);
      ledgers.push({ name: p.name, address: p.address, codeSize: await codeSize(p.address) });
    }
  } catch (e) {
    return { kind: "unavailable", reason: e instanceof Error ? e.message : String(e) };
  }
  const empty = ledgers.filter((l) => !(l.codeSize > 0)).map((l) => l.name);
  return empty.length ? { kind: "no-code", empty } : { kind: "dormant", ledgers };
}
