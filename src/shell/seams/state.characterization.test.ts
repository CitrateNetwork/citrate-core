// L0-WP02 SEAM-STATE: AppState as the app builds, persists and restores it, recorded at a826e441.
// L0-WP06 splits AppState into per-lane parts and composes freshState and PERSIST_SCOPE from them;
// every snapshot here must stay byte-identical through that move.
//
// Time and randomness: each value is computed twice (two clocks, two random streams) and whatever
// differs between the runs (hashes, thread ids, timestamps, the date) is masked as "<volatile>".
// The 0.5.1 save is the one v0.5.1 itself wrote (fixtures/upgrade-v0.5.1). The 0.5.9 save is the one
// Store.save() writes at a826e441, whose state.ts, store.ts and netScope.ts equal tag v0.5.9.
import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  freshState,
  loadState,
  NETWORK_KEYS,
  OBSERVED_NOT_PERSISTED,
  PERSIST_KEYS,
  PERSIST_SCOPE,
  RETIRED_KEYS,
  STORAGE_KEY,
  type AppState,
} from "../state";
import { NET_STAMP_FIELD } from "../netScope";
import { reconciledGrantPatch, store } from "../store";
import { ROOT, SEAMS_BASE, canonical, checkSnapshot, diffFixture, inputFixture, maskVolatile } from "../../../scripts/seams-snapshot.mjs";

const T1 = "2026-01-15T03:07:11.000Z";
const T2 = "2026-06-20T15:41:53.000Z";

function seeded(seed: number): () => number {
  let x = seed >>> 0;
  return () => {
    x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
    return x / 2 ** 32;
  };
}

function run(at: string, seed: number, f: () => unknown): unknown {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(at));
  const spy = vi.spyOn(Math, "random").mockImplementation(seeded(seed));
  try {
    return canonical(f());
  } finally {
    spy.mockRestore();
    vi.useRealTimers();
  }
}

/** `f()` with the values that depend on the clock or on randomness masked. */
const stable = (f: () => unknown) => maskVolatile(run(T1, 1, f), run(T2, 2, f));

function storageNow(): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i) as string;
    const raw = localStorage.getItem(k) as string;
    try {
      out[k] = JSON.parse(raw);
    } catch {
      out[k] = raw;
    }
  }
  return out;
}

/** loadState on a save, and what loading left in localStorage (the network archive, rewrites). */
function loadFrom(blob: unknown): { state: AppState; storage: Record<string, unknown> } {
  localStorage.clear();
  localStorage.setItem(STORAGE_KEY, JSON.stringify(blob));
  const state = loadState();
  return { state, storage: storageNow() };
}

/** What Store.save() writes for a settled member on this build's network (synthetic values only). */
function save059(): Record<string, unknown> {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-11-20T12:00:00.000Z"));
  try {
    localStorage.clear();
    const grant = reconciledGrantPatch({
      attributedStakeWei: "32000000000000000000000",
      attributedPrincipalWei: "32000000000000000000000",
      hasSbt: true,
      bondAddress: "0x00000000000000000000000000000000000b0bd5",
      bondDeployed: true,
      bondedStakeWei: "0",
      hasValidator: false,
      unlockBlock: 2_700_000,
      isUnlocked: false,
      isKycVerified: true,
      org: "",
    });
    if (!grant) throw new Error("grant patch was null");
    store.state = {
      ...freshState("p1"),
      ...grant,
      stage: "done",
      coachDone: true,
      tier: "member",
      s2: "verified",
      telemetry: true,
      telemetryConsentVersion: 1,
      walletAddr: "0x1111111111111111111111111111111111111111",
      commsAddr: "0x2222222222222222222222222222222222222222",
      deviceId: "dev-seams-fixture",
      chatBackend: "local",
      hermesReadAloud: true,
      sidebarCollapsed: true,
      connections: { github: true },
      jPages: [{ id: "pg-1", title: "Notes", kind: "page", pinned: true, blocks: ["first block"] }],
      jSel: "pg-1",
      userSkills: [{ id: "sk-1", name: "Summarise", description: "", instruction: "Summarise the page." }],
      pins: [{ cid: "bafybeigdyrztq7v7zv6seamsfixtureseamsfixtureseamsfixture", bond: 0, cadH: 24, nextIn: 3, last: "ok" }],
      activity: [{ id: "a1", kind: "Send", amount: "-1", hash: "0x" + "ab".repeat(32), ts: 1_795_000_000_000 }],
      sponsorUnits: 3,
    } as AppState;
    store.save();
    vi.advanceTimersByTime(1000);
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) throw new Error("Store.save() wrote nothing");
    return JSON.parse(raw) as Record<string, unknown>;
  } finally {
    vi.useRealTimers();
  }
}

const SAVE_051 = JSON.parse(
  readFileSync(join(ROOT, "fixtures/upgrade-v0.5.1/webview/localStorage.citrate-core-proto-v2.member.json"), "utf8"),
) as Record<string, unknown>;

const fixture = (name: string) => `fixtures/seams/${name}.${SEAMS_BASE}.json`;

