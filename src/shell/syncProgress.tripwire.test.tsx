// 0.5.2 RC52 — tripwire: no sync percentage, and no catch-up rate or ETA, is shown unless both
// heights (this node's and the public head) were read. Contract §1: "N blocks behind", never a
// "% from start"; unknown is not zero.
//   1. Behaviour: whatever readings are missing, syncLine never carries a rate, an ETA or a %.
//   2. Surfaces: the sidebar status, ALF and onboarding S6 show the sync line, never "N%".
//   3. Source scan: no surface builds a percentage from syncPct, and only syncProgress.ts formats a
//      rate or an ETA (so every one goes through syncProgress(), which requires both heights).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { Sidebar } from "./Sidebar";
import { ALF } from "../surfaces/ALF";
import { Onboarding } from "../onboarding/Onboarding";
import { freshState, type AppState } from "./state";
import type { Store } from "./store";
import { emptySeries, observe, syncLine, syncProgress, type SyncProgress } from "./syncProgress";

const T0 = 1_800_000_000_000;
const RATE_OR_ETA = /blocks\/s|about \d|under a minute|%/;

describe("behaviour: no rate, ETA or % without both heights", () => {
  // Deterministic pseudo-random walk (no Math.random: reproducible on CI).
  let seed = 7;
  const rnd = () => ((seed = (seed * 1_103_515_245 + 12_345) % 2 ** 31) / 2 ** 31);

  it("a local-only series never yields a rate or an ETA", () => {
    for (let run = 0; run < 50; run++) {
      let s = emptySeries();
      let h = 1 + Math.floor(rnd() * 1e6);
      for (let i = 0; i < 200; i++) s = observe(s, "local", T0 + i * 2000, (h += Math.floor(rnd() * 200)));
      const line = syncLine(syncProgress(s, T0 + 199 * 2000));
      expect(line).not.toMatch(RATE_OR_ETA);
      expect(line).toMatch(/^Sync unknown/);
    }
  });

  it("a head-only series never yields a rate or an ETA", () => {
    for (let run = 0; run < 50; run++) {
      let s = emptySeries();
      let h = 1 + Math.floor(rnd() * 1e6);
      for (let i = 0; i < 100; i++) s = observe(s, "head", T0 + i * 5000, (h += Math.floor(rnd() * 5)));
      const line = syncLine(syncProgress(s, T0 + 99 * 5000));
      expect(line).not.toMatch(RATE_OR_ETA);
      expect(line).toMatch(/^Sync unknown/);
    }
  });

  it("an ETA appears only with a positive measured rate", () => {
    for (let run = 0; run < 100; run++) {
      let s = emptySeries();
      let l = 1_000 + Math.floor(rnd() * 1e5);
      let h = l + Math.floor(rnd() * 1e5);
      const lr = rnd() * 60;
      const hr = rnd() * 2;
      const n = Math.floor(rnd() * 150);
      for (let i = 0; i <= n; i++) {
        s = observe(s, "local", T0 + i * 2000, Math.round((l += lr * 2)));
        if (i % 3 === 0) s = observe(s, "head", T0 + i * 2000, Math.round((h += hr * 6)));
      }
      const p = syncProgress(s, T0 + n * 2000);
      const line = syncLine(p);
      expect(line).not.toContain("%");
      if (/about \d|under a minute/.test(line)) {
        expect(p.kind === "syncing" && p.phase === "catching-up" && (p.rate ?? 0) > 0 && p.etaSec != null).toBe(true);
      }
    }
  });
});

const sidebarStore = { identity: () => ({ sub: "", name: "Tester", initials: "TE", role: "member" }) } as unknown as Store;
const noopStore = {
  startNode: () => {},
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

const catchingUp: SyncProgress = { kind: "syncing", local: 100_000, head: 112_340, behind: 12_340, phase: "catching-up", rate: 41.1, etaSec: 300 };
const headUnknown: SyncProgress = { kind: "unknown", missing: "head" };

function syncing(over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  return { ...s, signedIn: true, alfMember: true, node: "syncing", height: 100_000, peers: 3, ...over };
}

describe("surfaces show the sync line, never N%", () => {
  it("sidebar: the measured line while syncing", () => {
    const html = renderToStaticMarkup(<Sidebar store={sidebarStore} s={syncing({ syncPct: 89, syncProgress: catchingUp })} />);
    expect(html).toContain("Syncing · 12,340 blocks behind · catching up ~41 blocks/s · about 5 min");
    expect(html).not.toContain("89%");
  });

  it("sidebar: unknown head shows no %, rate or ETA (node.rs's -1 is not shown)", () => {
    for (const syncPct of [-1, 0, 42]) {
      const html = renderToStaticMarkup(<Sidebar store={sidebarStore} s={syncing({ syncPct, syncProgress: headUnknown })} />);
      expect(html).toContain("Sync unknown · network head unavailable");
      expect(html).not.toContain(syncPct + "%");
      expect(html).not.toMatch(/blocks\/s|about \d/);
    }
  });

  it("sidebar: before the first poll there is no number at all", () => {
    const html = renderToStaticMarkup(<Sidebar store={sidebarStore} s={syncing({ syncPct: 42, syncProgress: null })} />);
    expect(html).toContain("Sync unknown · not read yet");
    expect(html).not.toContain("42%");
  });

  it("ALF: the same line, never N%", () => {
    const html = renderToStaticMarkup(<ALF store={noopStore} s={syncing({ syncPct: 42, syncProgress: catchingUp })} />);
    expect(html).toContain("Syncing · 12,340 blocks behind");
    expect(html).not.toContain("42%");
  });

  it("onboarding S6: the same line, no % and no progress bar", () => {
    const html = renderToStaticMarkup(<Onboarding store={noopStore} s={syncing({ stage: "s6", syncPct: 42, syncProgress: catchingUp })} />);
    expect(html).toContain("Syncing · 12,340 blocks behind · catching up ~41 blocks/s · about 5 min");
    expect(html).not.toContain("42%");
    expect(html).not.toContain("width:42%");
  });
});

// ---------------------------------------------------------------- source scan
const SRC = join(__dirname, "..");
function sources(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) sources(p, out);
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name) && !/\.fixture\./.test(name)) out.push(p);
  }
  return out;
}
const rel = (p: string) => p.slice(SRC.length + 1).replace(/\\/g, "/");

/**
 * Files that still render syncPct as a percentage on main, each with the plan that removes it.
 * Node.tsx: the K3 Node page (#563) replaces its Status; when it lands, drop this entry and use
 * syncLine there.
 */
const PCT_ALLOWLIST = new Set<string>(["surfaces/Node.tsx"]);

describe("source scan", () => {
  it("no surface builds a percentage from syncPct", () => {
    const hits: string[] = [];
    for (const f of sources(SRC)) {
      if (PCT_ALLOWLIST.has(rel(f))) continue;
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (/syncPct\b/.test(line) && /%/.test(line) && !/^\s*(\/\/|\*)/.test(line)) hits.push(rel(f) + ":" + (i + 1) + ": " + line.trim());
        });
    }
    expect(hits).toEqual([]);
  });

  it("only syncProgress.ts formats a catch-up rate or an ETA", () => {
    const hits: string[] = [];
    for (const f of sources(SRC)) {
      if (rel(f) === "shell/syncProgress.ts") continue;
      const src = readFileSync(f, "utf8");
      if (/\bformat(Rate|Eta)\s*\(|blocks\/s/.test(src)) hits.push(rel(f));
    }
    expect(hits).toEqual([]);
  });
});
