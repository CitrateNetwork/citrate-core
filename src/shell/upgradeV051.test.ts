// 0.5.1 -> 0.5.2 upgrade: the webview's saved state (UPGRADE_051_052 §1).
//
// The fixtures are what v0.5.1 (b521d98e) itself wrote: its freshState, its reconciledGrantPatch
// for a settled grant, and its Store.save() (the PERSIST_KEYS filter), run in a v0.5.1 checkout by
// the generator in fixtures/upgrade-v0.5.1/GENERATOR.md. These tests load them with this build.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { loadState, freshState, PERSIST_KEYS, STORAGE_KEY, type AppState } from "./state";
import { store } from "./store";

const DIR = join(__dirname, "../../fixtures/upgrade-v0.5.1/webview");
const fixture = (name: string): Record<string, unknown> =>
  JSON.parse(readFileSync(join(DIR, `localStorage.citrate-core-proto-v2.${name}.json`), "utf8"));

const MEMBER = fixture("member");
const ONBOARDING = fixture("onboarding-s6");

function loadFrom(blob: Record<string, unknown>): AppState {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(blob));
  return loadState();
}

describe("0.5.1 saved state loads in this build", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.useRealTimers());

  it("the fixture is a full 0.5.1 save: one value for every key 0.5.1 persisted", () => {
    // 70 keys (v0.5.1 state.ts:988 PERSIST_KEYS); a short fixture would make the checks below vacuous.
    expect(Object.keys(MEMBER).length).toBe(70);
    expect(MEMBER.stage).toBe("done");
    expect(MEMBER.s5BondStatus).toBe("Staked · unlocks at block 2,700,000");
  });

  it("every value this build still persists comes back unchanged (no data loss)", () => {
    const s = loadFrom(MEMBER) as unknown as Record<string, unknown>;
    const persisted = new Set<string>(PERSIST_KEYS as string[]);
    const checked: string[] = [];
    for (const [k, v] of Object.entries(MEMBER)) {
      if (!persisted.has(k)) continue;
      expect({ key: k, value: s[k] }).toEqual({ key: k, value: v });
      checked.push(k);
    }
    // The member's grant, wallet, journal, skills and settings are among them.
    for (const k of ["hasGrant", "s5StakeWei", "walletAddr", "jPages", "userSkills", "connections", "activity", "pins"]) {
      expect(checked).toContain(k);
    }
  });

  it("a key this build no longer persists is never restored from a 0.5.1 save", () => {
    // Removing a key from PERSIST_KEYS does not stop loadState's Object.assign from restoring the
    // value 0.5.1 saved. Whoever retires a key must also drop it on load (as contract v1.1 V10 does
    // for observed node state), or a stale 0.5.1 value lives in memory for the whole session.
    const s = loadFrom(MEMBER) as unknown as Record<string, unknown>;
    const fresh = freshState("p1") as unknown as Record<string, unknown>;
    const persisted = new Set<string>(PERSIST_KEYS as string[]);
    for (const [k, v] of Object.entries(MEMBER)) {
      if (persisted.has(k)) continue;
      if (JSON.stringify(v) === JSON.stringify(fresh[k])) continue;
      expect({ key: k, restored: s[k] }).not.toEqual({ key: k, restored: v });
    }
  });

  it("a 0.5.1 telemetry opt-in gets disclosure version 1, so join steps wait for the one-time note", () => {
    expect(MEMBER.telemetry).toBe(true);
    expect("telemetryConsentVersion" in MEMBER).toBe(false);
    const s = loadFrom(MEMBER);
    expect(s.telemetry).toBe(true);
    expect(s.telemetryConsentVersion).toBe(1);
  });

  it("a member's own choices (Hermes loop and terminal turned off) survive the one-time defaults", () => {
    expect(MEMBER.hermesSidecarLoopDefaultApplied).toBe(true);
    expect(MEMBER.hermesTerminalDefaultApplied).toBe(true);
    const s = loadFrom(MEMBER);
    expect(s.hermesSidecarLoop).toBe(false);
    expect(s.hermesTerminal).toBe(false);
  });

  it("a member part-way through onboarding resumes at the same step with the grant kept", () => {
    const s = loadFrom(ONBOARDING);
    expect(s.stage).toBe("s6");
    expect(s.hasGrant).toBe(true);
    expect(s.s5).toBe("settled");
    expect(s.telemetry).toBe(false);
    expect(s.telemetryConsentVersion).toBeNull();
  });

  it("the first save after the upgrade writes every carried value back (nothing dropped on disk)", () => {
    vi.useFakeTimers();
    store.state = loadFrom(MEMBER);
    localStorage.clear();
    store.save();
    vi.advanceTimersByTime(1000);
    const written = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Record<string, unknown>;
    expect(written).not.toBeNull();
    for (const k of PERSIST_KEYS as string[]) {
      if (!(k in MEMBER)) continue;
      expect({ key: k, value: written[k] }).toEqual({ key: k, value: MEMBER[k] });
    }
    expect(written.telemetryConsentVersion).toBe(1);
  });
});
