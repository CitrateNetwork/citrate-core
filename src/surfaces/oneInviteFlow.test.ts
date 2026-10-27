// @vitest-environment node
// D-12 tripwire (webview side): one invite flow, request and approve. Fails if a second invite
// mechanism comes back into the app: a join_* / pending_join_* / rendezvous_* command, a
// citrate.ai/join link, a self-admit redeem, or the v2 link surfaces. The Rust side has its own
// tripwire (invites::tests::tripwire_there_is_one_invite_flow).
import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const SRC = resolve(__dirname, "..");

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return n === "__snapshots__" ? [] : files(p);
    return /\.(ts|tsx)$/.test(n) && !/\.test\.(ts|tsx)$/.test(n) ? [p] : [];
  });
}

/** What a second invite mechanism looks like in webview code. */
export const BANNED: [RegExp, string][] = [
  [/invoke(?:<[^>]*>)?\(\s*["'`](?:join|pending_join|rendezvous)_[a-z_]+["'`]|^\s*["']?(?:join|pending_join|rendezvous)_[a-z_]+["']?\s*:\s*\d/m, "a join_* / pending_join_* / rendezvous_* command"],
  [/citrate\.ai\/join/, "a citrate.ai/join link"],
  [/citrate:\/\/join\//, "a citrate://join/ link"],
  [/invites\.(redeem|submitClaim|pollClaims|verifyConsume)\b/, "the old invite calls (self-admit / claim-back)"],
  [/group_invite_(redeem|submit_claim|poll_claims|verify_consume)|group_referral_/, "a removed invite command"],
  [/bridge\.join\b|\bpendingJoin\b/, "the v2 invite-link client"],
];

export function offences(src: string): string[] {
  return BANNED.filter(([re]) => re.test(src)).map(([, why]) => why);
}

describe("one invite flow (D-12)", () => {
  it("the scan sees what it looks for", () => {
    expect(offences(`invoke("join_mint", {})`)).toContain("a join_* / pending_join_* / rendezvous_* command");
    expect(offences(`invoke('rendezvous_tick')`)).toHaveLength(1);
    expect(offences("const LONG = {\n  join_mint: 35_000,\n};")).toHaveLength(1);
    expect(offences("const u = `https://citrate.ai/join/${code}`")).toContain("a citrate.ai/join link");
    expect(offences("await bridge.invites.redeem(link)")).toHaveLength(1);
    expect(offences("bridge.join.mint(x)")).toHaveLength(1);
    expect(offences(`invoke("group_invite_request", { link })`)).toEqual([]);
    expect(offences("reachedStep(\"join_group\")")).toEqual([]);
  });

  it("no webview source brings a second invite mechanism back", () => {
    const found: string[] = [];
    for (const f of files(SRC)) {
      for (const why of offences(readFileSync(f, "utf8"))) found.push(`${relative(SRC, f)}: ${why}`);
    }
    expect(found).toEqual([]);
  });

  it("the bridge has one invite domain, with only request-and-approve methods", () => {
    const domains = readFileSync(join(SRC, "bridge/domains.ts"), "utf8");
    expect(domains).not.toMatch(/interface JoinDomain|\bjoin: JoinDomain/);
    const tauri = readFileSync(join(SRC, "bridge/tauri/invites.ts"), "utf8");
    const cmds = Array.from(tauri.matchAll(/invoke(?:<[^>]*>)?\("([a-z_]+)"/g)).map((m) => m[1]).sort();
    expect(cmds).toEqual([
      "group_invite_approve",
      "group_invite_check",
      "group_invite_create",
      "group_invite_decline",
      "group_invite_dismiss",
      "group_invite_my_requests",
      "group_invite_preview",
      "group_invite_request",
      "group_invite_requests",
      "group_invite_revoke",
      "group_invites",
    ]);
  });
});
