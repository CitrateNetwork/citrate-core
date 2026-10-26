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
  nodeNetwork: string | null;
  rpcUrl: string | null;
  explorerUrl: string | null;
  faucetUrl: string | null;
  bootnodes: string[] | null;
  addressBook: string | null;
  pending: Record<string, string>;
};

type RawProfile = Omit<NetworkProfile, "id">;

const raw = file as unknown as { active: string; profiles: Record<string, RawProfile> };

const FIELDS = [
  "chainId",
  "genesisHash",
  "nodeNetwork",
  "rpcUrl",
  "explorerUrl",
  "faucetUrl",
  "bootnodes",
  "addressBook",
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

/**
 * Why the profile is not ready: every unset value, then every malformed one. The same rule as
 * the Rust `NetworkProfile::ready` (src-tauri/src/network_profile.rs), which gates node starts
 * and every wallet signature, so the webview never treats as ready a profile Rust refuses.
 */
export function profileProblems(p: NetworkProfile | null): string[] {
  if (p === null) return ["no active network profile"];
  const missing = missingValues(p);
  if (missing.length > 0) return missing.map((m) => `${m.field} not set (${m.reason})`);
  const out: string[] = [];
  if (!Number.isSafeInteger(p.chainId) || (p.chainId as number) <= 0) out.push("chainId is not a chain id");
  if (expectedGenesis(p) === null) out.push("genesisHash is not a block hash");
  if (typeof p.nodeNetwork !== "string" || !/^[A-Za-z0-9_-]+$/.test(p.nodeNetwork)) {
    out.push("nodeNetwork is malformed");
  }
  for (const f of ["rpcUrl", "explorerUrl", "faucetUrl"] as const) {
    if (!isHttps(p[f])) out.push(`${f} is not an https URL`);
  }
  if (!Array.isArray(p.bootnodes) || p.bootnodes.length === 0) out.push("bootnodes is empty");
  else if (!p.bootnodes.every(isBootnode)) out.push("bootnodes has a malformed entry");
  if (typeof p.addressBook !== "string" || p.addressBook.length === 0) out.push("addressBook is empty");
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
