// =====================================================================
// citrate-core — the network this build targets (K6 network switch)
//
// One source for chain id, genesis, RPC, explorer, faucet, bootnodes and
// address book: src-tauri/network/profiles.json, read here and by the Rust
// side (src-tauri/src/network_profile.rs). A value nobody has set yet is null
// with a reason in `pending`; it is never a placeholder. A profile with any
// null is not ready, and the network gate blocks the app (NetworkCompatGate).
// =====================================================================
import file from "../../src-tauri/network/profiles.json";

export type NetworkProfile = {
  id: string;
  label: string;
  chainId: number | null;
  genesisHash: string | null;
  /** The `[chain] genesis_profile` the node is told (which block 0 it builds); one of NODE_GENESIS_PROFILES. */
  genesisProfile: string | null;
  nodeNetwork: string | null;
  rpcUrl: string | null;
  explorerUrl: string | null;
  faucetUrl: string | null;
  bootnodes: string[] | null;
  addressBook: string | null;
  /** sha256 (64 lowercase hex) of the frozen canonical address book of this genesis. */
  addressBookSha256: string | null;
  /** The citrate-chain commit (40 lowercase hex) the bundled node must contain. */
  minChainRev: string | null;
  /** CORE-N5: blocks a member node keeps (pruned chain store); absent = full history. At least
   *  MIN_MEMBER_PRUNE_RETAIN (network_profile.rs). */
  memberPruneRetain?: number | null;
  /** CORE-N2: why the network is retired; a retired profile is refused like an unset one. */
  retired?: string | null;
  /** The `[checkpoint]` values the node config repeats, for a genesis that compiles them (r1007:
   *  the node refuses to start when they differ). Written by scripts/network-cut.py apply; absent
   *  = the template's section. */
  nodeCheckpoint?: NodeCheckpoint | null;
  /** G-SEC PAY-14: whether FL payouts are open on this network; absent = false. Takes effect only
   *  where `flCodePins` matches the live code, which Rust checks (kit payouts_gate.rs); the UI
   *  reads the effective answer from core's `fl_payouts_status`. */
  payoutsOpen?: boolean;
  /** G-SEC PAY-11: each FL contract's address and keccak256 of its runtime code, from the
   *  network's root book; a contract not on the network has null values and a `pending` reason. */
  flCodePins?: FlCodePins;
  pending: Record<string, string>;
};

/** network_profile.rs `FL_CONTRACT_NAMES`: the contracts `flCodePins` names, exactly these. */
export const FL_PIN_CONTRACTS = ["WorkMarket", "FLRoundVerifier", "FederatedRoundLedger", "DatasetRegistry", "PatronageLedger", "LoRAFactory"] as const;
export type FlPin = { address: string | null; codeHash: string | null; pending?: string };
export type FlCodePins = Record<(typeof FL_PIN_CONTRACTS)[number], FlPin>;

/** network_profile.rs `NodeCheckpoint`: positive integers, exactly these keys. */
export type NodeCheckpoint = { interval: number; committeeSize: number; voteDepth: number };

function isNodeCheckpoint(c: unknown): boolean {
  if (typeof c !== "object" || c === null || Array.isArray(c)) return false;
  const keys = Object.keys(c).sort();
  if (keys.join(",") !== "committeeSize,interval,voteDepth") return false;
  return Object.values(c).every((v) => typeof v === "number" && Number.isSafeInteger(v) && v > 0);
}

/** network_profile.rs `payouts_policy_checked`: each pin is an address and a code hash, or neither
 *  with a pending reason. */
function flPinProblems(pins: unknown): string[] {
  if (typeof pins !== "object" || pins === null || Array.isArray(pins)) return ["flCodePins is not an object"];
  const names = Object.keys(pins).sort();
  if (names.join(",") !== [...FL_PIN_CONTRACTS].sort().join(",")) return [`flCodePins must name exactly ${FL_PIN_CONTRACTS.join(", ")}`];
  const out: string[] = [];
  for (const [name, pin] of Object.entries(pins as Record<string, FlPin>)) {
    const a = pin?.address;
    const h = pin?.codeHash;
    if (a == null && h == null) {
      if (typeof pin?.pending !== "string" || pin.pending.trim() === "") out.push(`flCodePins.${name} is not pinned and names no pending reason`);
      continue;
    }
    if (typeof a !== "string" || !/^0x[0-9a-fA-F]{40}$/.test(a) || /^0x0{40}$/.test(a)) out.push(`flCodePins.${name}.address is not an address`);
    if (typeof h !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(h) || /^0x0{64}$/.test(h)) out.push(`flCodePins.${name}.codeHash is not a code hash`);
  }
  return out;
}

