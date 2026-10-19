// =====================================================================
// citrate-core — K4 storage wording and formatting (pure; shared by Files, Pins and Backup).
//
// UI truth contract §1 "Pins": IPFSIncentivesV3 is deployed but dormant and unfunded, so no pin has
// a bond and Pinned / Challenged / Released are never shown as live. A pin this node keeps is
// "Local pin · no bond" (the design's wording; flagged for chain-team approval in the K4 PR). The
// V3 BOND / REWARD / ROUNDS reads are parameters only (§2), and unknown is never shown as 0.
// =====================================================================
import type { BackupMismatch, BackupProgress, PinRow } from "../bridge/domains";

/** The required wording until V3 is funded at genesis (truth contract §1). */
export const PIN_BONDS_NOT_LIVE = "Pin bonds not live yet";

/** The label for a pin this node keeps (no bond). */
export const LOCAL_PIN_LABEL = "Local pin · no bond";

/** An index row the IPFS daemon no longer holds. */
export const NOT_PINNED_LABEL = "Not pinned on this node";

/** The one label a pin row may show. The daemon is the source: pinned → local pin, no bond.
 *  Anything else (including a reserved `pinning`/`challenged` value) is not shown as a chain state. */
export function pinStateLabel(row: Pick<PinRow, "pinState">): string {
  return row.pinState === "pinned" ? LOCAL_PIN_LABEL : NOT_PINNED_LABEL;
}

/** Bytes for people ("—" for unknown/zero). */
export function humanBytes(n: number): string {
  if (!n || n < 0) return "—";
  const gb = n / 1e9;
  if (gb >= 1) return `${gb.toFixed(gb >= 10 ? 0 : 1)} GB`;
  const mb = n / 1e6;
  if (mb >= 1) return `${Math.round(mb)} MB`;
  return `${Math.max(1, Math.round(n / 1e3))} KB`;
}

/** A wei amount (decimal string) as SALT, exact, trailing zeros trimmed. `null` (unknown) → "—". */
export function formatSaltWei(wei: string | null | undefined): string {
  if (wei == null || !/^\d+$/.test(wei)) return "—";
  const v = BigInt(wei);
  const unit = 10n ** 18n;
  const whole = v / unit;
  const frac = (v % unit).toString().padStart(18, "0").replace(/0+$/, "");
  const wholeStr = whole.toLocaleString("en-US");
  return frac ? `${wholeStr}.${frac} SALT` : `${wholeStr} SALT`;
}

/** One manifest mismatch in plain words. */
export function describeMismatch(m: BackupMismatch): string {
  switch (m.kind) {
    case "missing":
      return `${m.path}: missing from the folder`;
    case "sizeDiffers":
      return `${m.path}: ${m.actual} bytes, the manifest says ${m.expected}`;
    case "hashDiffers":
      return `${m.path}: content differs from the manifest (sha256)`;
    case "unlisted":
      return `${m.path}: not listed in the manifest`;
    case "unsafePath":
      return `${m.path}: not an allowed backup path`;
    case "notAFile":
      return `${m.path}: not a regular file`;
    case "badMode":
      return `${m.path}: not an allowed file mode`;
    case "modeDiffers":
      return `${m.path}: mode ${m.actual.toString(8)} after restore, the manifest says ${m.expected.toString(8)}`;
  }
}

/** A progress step as a 0–100 percentage, or null when the step has no count. */
export function progressPct(p: BackupProgress | null): number | null {
  if (!p || p.total <= 0) return null;
  return Math.max(0, Math.min(100, Math.round((p.done / p.total) * 100)));
}

/** A progress step in words. */
export function progressText(p: BackupProgress | null): string {
  if (!p) return "Starting…";
  switch (p.phase) {
    case "files":
      return `Files ${p.done} of ${p.total}`;
    case "memory":
      return `Memory ${p.done} of ${p.total}`;
    case "verify":
      return "Checking against the manifest…";
    case "daemon":
      return "Pausing the memory daemon…";
    case "done":
      return "Done";
  }
}
