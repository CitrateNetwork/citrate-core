// =====================================================================
// citrate-core: the slice registry (L0-WP06, lanes/L0_INTEGRATION_SEAMS.md 3.4)
//
// New 0.6.0 lane state never goes into AppState (state.ts). It goes into a slice built with
// createSlice and registered here, so ten lanes can add state without editing one shared file.
// The registry refuses what would let two lanes collide or let state leak:
//   - an id is unique and starts with its lane ("l4.groupHistory" for lane L4);
//   - a persisted slice uses its own storage key under citrate-core.v60.*, one key per slice;
//   - persisted keys are fields of the slice, and only they are written or read back;
//   - a sealed slice (D16) may not use webview storage at all: sealed state persists through the
//     L9 sealed store only, so a sealed slice with a persist block is refused.
// Persistence (non-sealed only): the listed keys are read once at registration and written on
// every change. `device` scope uses the key as given. `network` scope keys the entry by the
// genesis its values were read on (`<key>@<genesis>`, netScope.currentNetKey), so a value read
// on one network is never restored on another; the previous network's entry stays where it was.
// =====================================================================
import type { Slice } from "./createSlice";
import { currentNetKey } from "../netScope";

export type SliceLane = `L${0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;

export interface SliceRegistration<S extends object> {
  /** `l<N>.<name>`, unique, for example "l4.groupHistory". */
  id: string;
  lane: SliceLane;
  slice: Slice<S>;
  /** Webview persistence (localStorage). Refused for a sealed slice. */
  persist?: { scope: "device" | "network"; keys: (keyof S)[]; storageKey: `citrate-core.v60.${string}` };
  /** true = may not use localStorage; persists through the L9 sealed store only (D16). */
  sealed?: boolean;
}

const LANE_RE = /^L[0-9]$/;
const STORAGE_KEY_RE = /^citrate-core\.v60\.[a-z0-9][a-z0-9._-]*$/;
const SEALED_REFUSAL = "sealed slices persist through the sealed store only";

const registered: SliceRegistration<object>[] = [];

/** The JSON kind of a value, the unit a saved value is checked against its default by. */
function kind(v: unknown): string {
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  return typeof v;
}

/** A saved value may replace a default of the same kind; a null default takes a primitive. */
function restorable(saved: unknown, initial: unknown): boolean {
  const k = kind(saved);
  if (kind(initial) === "null") return k === "null" || k === "string" || k === "number" || k === "boolean";
  return k === kind(initial);
}

function validate<S extends object>(r: SliceRegistration<S>): void {
  if (typeof r.lane !== "string" || !LANE_RE.test(r.lane)) throw new Error(`slice "${r.id}": lane must be L0 to L9`);
  const prefix = r.lane.toLowerCase() + ".";
  if (typeof r.id !== "string" || !r.id.startsWith(prefix) || r.id.length === prefix.length) {
    throw new Error(`slice "${r.id}": an ${r.lane} slice id must start with "${prefix}" and name the slice`);
  }
  if (registered.some((x) => x.id === r.id)) throw new Error(`slice "${r.id}" is already registered`);
  // Fail closed: anything other than an absent or false `sealed` counts as sealed.
  const sealed = r.sealed !== undefined && r.sealed !== false;
  if (r.persist === undefined) return;
  if (sealed) throw new Error(SEALED_REFUSAL);
  const { scope, keys, storageKey } = r.persist;
  if (scope !== "device" && scope !== "network") throw new Error(`slice "${r.id}": persist scope must be device or network`);
  if (typeof storageKey !== "string" || !STORAGE_KEY_RE.test(storageKey)) {
    throw new Error(`slice "${r.id}": storage key "${String(storageKey)}" must be citrate-core.v60.<name> (a-z, 0-9, . _ -)`);
  }
  const owner = registered.find((x) => x.persist?.storageKey === storageKey);
  if (owner) throw new Error(`slice "${r.id}": storage key "${storageKey}" is already used by ${owner.id}`);
  if (!Array.isArray(keys) || keys.length === 0) throw new Error(`slice "${r.id}": persist needs at least one key`);
  const initial = r.slice.get();
  for (const k of keys) {
    if (typeof k !== "string" || !Object.prototype.hasOwnProperty.call(initial, k)) {
      throw new Error(`slice "${r.id}": persist key "${String(k)}" is not a field of the slice`);
    }
  }
}

/** The localStorage entry a persisted slice reads and writes. */
function entryKey(persist: NonNullable<SliceRegistration<object>["persist"]>): string {
  return persist.scope === "network" ? `${persist.storageKey}@${currentNetKey()}` : persist.storageKey;
}

function restore<S extends object>(slice: Slice<S>, keys: (keyof S)[], storageKey: string): void {
  let saved: unknown;
  try {
    saved = JSON.parse(localStorage.getItem(storageKey) || "null");
  } catch {
    return;
  }
  if (kind(saved) !== "object") return;
  const blob = saved as Record<string, unknown>;
  const initial = slice.get() as Record<string, unknown>;
  const patch: Record<string, unknown> = {};
  for (const k of keys as string[]) {
    if (Object.prototype.hasOwnProperty.call(blob, k) && restorable(blob[k], initial[k])) patch[k] = blob[k];
  }
  if (Object.keys(patch).length > 0) slice.set(patch as Partial<S>);
}

function writeOnChange<S extends object>(slice: Slice<S>, keys: (keyof S)[], storageKey: string): void {
  slice.subscribe(() => {
    const s = slice.get() as Record<string, unknown>;
    const keep: Record<string, unknown> = {};
    for (const k of keys as string[]) keep[k] = s[k];
    try {
      localStorage.setItem(storageKey, JSON.stringify(keep));
    } catch {
      /* storage unavailable: the slice keeps working in memory */
    }
  });
}

/**
 * Register a lane's slice. Throws (registering nothing) when the registration breaks a rule
 * above. A persisted slice is restored from its entry before this returns.
 */
export function registerSlice<S extends object>(r: SliceRegistration<S>): Slice<S> {
  validate(r);
  if (r.persist) {
    const storageKey = entryKey(r.persist as NonNullable<SliceRegistration<object>["persist"]>);
    restore(r.slice, r.persist.keys, storageKey);
    writeOnChange(r.slice, r.persist.keys, storageKey);
  }
  registered.push(r as unknown as SliceRegistration<object>);
  return r.slice;
}

/** Every slice registered so far, in registration order. */
export function registeredSlices(): readonly SliceRegistration<object>[] {
  return [...registered];
}
