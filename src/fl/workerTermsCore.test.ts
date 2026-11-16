// Settings › Work, the wiring to core (fl_work.rs): terms and the envelope go to the sealed store
// through these commands and no other, and authority follows the stored envelope.
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
const invoke = vi.fn();
vi.mock("../bridge/tauri/invoke", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const { loadWorkerTerms, saveWorkerTerms } = await import("./workerTerms");
const { approveEnvelope, envelopeOff, envelopeView, HIC1, readEnvelope, selectAuthority, setEnvelopeForAuthority, subscribeAuthority } = await import("./authority");

const terms = { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: ["support-tickets"] };
const env = { on: true, name: "Weeknight rounds", joinsPerWeek: 3, maxHoursPerRound: 6, approvedAt: 1, offAt: null, weekStart: 1, joinsThisWeek: 2 };

beforeEach(() => {
  invoke.mockReset();
  setEnvelopeForAuthority(null);
});

describe("worker terms in core", () => {
  it("save seals through fl_work_terms_save and returns core's receipt", async () => {
    invoke.mockResolvedValueOnce({ savedAt: 5, termsHash: "ab".repeat(32), sealed: true });
    await expect(saveWorkerTerms(terms)).resolves.toEqual({ savedAt: 5, termsHash: "ab".repeat(32), sealed: true });
    expect(invoke).toHaveBeenCalledWith("fl_work_terms_save", { terms });
  });
  it("malformed terms never reach core", async () => {
    await expect(saveWorkerTerms({ ...terms, floorSaltPerUnit: "x" })).rejects.toThrow(/floor/);
    expect(invoke).not.toHaveBeenCalled();
  });
  it("load reads fl_work_terms_get (null when none)", async () => {
    invoke.mockResolvedValueOnce(null);
    await expect(loadWorkerTerms()).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith("fl_work_terms_get");
  });
});

describe("the envelope in core drives authority", () => {
  it("caps say what is left", () => {
    expect(envelopeView(env)!.caps).toEqual([
      { label: "joins this week", left: "1 of 3" },
      { label: "machine hours per round", left: "up to 6" },
    ]);
    expect(envelopeView(null)).toBeNull();
  });
  it("approve switches to HIC-2; off switches back at once, before core answers", async () => {
    invoke.mockResolvedValueOnce(env);
    await approveEnvelope({ name: "Weeknight rounds", joinsPerWeek: 3, maxHoursPerRound: 6 });
    expect(invoke).toHaveBeenCalledWith("fl_work_envelope_approve", { envelope: { name: "Weeknight rounds", joinsPerWeek: 3, maxHoursPerRound: 6 } });
    expect(selectAuthority()).toMatchObject({ level: "HIC-2", name: "Weeknight rounds" });
    let release!: (v: unknown) => void;
    invoke.mockReturnValueOnce(new Promise((r) => (release = r)));
    const p = envelopeOff();
    expect(selectAuthority()).toEqual(HIC1);
    release({ ...env, on: false, offAt: 9 });
    await p;
    expect(selectAuthority()).toEqual(HIC1);
  });
  it("the first subscriber reads the stored envelope", async () => {
    invoke.mockResolvedValueOnce(env);
    const off = subscribeAuthority(() => {});
    await vi.waitFor(() => expect(selectAuthority().level).toBe("HIC-2"));
    expect(invoke).toHaveBeenCalledWith("fl_work_envelope_get");
    off();
    invoke.mockResolvedValueOnce(null);
    await expect(readEnvelope()).resolves.toBeNull();
  });
});
