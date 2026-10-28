// Tripwires (STD) for the K3 Node page: Node › Status (surfaces/Node.tsx) and the tabs under
// surfaces/node. Each guards a bug class, so a new file or a new row can't reintroduce it:
//   - one activation gate (ported from #362): Activate validator only behind activationOffer, which
//     offers only inside canOfferActivation; nothing arms, registers or activates directly;
//   - Rule 7: every row names its source, and figures come from the store's named fields or the
//     two reads this page makes itself, never a page-local chain read or a seeded field;
//   - one Services implementation: main's registry-backed NodeServices (K1a-2) is the only one;
//   - no retired 0.5.1 Node screen state (the nTab switch, sim cpu / ram / peer rows, crash seeds).
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.cwd();
const read = (p: string) => readFileSync(join(ROOT, p), "utf8");
const NODE_DIR = "src/surfaces/node";
/** The Node page's production files: Status, and every non-test, non-fixture file under surfaces/node. */
const PAGE_FILES = [
  "src/surfaces/Node.tsx",
  ...readdirSync(join(ROOT, NODE_DIR))
    .filter((f) => /\.tsx?$/.test(f) && !/\.(test|fixture)\.tsx?$/.test(f))
    .map((f) => NODE_DIR + "/" + f),
];

describe("tripwire — the Node page has one activation gate", () => {
  it("every Node page file that calls activateValidator decides first with activationOffer", () => {
    // #554: the activation entry is store.requestValidatorActivation (activateValidator is private).
    const CALL = "requestValidatorActivation(";
    let callers = 0;
    for (const f of PAGE_FILES) {
      const body = read(f);
      expect(body, f).not.toMatch(/\.activateValidator\(/);
      if (!body.includes(CALL)) continue;
      callers++;
      expect(body, f).toContain("activationOffer(");
      expect(body.indexOf("activationOffer("), f).toBeLessThan(body.indexOf(CALL));
      // One call site, behind the offer's refusal branch.
      expect(body.split(CALL).length - 1, f).toBe(1);
      expect(body.indexOf("!offer.offer"), f).toBeGreaterThan(-1);
      expect(body.indexOf(CALL), f).toBeGreaterThan(body.indexOf("!offer.offer"));
    }
    expect(callers).toBe(1);
  });

  it("activationOffer offers only inside canOfferActivation", () => {
    const words = read(NODE_DIR + "/nodeWords.ts");
    const start = words.indexOf("export function activationOffer(");
    expect(start).toBeGreaterThan(-1);
    const body = words.slice(start, words.indexOf("\n}\n", start));
    const gate = body.indexOf("if (canOfferActivation(");
    const after = body.indexOf("const word = validatorWord(");
    expect(gate).toBeGreaterThan(-1);
    expect(body.match(/offer: true/g)?.length).toBe(1);
    expect(body.indexOf("offer: true")).toBeGreaterThan(gate);
    expect(body.indexOf("offer: true")).toBeLessThan(after);
  });

  it("no Node page file arms mining, registers or activates a validator directly", () => {
    for (const f of PAGE_FILES) expect(read(f), f).not.toMatch(/armMining\(|registerValidator\(|\bactivate\(/);
  });

  it("there is no second, page-local gate on a stake amount (contract §1: stake amounts never make a validator)", () => {
    for (const f of PAGE_FILES) expect(read(f), f).not.toMatch(/bondedStake\s*>=|32[_,]?000\s*[<>]=?|[<>]=?\s*32[_,]?000|\bactivateGate\b/);
  });
});

describe("tripwire — Rule 7: every Node page figure names its source", () => {
  it("every <Row> passes a non-empty src", () => {
    let rows = 0;
    for (const f of PAGE_FILES) {
      const body = read(f);
      const re = /<Row\b/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(body))) {
        rows++;
        // The element ends at the first "/>" outside any {…} expression; its props must include src.
        let depth = 0;
        let end = m.index;
        for (; end < body.length; end++) {
          const c = body[end];
          if (c === "{") depth++;
          else if (c === "}") depth--;
          else if (depth === 0 && body.startsWith("/>", end)) break;
        }
        const el = body.slice(m.index, end);
        expect(el, f + "@" + m.index).toMatch(/\bsrc=(?:"[^"]+"|\{[^}]+)/);
      }
    }
    expect(rows).toBeGreaterThan(15);
  });

  it("figures come from the store's named fields: no page-local chain reads, no seeded or retired fields", () => {
    for (const f of PAGE_FILES) {
      const body = read(f);
      // One read path per figure: the store (refreshNode / refreshEarnings / refreshProducers) or the
      // page's two named reads (node_status probe, node_last_crash). No contract reads of its own.
      expect(body, f).not.toMatch(/useReadContract|readContract\(|validatorEarnings\(|grantStatus\(/);
      // Seeded or off-chain values never become a Node page figure.
      expect(body, f).not.toMatch(/\bs\.(blocksProposed|claimable|liquid|syncPct|cpu|ram|peerRows|crashes|earnSource)\b/);
    }
    const bridgeCalls = PAGE_FILES.flatMap((f) => [...read(f).matchAll(/bridge\.(\w+\.\w+)\(/g)].map((m) => m[1]));
    expect([...new Set(bridgeCalls)].sort()).toEqual(["node.lastCrash", "node.status"]);
  });
});

describe("tripwire — one Services implementation (main's K1a-2 registry tab)", () => {
  it("only surfaces/NodeServices.tsx renders the services list, and Node › Services routes to it", () => {
    for (const f of PAGE_FILES) {
      expect(read(f), f).not.toMatch(/services_snapshot|servicesSnapshot|ServiceEntry|export function NodeServices|PHASE_LABEL/);
    }
    expect(readdirSync(join(ROOT, NODE_DIR)).filter((f) => /services/i.test(f))).toEqual([]);
    const place = read("src/shell/PlaceView.tsx");
    const at = place.indexOf('case "node/services":');
    expect(place.slice(at, place.indexOf("case ", at + 10))).toContain("<NodeServices ");
    expect(place).toMatch(/import \{[^}]*\bNodeServices\b[^}]*\} from "\.\.\/surfaces";/);
  });
});

describe("tripwire — the 0.5.1 Node screen's state is retired", () => {
  it("nothing reads or writes nTab, cpu, ram, peerRows or crashes, and a 0.5.1 save drops nTab and crashes", () => {
    const files = (readdirSync(join(ROOT, "src"), { recursive: true }) as string[]).filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f) && !f.endsWith("nodePage.tripwire.test.ts"));
    for (const f of files) {
      // Code only: comments may name the retired keys, and RETIRED_KEYS lists them to drop them.
      const code = read("src/" + f)
        .split("\n")
        .filter((l) => !/^\s*(\/\/|\*|\/\*)/.test(l) && !l.includes("RETIRED_KEYS"))
        .join("\n");
      expect(code, f).not.toMatch(/\bnTab\b|\bpeerRows\b|\bmakePeers\b|\bcrashes:|\.crashes\b|"crashes"|\bu\.(cpu|ram)\b|\bcpu: number|\bram: number/);
    }
    expect(read("src/shell/state.ts")).toMatch(/RETIRED_KEYS[^\n]*"nTab", "crashes"/);
  });
});
