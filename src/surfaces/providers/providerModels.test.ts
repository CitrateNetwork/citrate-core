// D-11 (RC52) — the rules behind Settings › Models: which state a card is in, when a cap is reached,
// when a key is due for rotation, what Resume does, and the caps and custom-endpoint forms.
import { describe, expect, it } from "vitest";
import { NOW, DAY, usage, vendor } from "./fixtures/providerStates.fixture";
import {
  ROTATE_AFTER_DAYS,
  capReached,
  capsToForm,
  cardState,
  confirmMatches,
  customToInput,
  formToCaps,
  keyAgeDays,
  resumeEffect,
  rotationDue,
  spendLine,
  tokenLine,
} from "./providerModels";

describe("cardState", () => {
  it("no adapter in this build is 'unavailable', even with a key", () => {
    expect(cardState(vendor({ adapter: null, keyConfigured: true }), undefined)).toBe("unavailable");
  });
  it("an unconfirmed custom endpoint waits for confirmation before anything else", () => {
    expect(cardState(vendor({ kind: "custom", verified: false, confirmed: false, halt: { kind: "member", message: "paused by you", atMs: NOW } }), undefined)).toBe("unconfirmed");
  });
  it("halted beats capped beats connected", () => {
    const capped = usage({ today: { calls: 3, tokens: 1_000_000, spendMicros: 0, unpricedCalls: 3 } });
    expect(cardState(vendor({ keyConfigured: true, halt: { kind: "burst", message: "x", atMs: NOW } }), capped)).toBe("halted");
    expect(cardState(vendor({ keyConfigured: true }), capped)).toBe("capped");
    expect(cardState(vendor({ keyConfigured: true }), usage())).toBe("connected");
    expect(cardState(vendor(), usage())).toBe("disconnected");
  });
});

describe("capReached", () => {
  it("names the cap that is reached, tokens and currency both", () => {
    const caps = vendor().caps;
    expect(capReached(usage({ month: { calls: 1, tokens: 10_000_000, spendMicros: 0, unpricedCalls: 0 } }), caps)).toContain("monthly token cap");
    expect(capReached(usage({ today: { calls: 1, tokens: 5, spendMicros: 5_000_000, unpricedCalls: 0 } }), caps)).toContain("daily spending cap");
    expect(capReached(usage({ month: { calls: 1, tokens: 5, spendMicros: 50_000_000, unpricedCalls: 0 } }), caps)).toContain("monthly spending cap");
  });
  it("no cap of a kind is never reached; unknown usage is not a reached cap", () => {
    const none = { dailyTokens: null, monthlyTokens: null, dailyMicros: null, monthlyMicros: null };
    expect(capReached(usage({ today: { calls: 9, tokens: 9e12, spendMicros: 9e12, unpricedCalls: 0 } }), none)).toBeNull();
    expect(capReached(undefined, vendor().caps)).toBeNull();
  });
});

describe("rotation reminders (P-7c)", () => {
  it("flags a key at or past the rotation age, never an unknown age", () => {
    expect(ROTATE_AFTER_DAYS).toBe(90);
    expect(rotationDue(vendor({ keyConfigured: true, keySetMs: NOW - 90 * DAY }), NOW)).toBe(90);
    expect(rotationDue(vendor({ keyConfigured: true, keySetMs: NOW - 89 * DAY }), NOW)).toBeNull();
    expect(rotationDue(vendor({ keyConfigured: true, keySetMs: null }), NOW)).toBeNull();
    expect(rotationDue(vendor({ keyConfigured: false, keySetMs: NOW - 400 * DAY }), NOW)).toBeNull();
  });
  it("a set time in the future (clock skew) is not an age", () => {
    expect(keyAgeDays(NOW + DAY, NOW)).toBeNull();
    expect(keyAgeDays(0, NOW)).toBeNull();
  });
});

