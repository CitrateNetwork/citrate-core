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

/** True when every value of the profile is set. */
export function isReady(p: NetworkProfile | null = ACTIVE_PROFILE): boolean {
  return p !== null && missingValues(p).length === 0 && expectedGenesis(p) !== null;
}
