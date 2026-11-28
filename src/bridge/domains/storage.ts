// citrate-core: bridge domain `storage`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ── C-17 storage & pinning file store (lane s2) ──
export interface PinRow {
  cid: string;
  sizeBytes: number;
  bondSalt: string;
  pinState: "pinned" | "pinning" | "challenged" | "unpinned";
  addedAt: number;
}
/** K4 — IPFSIncentivesV3 pin-bond PARAMETERS (Rust `storage::PinParams`). Parameters only: the
 *  contract is deployed but dormant and unfunded, so `bondsLive` is always false in this build.
 *  A null value means its read failed (unknown is not zero); the reason is in `errors`. */
export interface PinParams {
  contract: string;
  /** `BOND()` in wei, decimal string. */
  bondWei: string | null;
  /** `REWARD()` in wei, decimal string, paid over `rounds` rounds. */
  rewardWei: string | null;
  rounds: number | null;
  bondsLive: boolean;
  /** Where the values came from (shown on screen). */
  source: string;
  errors: string[];
}

/** K4 — a storage part a backup can hold. */
export type BackupPart = "files" | "memory";

/** STO-2 — how an export is written: sealed under a passphrase (the default), or the plain K4
 *  folder, which the member must choose explicitly. */
export type BackupProtection = { passphrase: string } | { plaintext: true };

/** K4 — one export/restore progress step (Rust `storage_backup::Progress`). */
export interface BackupProgress {
  op: "export" | "restore";
  phase: "files" | "memory" | "verify" | "daemon" | "done";
  done: number;
  total: number;
  item: string;
}

/** K4 — one way a backup folder disagrees with its manifest (Rust `storage_backup::Mismatch`). */
export type BackupMismatch =
  | { kind: "missing"; path: string }
  | { kind: "sizeDiffers"; path: string; expected: number; actual: number }
  | { kind: "hashDiffers"; path: string }
  | { kind: "unlisted"; path: string }
  | { kind: "unsafePath"; path: string }
  | { kind: "notAFile"; path: string }
  | { kind: "badMode"; path: string }
  | { kind: "modeDiffers"; path: string; expected: number; actual: number };

export interface BackupExportReport {
  folder: string;
  createdAt: number;
  /** The parts actually written (a skipped part is in `skipped` with item "files" / "memory"). */
  parts: BackupPart[];
  files: number;
  memoryFiles: number;
  totalBytes: number;
  entriesSha256: string;
  skipped: { item: string; reason: string }[];
  daemonRestarted: boolean;
  notes: string[];
  /** STO-2: `folder` is one passphrase-sealed file, not a plain folder. */
  encrypted: boolean;
}

export interface BackupVerifyReport {
  folder: string;
  ok: boolean;
  createdAt: number | null;
  parts: BackupPart[];
  fileCount: number;
  totalBytes: number;
  mismatches: BackupMismatch[];
  /** A problem with the manifest itself (missing, other format, edited). */
  problem: string | null;
}

export interface BackupRestoreReport {
  folder: string;
  /** The parts actually restored. */
  parts: BackupPart[];
  /** Requested parts that were not restored, and why. */
  skipped: { item: string; reason: string }[];
  filesRestored: number;
  cidsChanged: { expected: string; actual: string }[];
  memoryFilesRestored: number;
  previousMemoryKeptAt: string | null;
  daemonRestarted: boolean;
  notes: string[];
}

export interface StorageDomain {
  /** Add a local file to IPFS; returns its CID. */
  add(path: string): Promise<{ cid: string; sizeBytes: number }>;
  /** K4: keep a CID on this node only ("Local pin · no bond"). No bond, no transaction: pin bonds
   *  are not live until IPFSIncentivesV3 is funded at genesis. */
  pinLocal(cid: string): Promise<void>;
  list(): Promise<PinRow[]>;
  retrieve(cid: string): Promise<{ path: string }>;
  unpin(cid: string): Promise<void>;
  /** K4: the IPFSIncentivesV3 BOND / REWARD / ROUNDS parameters, read from chain 40204. */
  pinParams(): Promise<PinParams>;
  /** K4 + STO-2: export Files and/or Memory into `dest`, as one passphrase-sealed file or, when the
   *  member chose it, a plain verified folder. */
  backupExport(
    dest: string,
    parts: BackupPart[],
    protection: BackupProtection,
    onProgress?: (p: BackupProgress) => void,
  ): Promise<BackupExportReport>;
  /** K4: check a backup against its manifest (sha256 + size per file). A sealed file needs its passphrase. */
  backupVerify(path: string, passphrase?: string): Promise<BackupVerifyReport>;
  /** K4: restore from a backup; Rust refuses a wrong passphrase or any manifest mismatch before touching anything. */
  backupRestore(
    path: string,
    parts: BackupPart[],
    passphrase?: string,
    onProgress?: (p: BackupProgress) => void,
  ): Promise<BackupRestoreReport>;
}
