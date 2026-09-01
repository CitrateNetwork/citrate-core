// GROW-REWARDS — parameter sweep over the score exponents (α,β,γ). For each combo it evaluates the
// param-dependent fairness invariants and records pass + the min margin, so we can find the ROBUST
// region (passes with room) rather than one hand-picked point — the spec's "choose parameters by
// simulation, not by feel." Pure + fast (small nets); test-only tooling.
import { CANDIDATE_PARAMS } from "./scenarios";
import { evaluateFairness, fairnessAllPass, securityMinMargin } from "./invariants";

export interface SweepCell {
  alpha: number;
  beta: number;
  gamma: number;
  pass: boolean;
  minMargin: number;
}

/** Sweep the α/β/γ grid (all other params = CANDIDATE_PARAMS). */
export function sweep(alphas: number[], betas: number[], gammas: number[]): SweepCell[] {
  const cells: SweepCell[] = [];
  for (const alpha of alphas)
    for (const beta of betas)
      for (const gamma of gammas) {
        const P = { ...CANDIDATE_PARAMS, alpha, beta, gamma };
        const pass = fairnessAllPass(P);
        cells.push({ alpha, beta, gamma, pass, minMargin: pass ? securityMinMargin(P) : 0 });
      }
  return cells;
}

/** Cells that pass ALL invariants with at least `marginFloor` of headroom (the robust region). */
export function robustCells(cells: SweepCell[], marginFloor = 1.5): SweepCell[] {
  return cells.filter((c) => c.pass && c.minMargin >= marginFloor);
}

/** The single most robust parameter set (highest min-margin), or null if none pass. */
export function mostRobust(cells: SweepCell[]): SweepCell | null {
  const passing = cells.filter((c) => c.pass).sort((a, b) => b.minMargin - a.minMargin);
  return passing[0] ?? null;
}

/** Re-export for callers that want the raw per-invariant breakdown at a point. */
export { evaluateFairness };