/** The undo window (ADR CV-7a): the fewest blocks a pruning member may keep (network_profile.rs). */
export const MIN_MEMBER_PRUNE_RETAIN = 10_000;

type RawProfile = Omit<NetworkProfile, "id">;

/** A genesis profile the bundled node builds (network_profile.rs `NodeGenesis`). */
export type NodeGenesis = { genesisHash: string; nodeDefault: boolean; compiledActivations: boolean };

const raw = file as unknown as {
  active: string;
  nodeGenesisProfiles?: Record<string, NodeGenesis>;
  profiles: Record<string, RawProfile>;
};

/** The genesis profiles the bundled node builds, by `[chain] genesis_profile` name. */
export const NODE_GENESIS_PROFILES: Readonly<Record<string, NodeGenesis>> = raw.nodeGenesisProfiles ?? {};

const FIELDS = [
  "chainId",
  "genesisHash",
  "genesisProfile",
  "nodeNetwork",
  "rpcUrl",
  "explorerUrl",
  "faucetUrl",
  "bootnodes",
  "addressBook",
  "addressBookSha256",
  "minChainRev",
] as const;

/** Every profile this build knows. */
export const PROFILES: NetworkProfile[] = Object.entries(raw.profiles).map(([id, p]) => ({
  id,
  ...p,
  pending: p.pending ?? {},
}));

/** The id of the network this build targets. */
export const ACTIVE_PROFILE_ID: string = raw.active;

/** The profile of the network this build targets, or null if `active` names none. */
export const ACTIVE_PROFILE: NetworkProfile | null =
  PROFILES.find((p) => p.id === ACTIVE_PROFILE_ID) ?? null;

/** The unset values of a profile, each with what sets it. */
export function missingValues(p: NetworkProfile): { field: string; reason: string }[] {
  return FIELDS.filter((f) => p[f] === null || p[f] === undefined).map((f) => ({
    field: f,
    reason: p.pending[f] ?? "not recorded",
  }));
}

/** The chain id this build targets, for copy ("chain {CHAIN_ID_TEXT}"): the active profile's,
 *  or "not set" when it has none. Copy never names a chain id of its own. */
export const CHAIN_ID_TEXT: string = ACTIVE_PROFILE?.chainId != null ? String(ACTIVE_PROFILE.chainId) : "not set";

/** The host of the profile's RPC (`rpc.example.org`), or null when it is not set. */
export function rpcHost(p: NetworkProfile | null = ACTIVE_PROFILE): string | null {
  if (!p?.rpcUrl) return null;
  try {
    return new URL(p.rpcUrl).host || null;
  } catch {
    return null;
  }
}

const HASH = /^0x[0-9a-f]{64}$/;

/**
 * The block-0 hash this build targets (lowercase), or null when the active
 * profile has none set (or a malformed or zero one, which counts as unset).
 */
export function expectedGenesis(p: NetworkProfile | null = ACTIVE_PROFILE): string | null {
  const g = (p?.genesisHash ?? "").toLowerCase();
  if (!HASH.test(g) || /^0x0+$/.test(g)) return null;
  return g;
}

/** `noise_<64 hex>@<host>:<port>` (network_profile.rs is_bootnode). */
function isBootnode(s: unknown): boolean {
  if (typeof s !== "string") return false;
  const m = /^noise_([0-9a-fA-F]{64})@([A-Za-z0-9.-]+):([0-9]+)$/.exec(s);
  if (!m) return false;
  const port = Number(m[3]);
  return Number.isInteger(port) && port > 0 && port <= 65535;
}

