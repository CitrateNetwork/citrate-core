// created: 2026-10-23 | branch: test/join-e2e-gate | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
// status: active. Join-E2E harness only (scripts/ci/join-e2e.sh).
//
// The inviter's half of a direct invite, through the landing join service's OWN store code
// (src/lib/join-store.ts `reserveCode` then `attachEnvelope`, the calls POST /api/join and
// PUT /api/join/<code> make), against the harness's local Postgres via the Neon HTTP shim.
//
// What this does NOT exercise: the mint routes' bearer check. Those routes validate the caller at
// https://auth.citrate.ai/me (src/lib/join-auth.ts IDENTITY_ME_URL, a constant), and the gate makes
// no production call, so the gate's spec lists "mint through the routes" as pending a local
// identity provider. The minter id here is a fixed throwaway string, not derived from any account.
//
// Run from the landing checkout:
//   node --conditions=react-server --import tsx <this file> reserve <ttlSeconds> > code.txt
//   node --conditions=react-server --import tsx <this file> attach <code> <env-file>
// Prints only the code (reserve) or the envHash (attach). Exit 1 on any refusal.

import { readFileSync } from "node:fs";
import { attachEnvelope, reserveCode } from "@/lib/join-store";

const MINTER = "join-e2e-throwaway-minter";

async function main(argv: string[]): Promise<number> {
  const [cmd, a, b] = argv;
  if (cmd === "reserve") {
    const r = await reserveCode(
      {
        kind: "direct",
        minter: MINTER,
        clusterName: "Join E2E Group",
        goal: "training",
        inviterDisplay: null,
        inviterVerified: false,
        maxResolves: 5,
        ttlSeconds: Number(a ?? 3600),
      },
      { perWindow: 20, windowSeconds: 86_400, maxLive: 200 },
    );
    if (!r.ok) {
      console.error(`reserve refused: ${r.reason}`);
      return 1;
    }
    if (r.row.status !== "pending") {
      console.error(`reserve: expected a pending direct code, got ${r.row.status}`);
      return 1;
    }
    process.stdout.write(`${r.row.code}\n`);
    return 0;
  }
  if (cmd === "attach" && a && b) {
    const env = readFileSync(b, "utf8").trim();
    const out = await attachEnvelope(a, MINTER, env, 1);
    if (out.result !== "attached" && out.result !== "unchanged") {
      console.error(`attach refused: ${out.result}`);
      return 1;
    }
    process.stdout.write(`${out.envHash}\n`);
    return 0;
  }
  console.error("usage: seed-invite.mts reserve <ttlSeconds> | attach <code> <env-file>");
  return 2;
}

main(process.argv.slice(2)).then(
  (rc) => process.exit(rc),
  (err) => {
    console.error(`seed failed: ${(err as Error)?.name}: ${(err as Error)?.message}`);
    process.exit(1);
  },
);