describe("SEAM-STATE: AppState, its persisted keys and its saves", () => {
  afterEach(() => localStorage.clear());

  it("fresh_state_is_unchanged", () => {
    const masked = stable(() => freshState("p1"));
    expect(Object.keys(masked as object).length).toBeGreaterThan(150);
    expect(checkSnapshot(fixture("fresh-state-p1"), masked)).toEqual([]);
    // Mutation: a state field that is dropped, or whose default changes, is named.
    const { route: _route, ...dropped } = masked as Record<string, unknown>;
    expect(diffFixture(fixture("fresh-state-p1"), dropped)).toEqual(["- route"]);
    expect(diffFixture(fixture("fresh-state-p1"), { ...(masked as object), nodeIntent: "stop" })).toEqual(['~ nodeIntent: "run" -> "stop"']);
  });

  it("fresh_state_is_unchanged_for_every_persona", () => {
    // L0-WP06 moves the persona-dependent defaults (tier, stage, wallet address, socket path) into
    // the lane parts; the P1 snapshot alone would not show a P2 to P4 default that moved wrong.
    // An unknown id falls back to P1's persona under that id.
    for (const pid of ["p2", "p3", "p4", "zz"]) {
      const masked = stable(() => freshState(pid));
      expect(checkSnapshot(fixture(`fresh-state-${pid}`), masked), pid).toEqual([]);
    }
    expect(diffFixture(fixture("fresh-state-p4"), { ...(stable(() => freshState("p4")) as object), org: null })).toEqual(['~ org: "BA-7" -> null']);
  });

  it("persist_keys_are_unchanged", () => {
    const table = {
      storageKey: STORAGE_KEY,
      netStampField: NET_STAMP_FIELD,
      persistKeys: [...PERSIST_KEYS],
      persistScope: PERSIST_SCOPE,
      networkKeys: [...NETWORK_KEYS],
      observedNotPersisted: [...OBSERVED_NOT_PERSISTED],
      retiredKeys: [...RETIRED_KEYS],
    };
    expect(checkSnapshot(fixture("persist-keys"), table)).toEqual([]);
    // Mutation: a key that stops being persisted, or changes scope, is named.
    const { walletAddr: _w, ...lessScope } = PERSIST_SCOPE;
    expect(diffFixture(fixture("persist-keys"), { ...table, persistScope: lessScope })).toEqual(["- persistScope.walletAddr"]);
    expect(diffFixture(fixture("persist-keys"), { ...table, persistScope: { ...PERSIST_SCOPE, pins: "network" } })).toEqual(['~ persistScope.pins: "device" -> "network"']);
  });

  it("load_state_reads_a_059_save", () => {
    const input = inputFixture(fixture("save-0.5.9.member"), save059);
    const save = input.value as Record<string, unknown>;
    const allowed = new Set<string>([...PERSIST_KEYS, NET_STAMP_FIELD]);
    expect(Object.keys(save).filter((k) => !allowed.has(k))).toEqual([]);
    expect(Object.keys(save).length).toBeGreaterThan(60);
    expect(checkSnapshot(fixture("load-0.5.9-save"), stable(() => loadFrom(save)))).toEqual([]);
    expect(input.recorded, "the 0.5.9 save fixture is recorded").toBe(true);
  });

  it("load_state_reads_a_051_save", () => {
    expect(Object.keys(SAVE_051).length).toBe(70);
    expect(checkSnapshot(fixture("load-0.5.1-save"), stable(() => loadFrom(SAVE_051)))).toEqual([]);
  });

  it("load_state_never_throws_and_never_restores_observed_or_retired_keys (generated saves)", () => {
    // Property check over generated save shapes: subsets of a real save, values of the wrong
    // type, and blobs that are not an object at all.
    const rand = seeded(20261127);
    const pick = <T>(xs: T[]): T => xs[Math.floor(rand() * xs.length)];
    const junk: unknown[] = [null, 0, -1, "", "x", true, [], {}, [1, "a"], { nested: { deep: 1 } }];
    const keys = [...Object.keys(SAVE_051), ...OBSERVED_NOT_PERSISTED, ...RETIRED_KEYS, "unknownKey"];
    for (let i = 0; i < 300; i++) {
      let blob: unknown;
      if (i % 25 === 0) blob = pick(junk);
      else {
        const b: Record<string, unknown> = {};
        for (const k of keys) {
          const r = rand();
          if (r < 0.5) continue;
          b[k] = r < 0.8 && k in SAVE_051 ? SAVE_051[k] : pick(junk);
        }
        blob = b;
      }
      const s = loadFrom(blob).state as unknown as Record<string, unknown>;
      const fresh = freshState(typeof s.persona === "string" ? s.persona : "p1") as unknown as Record<string, unknown>;
      for (const k of Object.keys(fresh)) expect(k in s, `case ${i}: ${k}`).toBe(true);
      for (const k of OBSERVED_NOT_PERSISTED) expect(s[k], `case ${i}: ${k}`).toEqual(fresh[k]);
      for (const k of RETIRED_KEYS) expect(k in s, `case ${i}: ${k}`).toBe(false);
      expect([s.queue, s.toast, s.demoOpen, s.chatQueue], `case ${i}`).toEqual([[], null, false, []]);
    }
  });
});
