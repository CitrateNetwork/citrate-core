// Every daemon run outcome has its own label in the run log (Hermes › Work and Controls › Records).
// A new RunOutcome must get a label here, never fall through to its raw id.
import { describe, expect, it } from "vitest";
import type { RunOutcome } from "./api";
import { OUTCOME_LABEL, outcomeLabel } from "./DaemonRunLog";

// `satisfies` makes this list fail to typecheck when RunOutcome gains or loses a member.
const EVERY_OUTCOME = Object.keys({
  answered: 1,
  failed: 1,
  stopped: 1,
  timed_out: 1,
  over_budget: 1,
  not_started: 1,
} satisfies Record<RunOutcome, 1>) as RunOutcome[];

describe("daemon run outcome labels", () => {
  it.each(EVERY_OUTCOME)("%s has its own label and colour", (o) => {
    const l = OUTCOME_LABEL[o];
    expect(l, o).toBeDefined();
    expect(l.text.trim().length).toBeGreaterThan(0);
    expect(l.text).not.toContain("_");
    expect(l.color).toMatch(/^var\(--/);
    expect(outcomeLabel(o)).toBe(l);
  });

  it("a run that never started reads 'not started' in the muted colour, never as a failure", () => {
    expect(OUTCOME_LABEL.not_started).toEqual({ text: "not started", color: "var(--tx-3)" });
  });
});
