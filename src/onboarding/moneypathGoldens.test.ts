// =====================================================================
// G7 (0.5.2 RC): the money-path e2e goldens through the UI's truth labels.
//
// scripts/e2e/moneypath-anvil.sh records core's Activate-validator read model
// (validator_activation::read_activation_status) at each step of the flows on the
// genesis contract set, into src-tauri/tests/fixtures/moneypath/*.golden.json. This
// test feeds each recorded read through the same pure functions the step renders
// with (validatorState, activationPhase) and checks the word and phase in
// labels.json. So a golden rewrite that changes what the member would be told
// fails here, in CI, without anvil.
// =====================================================================
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import type { ActivationStatusView } from "../bridge/domains";
import { activationPhase, validatorState } from "./activateValidator";

// Paths from the repo root, where vitest runs (as the other fixture-reading tests do).
const DIR = "src-tauri/tests/fixtures/moneypath/";
const read = (name: string) => JSON.parse(readFileSync(DIR + name, "utf8"));

type Expect = { validatorWord: string | null; phase: string };
const LABELS: Record<string, Record<string, Expect> | string> = read("labels.json");
const FLOWS = ["membership-activation", "bond-exit"];

/** The recorded read, with the golden's normalized placeholders read as "not read". */
function view(recorded: Record<string, unknown>): ActivationStatusView {
  const st = { ...recorded } as Record<string, unknown>;
  for (const k of ["blockTimeMs", "unlockTimestamp"]) if (typeof st[k] === "string") st[k] = null;
  return st as unknown as ActivationStatusView;
}

describe("G7 money-path goldens: truth labels", () => {
  for (const flow of FLOWS) {
    const golden = read(`${flow}.golden.json`);
    const expected = (LABELS[flow] ?? {}) as Record<string, Expect>;

    it(`${flow}: every recorded activation read has a label expectation, and every expectation a step`, () => {
      const recorded = golden.steps
        .filter((s: { core: Record<string, unknown> }) => s.core.activationStatus)
        .map((s: { step: string }) => s.step);
      expect(Object.keys(expected).sort()).toEqual([...recorded].sort());
    });

    for (const s of golden.steps as { step: string; core: { activationStatus?: Record<string, unknown> } }[]) {
      if (!s.core.activationStatus) continue;
      it(`${flow} / ${s.step}: ${expected[s.step]?.validatorWord} (${expected[s.step]?.phase})`, () => {
        const st = view(s.core.activationStatus!);
        expect(validatorState(st).word).toBe(expected[s.step]?.validatorWord);
        expect(activationPhase(st, true)).toBe(expected[s.step]?.phase);
      });
    }
  }
});