/** An `https://` URL with a non-empty, whitespace-free rest (network_profile.rs ready). */
function isHttps(v: unknown): boolean {
  return typeof v === "string" && v.startsWith("https://") && v.length > 8 && !/\s/.test(v);
}

/** Exactly `len` lowercase hex digits, not all zero (network_profile.rs is_lower_hex_nonzero). */
function isLowerHexNonZero(v: unknown, len: number): boolean {
  return typeof v === "string" && v.length === len && /^[0-9a-f]+$/.test(v) && /[1-9a-f]/.test(v);
}

/**
 * Why the profile is not ready: every unset value, then every malformed one. The same rule as
 * the Rust `NetworkProfile::ready` (src-tauri/src/network_profile.rs), which gates node starts
 * and every wallet signature, so the webview never treats as ready a profile Rust refuses.
 */
export function profileProblems(p: NetworkProfile | null): string[] {
  if (p === null) return ["no active network profile"];
  const missing = missingValues(p);
  if (missing.length > 0) return missing.map((m) => `${m.field} not set (${m.reason})`);
  if (p.retired != null) {
    return typeof p.retired === "string" && p.retired.trim() !== "" ? [`retired: ${p.retired}`] : ["retired is not a reason"];
  }
  const out: string[] = [];
  if (p.memberPruneRetain != null && !(Number.isSafeInteger(p.memberPruneRetain) && p.memberPruneRetain >= MIN_MEMBER_PRUNE_RETAIN)) {
    out.push("memberPruneRetain is below the 10,000-block undo window");
  }
  if (p.nodeCheckpoint != null && !isNodeCheckpoint(p.nodeCheckpoint)) {
    out.push("nodeCheckpoint is not {interval, committeeSize, voteDepth} of positive integers");
  }
  if (p.payoutsOpen !== undefined && typeof p.payoutsOpen !== "boolean") out.push("payoutsOpen is not true or false");
  if (p.flCodePins !== undefined) out.push(...flPinProblems(p.flCodePins));
  if (!Number.isSafeInteger(p.chainId) || (p.chainId as number) <= 0) out.push("chainId is not a chain id");
  if (expectedGenesis(p) === null) out.push("genesisHash is not a block hash");
  // network_profile.rs ready(): a name the bundled node does not build is refused (fail closed).
  if (
    typeof p.genesisProfile !== "string" ||
    !/^[a-z0-9_]+$/.test(p.genesisProfile) ||
    !Object.prototype.hasOwnProperty.call(NODE_GENESIS_PROFILES, p.genesisProfile)
  ) {
    out.push("genesisProfile is not a genesis profile the bundled node builds");
  }
  if (typeof p.nodeNetwork !== "string" || !/^[A-Za-z0-9_-]+$/.test(p.nodeNetwork)) {
    out.push("nodeNetwork is malformed");
  }
  for (const f of ["rpcUrl", "explorerUrl", "faucetUrl"] as const) {
    if (!isHttps(p[f])) out.push(`${f} is not an https URL`);
  }
  if (!Array.isArray(p.bootnodes) || p.bootnodes.length === 0) out.push("bootnodes is empty");
  else if (!p.bootnodes.every(isBootnode)) out.push("bootnodes has a malformed entry");
  if (typeof p.addressBook !== "string" || p.addressBook.length === 0) out.push("addressBook is empty");
  if (!isLowerHexNonZero(p.addressBookSha256, 64)) out.push("addressBookSha256 is not a sha256 digest");
  if (!isLowerHexNonZero(p.minChainRev, 40)) out.push("minChainRev is not a commit");
  return out;
}

/** True when every value of the profile is set and well formed (Rust `ready()`). */
export function isReady(p: NetworkProfile | null = ACTIVE_PROFILE): boolean {
  return profileProblems(p).length === 0;
}

/**
 * The genesis the network gate compares against: the profile's genesis only when the WHOLE
 * profile is ready, else null (Rust `expected_network`: any unset or malformed value gives an
 * all-unset gate). A valid genesis beside a missing addressBook is still "unset".
 */
export function gateGenesis(p: NetworkProfile | null = ACTIVE_PROFILE): string | null {
  return isReady(p) ? expectedGenesis(p) : null;
}
