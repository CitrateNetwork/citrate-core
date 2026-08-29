// CX-S7.2 — the first-run coach must lead a non-technical user "in a Group with a model running"
// (gS-ia): the guided walk includes a Groups step whose CTA opens the Groups surface. Guards against
// a regression that drops the group step (the pre-S7 coach never mentioned Groups at all).
import { describe, it, expect } from "vitest";
import { COACH_STEPS } from "../data/seed";

describe("First-run coach IA — leads to a Group (gS-ia / S7.2)", () => {
  it("includes a Groups step", () => {
    expect(COACH_STEPS.some((s) => s.id === "groups")).toBe(true);
  });

  it("the Groups step carries a CTA that navigates to the Groups surface", () => {
    const groups = COACH_STEPS.find((s) => s.id === "groups");
    expect(groups?.cta).toBeDefined();
    expect(groups?.cta?.route).toBe("groups");
    expect((groups?.cta?.label ?? "").length).toBeGreaterThan(0);
  });

  it("ties the group to the running model (a model is up by S6.5)", () => {
    const groups = COACH_STEPS.find((s) => s.id === "groups");
    expect(`${groups?.title} ${groups?.body}`.toLowerCase()).toContain("model");
  });
});
