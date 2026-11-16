// Audit item (#688): "Review draft" on a verified group-chat draft hands it to Open a round
// (board 07's setRoundDraft), so Open a round opens prefilled. Values Hermes did not propose stay
// empty for the member; nothing is written when the build has no Open a round draft store.
import { describe, it, expect, vi } from "vitest";
import { handOffDraft, toOpenRound } from "./roundDraftLink";

const DRAFT = { title: "Tune the support model", budgetSalt: 600, rateSalt: 1.5, workers: 8, deadline: "2026-11-22" };

describe("draft hand-off to Open a round", () => {
  it("maps the proposed values and leaves the rest for the member", () => {
    expect(toOpenRound(DRAFT, "g1", "Orchard Lab")).toEqual({
      groupId: "g1",
      template: "tune",
      dataClass: "",
      budgetSalt: "600",
      rateSalt: "1.5",
      maxWorkers: 8,
      deadline: "2026-11-22",
      who: "members",
      draftedFor: "Orchard Lab",
    });
    const sparse = toOpenRound({ title: "t", budgetSalt: null, rateSalt: null, workers: null, deadline: null }, "g1", "x");
    expect(sparse.budgetSalt).toBe("");
    expect(sparse.rateSalt).toBe("");
    expect(sparse.deadline).toBe("");
    expect("maxWorkers" in sparse).toBe(false);
  });
  it("calls setRoundDraft(draft, \"hermes\") when Open a round is in the build", async () => {
    const setRoundDraft = vi.fn();
    const ok = await handOffDraft(DRAFT, "g1", "Orchard Lab", { "../../fl/roundDraft.ts": async () => ({ setRoundDraft }) });
    expect(ok).toBe(true);
    expect(setRoundDraft).toHaveBeenCalledWith(toOpenRound(DRAFT, "g1", "Orchard Lab"), "hermes");
  });
  it("hands nothing over when the build has no draft store", async () => {
    expect(await handOffDraft(DRAFT, "g1", "x", {})).toBe(false);
    expect(await handOffDraft(DRAFT, "g1", "x", { "../../fl/roundDraft.ts": async () => ({}) })).toBe(false);
  });
});
