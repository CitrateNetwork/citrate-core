// =====================================================================
// citrate-core: the sim personas (L0-WP06, moved verbatim from state.ts)
//
// A leaf module: it imports nothing, so every AppState part (state/core.ts, state/l<N>.ts) can
// read the persona it builds its defaults from without importing state.ts at run time (state.ts
// imports the parts, so a part that imported state.ts would close an import cycle).
// state.ts re-exports all of it, so `import { PERSONAS, makeAddr } from "./state"` is unchanged.
// =====================================================================

export interface Persona {
  id: string;
  name: string;
  initials: string;
  label: string;
  tier: string;
  role: string;
  org: string | null;
  fresh: boolean;
  blurb: string;
  email: string;
}

export const PERSONAS: Record<string, Persona> = {
  p1: { id: "p1", name: "Dana Okafor", initials: "DO", label: "P1 · The Joiner", tier: "pilot", role: "member", org: null, fresh: true, blurb: "Fresh install — walks S0–S6.", email: "dana.okafor@fastmail.com" },
  p2: { id: "p2", name: "Marcus Bell", initials: "MB", label: "P2 · The Operator", tier: "pilot", role: "operator", org: null, fresh: false, blurb: "Onboarded, node validating, deep telemetry.", email: "mbell@protonmail.com" },
  p3: { id: "p3", name: "Priya Anand", initials: "PA", label: "P3 · The Builder", tier: "pilot", role: "builder", org: null, fresh: false, blurb: "SDKs, gateway key, agents on the socket.", email: "priya@lattice.dev" },
  p4: { id: "p4", name: "R. Calloway", initials: "RC", label: "P4 · The Org Seat", tier: "enterprise", role: "org-seat", org: "BA-7", fresh: false, blurb: "Org seat — org-scoped doors render.", email: "r.calloway@defense_prime.com" },
};

/** The persona `freshState(pid)` builds from: an unknown pid falls back to P1, as it always has. */
export function personaFor(pid: string): Persona {
  return PERSONAS[pid] || PERSONAS.p1;
}

// ------- deterministic address / hash helpers (verbatim from design) -------
export function makeAddr(seed: string): string {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < seed.length; i++) {
    h = Math.imul(h ^ seed.charCodeAt(i), 16777619) >>> 0;
  }
  let out = "";
  for (let k = 0; k < 5; k++) {
    h = Math.imul(h ^ (h >>> 13), 0x5bd1e995) >>> 0;
    out += h.toString(16).padStart(8, "0");
  }
  return "0x" + out.slice(0, 40);
}
