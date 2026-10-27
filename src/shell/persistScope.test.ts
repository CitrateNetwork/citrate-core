// RC52 tripwire: every value the webview persists has a scope, and every network-scoped one is
// keyed to the genesis it was read on.
//
// - The table below lists every persisted AppState key with its scope. A new persisted key fails
//   here until it is classified (and the compiler requires a scope in PERSIST_SCOPE).
// - Every network-scoped key is checked by behaviour, not by name: a value saved on one genesis is
//   never restored on another, and is archived. Every device-scoped key survives the switch.
// - Every localStorage entry the app reads or writes is listed with its scope. A new entry fails
//   here until it is listed; a network-scoped one must be keyed by genesis, like the archive.
import { beforeEach, describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { NETWORK_KEYS, PERSIST_KEYS, PERSIST_SCOPE, STORAGE_KEY, loadState } from "./state";
import { readNetArchive } from "./netScope";

const SCOPES: Record<string, "network" | "device"> = {
  persona: "device", tier: "device", org: "device", citrateRole: "device", entitlement: "device",
  stage: "device", s2: "device", s3: "device",
  s5: "network", s5n: "network", hasGrant: "network", hasSbt: "network",
  liquid: "network", bondedStake: "network", claimable: "network", activity: "network",
  nodeIntent: "device", rpc: "device", net: "device", cpuCap: "device", autolock: "device",
  channel: "device", telemetry: "device", telemetryConsentVersion: "device", storageMode: "device",
  coachDone: "device", dataDir: "device", coreMembershipUrl: "device",
  s5StakeWei: "network", s5BondStatus: "network",
  walletAddr: "device", commsAddr: "device", socketPath: "device", activeModelId: "device",
  kycOutcome: "device", chatBackend: "device", crashes: "device", wTab: "device", nTab: "device",
  cTab: "device", sSec: "device", route: "device", deviceId: "device",
  pins: "device", userSkills: "device", jPages: "device", jSel: "device", connections: "device",
  aiDefault: "device", hermesSidecarLoop: "device", hermesSidecarLoopDefaultApplied: "device",
  hermesTerminal: "device", hermesTerminalDefaultApplied: "device", hermesBrief: "device",
  hermesPersona: "device", customPersonas: "device", hermesReadAloud: "device",
  journalAutoSummary: "device", journalAutoSummaryDay: "device",
  sponsorUnits: "network", blocksProposed: "network",
  netUnread: "network", networkNotice: "device",
};

/** Device keys that loadState migrates on purpose, so a stored value is not returned as is. */
const MIGRATED_ON_LOAD: Record<string, string> = {
  telemetryConsentVersion: "a stored opt-in without a numeric version gets version 1",
  hermesSidecarLoopDefaultApplied: "the one-time loop default is marked applied",
};

const OLD = "0x" + "1".repeat(64);
const NEW = "0x" + "2".repeat(64);
const sentinel = (k: string) => ({ rc52Sentinel: k });

beforeEach(() => localStorage.clear());

describe("persisted state scope tripwire", () => {
  it("every persisted key is listed here with its scope", () => {
    expect(PERSIST_SCOPE).toEqual(SCOPES);
    expect([...PERSIST_KEYS].sort()).toEqual(Object.keys(SCOPES).sort());
  });

  it("every network-scoped key saved on one genesis is archived, never restored on another", () => {
    expect(NETWORK_KEYS.length).toBeGreaterThan(0);
    for (const k of NETWORK_KEYS) {
      localStorage.clear();
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ netGenesis: OLD, [k]: sentinel(k) }));
      const s = loadState(NEW) as unknown as Record<string, unknown>;
      expect({ key: k, restored: s[k] }).not.toEqual({ key: k, restored: sentinel(k) });
      const archive = readNetArchive();
      expect({ key: k, archived: archive[0]?.values[k] }).toEqual({ key: k, archived: sentinel(k) });
      expect(archive[0].genesis).toBe(OLD);
    }
  });

  it("every network-scoped key saved on the same genesis is restored", () => {
    for (const k of NETWORK_KEYS) {
      localStorage.clear();
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ netGenesis: NEW, [k]: sentinel(k) }));
      const s = loadState(NEW) as unknown as Record<string, unknown>;
      expect({ key: k, restored: s[k] }).toEqual({ key: k, restored: sentinel(k) });
    }
  });

  it("every device-scoped key survives a genesis switch", () => {
    const device = PERSIST_KEYS.filter((k) => PERSIST_SCOPE[k] === "device" && !(k in MIGRATED_ON_LOAD));
    for (const k of device) {
      localStorage.clear();
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ netGenesis: OLD, hermesSidecarLoopDefaultApplied: true, [k]: sentinel(k) }),
      );
      const s = loadState(NEW) as unknown as Record<string, unknown>;
      const want = k === "networkNotice" ? { from: OLD, to: NEW } : sentinel(k);
      if (k === "networkNotice") expect(s[k]).toMatchObject(want);
      else expect({ key: k, restored: s[k] }).toEqual({ key: k, restored: want });
    }
  });
});

// ---- localStorage entries -------------------------------------------------------------------

/** Every localStorage key expression the app uses, by file, with its scope. */
const STORAGE_ENTRIES: Record<string, string> = {
  "src/shell/state.ts STORAGE_KEY": "per key: PERSIST_SCOPE, stamped with its genesis",
  "src/shell/store.ts STORAGE_KEY": "per key: PERSIST_SCOPE, stamped with its genesis",
  "src/ErrorBoundary.tsx STORAGE_KEY": "reads the telemetry consent (device)",
  "src/privacy/joinFunnel.ts STORAGE_KEY": "reads the telemetry consent (device)",
  "src/privacy/joinFunnel.ts JOIN_FUNNEL_STORAGE_KEY": "device: this install's join steps",
  "src/shell/slices/groups.ts HISTORY_KEY": "device: group chat history drained from the member's daemon",
  "src/agent/sidecarProvider.ts key": "device: the Hermes sidecar session",
  "src/shell/netScope.ts NET_ARCHIVE_KEY": "network, keyed by genesis: the previous networks' archive",
  "src/shell/netScope.ts `${NET_ARCHIVE_KEY}.damaged-${entry.archivedAt}`": "a damaged archive set aside",
};

function sources(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...sources(p));
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name)) out.push(p);
  }
  return out;
}

describe("localStorage entry tripwire", () => {
  it("every localStorage key the app uses is listed with its scope", () => {
    const root = join(__dirname, "../..");
    const found = new Set<string>();
    for (const f of sources(join(root, "src"))) {
      const text = readFileSync(f, "utf8");
      for (const m of text.matchAll(/localStorage\.(?:getItem|setItem|removeItem)\(\s*([^,)]+)/g)) {
        found.add(`${relative(root, f)} ${m[1].trim()}`);
      }
    }
    expect([...found].sort()).toEqual(Object.keys(STORAGE_ENTRIES).sort());
  });
});