describe("resumeEffect", () => {
  it("says that resuming an unknown-model halt approves the model", () => {
    const r = resumeEffect("unknownModel");
    expect(r.label).toContain("approve");
    expect(r.note).toContain("approves the model");
  });
  it("says that resuming an unreadable-settings halt resets every provider", () => {
    expect(resumeEffect("guardUnreadable").note).toContain("every provider");
  });
  it("a member or automatic pause just resumes", () => {
    for (const k of ["member", "burst", "tokenSpike", "somethingNew"]) expect(resumeEffect(k).label).toBe("Resume");
  });
});

describe("usage lines", () => {
  it("shows token use against the cap, and 'no cap' when there is none", () => {
    expect(tokenLine(1200, 1_000_000)).toBe("1,200 of 1,000,000 tokens");
    expect(tokenLine(1200, null)).toBe("1,200 tokens (no cap)");
  });
  it("spend counts priced calls and says unpriced cost is unknown, not zero", () => {
    expect(spendLine({ calls: 2, tokens: 1, spendMicros: 1_250_000, unpricedCalls: 0 }, 5_000_000)).toBe("$1.25 of $5.00 spent");
    expect(spendLine({ calls: 3, tokens: 1, spendMicros: 0, unpricedCalls: 3 }, null)).toBe("$0.00 spent (no cap), plus 3 calls with no price set (cost unknown)");
  });
});

describe("caps form", () => {
  it("round-trips both token and currency caps", () => {
    const caps = vendor().caps;
    const f = capsToForm(caps);
    expect(f).toEqual({ dailyTokens: "1000000", monthlyTokens: "10000000", dailyUsd: "5.00", monthlyUsd: "50.00" });
    expect(formToCaps(f)).toEqual({ caps });
  });
  it("blank is no cap of that kind", () => {
    expect(formToCaps({ dailyTokens: "", monthlyTokens: " ", dailyUsd: "", monthlyUsd: "" })).toEqual({ caps: { dailyTokens: null, monthlyTokens: null, dailyMicros: null, monthlyMicros: null } });
  });
  it("refuses junk, negatives and a daily cap above the monthly one", () => {
    expect(formToCaps({ dailyTokens: "-5", monthlyTokens: "", dailyUsd: "", monthlyUsd: "" })).toHaveProperty("error");
    expect(formToCaps({ dailyTokens: "1e9", monthlyTokens: "", dailyUsd: "", monthlyUsd: "" })).toHaveProperty("error");
    expect(formToCaps({ dailyTokens: "", monthlyTokens: "", dailyUsd: "abc", monthlyUsd: "" })).toHaveProperty("error");
    expect(formToCaps({ dailyTokens: "10", monthlyTokens: "5", dailyUsd: "", monthlyUsd: "" })).toEqual({ error: "The daily token cap is above the monthly one." });
    expect(formToCaps({ dailyTokens: "", monthlyTokens: "", dailyUsd: "6", monthlyUsd: "5" })).toEqual({ error: "The daily spending cap is above the monthly one." });
  });
});

describe("custom endpoint form", () => {
  it("needs a name, an https URL and at least one model", () => {
    expect(customToInput({ label: "", baseUrl: "https://x.example/v1", models: "m" })).toHaveProperty("error");
    expect(customToInput({ label: "Box", baseUrl: "http://x.example/v1", models: "m" })).toHaveProperty("error");
    expect(customToInput({ label: "Box", baseUrl: "https://x.example/v1", models: " , " })).toHaveProperty("error");
    expect(customToInput({ label: " Box ", baseUrl: "https://x.example/v1", models: "a, b c" })).toEqual({ label: "Box", baseUrl: "https://x.example/v1", models: ["a", "b", "c"] });
  });
  it("confirmation needs the exact URL: no trimming, no case folding, no trailing slash", () => {
    const u = "https://llm.example.org/v1";
    expect(confirmMatches(u, u)).toBe(true);
    expect(confirmMatches(u + "/", u)).toBe(false);
    expect(confirmMatches(" " + u, u)).toBe(false);
    expect(confirmMatches(u.toUpperCase(), u)).toBe(false);
    expect(confirmMatches("", u)).toBe(false);
  });
});
