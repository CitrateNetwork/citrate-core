// @vitest-environment node
// GROW-S1 tasks 11/12/14 — v2 invite links in the app: what a pasted or deep-linked string is, how a
// join-service error reads, and that the webview no longer builds or fetches join links itself.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { classifyInviteInput, joinErrorKind, joinErrorMessage, CODE_RE, JOIN_GOALS } from "./referral";

const CODE = "Ab3dE7gh";
const KEY = "AAECAwQFBgcICQoLDA0ODw";
const src = (p: string) => readFileSync(resolve(__dirname, p), "utf8");

describe("classifyInviteInput — v2 links (both schemes) and bare codes", () => {
  it.each([
    `https://citrate.ai/join/${CODE}#${KEY}`,
    `https://www.citrate.ai/join/${CODE}#${KEY}`,
    `citrate://join/${CODE}#${KEY}`,
    `${CODE}#${KEY}`,
    `  https://citrate.ai/join/${CODE}#${KEY}  `,
  ])("%s is a v2 invite with its key", (raw) => {
    expect(classifyInviteInput(raw)).toEqual({ kind: "v2", hasKey: true });
  });

  it.each([`https://citrate.ai/join/${CODE}`, `citrate://join/${CODE}`, CODE])("%s is a v2 invite without a key", (raw) => {
    expect(classifyInviteInput(raw)).toEqual({ kind: "v2", hasKey: false });
  });

  it("an app-only relay invite stays on the existing redeem path", () => {
    expect(classifyInviteInput("citrate://invite?g=grp_1&t=tok&k=02ab")).toEqual({ kind: "relay" });
  });

  it("a legacy display link (with ?by&c&g) is display-only, never treated as a code", () => {
    const c = classifyInviteInput(`https://citrate.ai/join/${CODE}?by=dana&c=Garden`);
    expect(c.kind).toBe("legacy-referral");
    if (c.kind === "legacy-referral") expect(c.parts.inviterHandle).toBe("dana");
  });

  it.each(["", "hello there", "https://evil.example/join/Ab3dE7gh#AAECAwQFBgcICQoLDA0ODw", "citrate://pair?c=x&s=y", "https://citrate.ai/join/0O1lI000"])(
    "%j is not an invite",
    (raw) => {
      expect(classifyInviteInput(raw)).toEqual({ kind: "none" });
    },
  );

  it("the code alphabet matches the service (no 0/O/1/l/I)", () => {
    expect(CODE_RE.test(CODE)).toBe(true);
    expect(CODE_RE.test("0O1lI000")).toBe(false);
  });

  it("goals are the service enum", () => {
    expect([...JOIN_GOALS]).toEqual(["general", "storage", "training", "inference", "dapps"]);
  });
});

describe("joinErrorKind / joinErrorMessage — the Rust join errors, read honestly", () => {
  it("a 503 / unreachable service is 'unavailable' (the caller falls back)", () => {
    expect(joinErrorKind("join: invite links are temporarily unavailable")).toBe("unavailable");
    expect(joinErrorKind(new Error("join: could not reach the invite service"))).toBe("unavailable");
  });

  it("a rejected name tells the member to use Latin letters and digits", () => {
    const raw = 'join: that cluster name isn\'t allowed on an invite. Use Latin letters and digits, and avoid reserved words such as "official" or "support"';
    expect(joinErrorKind(raw)).toBe("nameNotAllowed");
    expect(joinErrorMessage(raw)).toMatch(/Latin letters and digits/);
  });

  it("sign-in, not-found and rate limit read as themselves", () => {
    expect(joinErrorKind("join: sign in to create invite links")).toBe("notSignedIn");
    expect(joinErrorKind("join: session expired or not authorized, sign in again")).toBe("notSignedIn");
    expect(joinErrorKind("join: invite not found")).toBe("notFound");
    expect(joinErrorMessage("join: invite not found")).toMatch(/expired|revoked/i);
    expect(joinErrorKind("join: too many invite requests, try again later")).toBe("rateLimited");
    expect(joinErrorKind("something else")).toBe("other");
  });
});

describe("task 14 — the webview builds and fetches no join links of its own", () => {
  it("referral.ts has no fetch, no resolver URL and no link builder", () => {
    const r = src("referral.ts");
    expect(r).not.toMatch(/\bfetch\(/);
    expect(r).not.toMatch(/api\/join/);
    expect(r).not.toMatch(/resolveJoinCode|mintJoinLink|buildJoinLink/);
    expect(r).not.toMatch(/from "jose"|import\("jose"\)/);
  });

  it("no surface builds a link with a group id or wallet address in it", () => {
    for (const f of ["Groups.tsx", "Community.tsx", "InviteLinks.tsx", "../shell/store.ts"]) {
      const s = src(f);
      expect(s, f).not.toMatch(/buildJoinLink/);
      expect(s, f).not.toMatch(/[?&]ref=|searchParams\.set\("ref"/);
    }
  });

  it("the CSP connect-src is unchanged (no citrate.ai origin for the webview)", () => {
    const conf = src("../../src-tauri/tauri.conf.json");
    const csp = /"csp":\s*"([^"]*)"/.exec(conf)?.[1] ?? "";
    const connect = /connect-src([^;]*)/.exec(csp)?.[1] ?? "";
    expect(connect).not.toMatch(/https:\/\/(www\.)?citrate\.ai(\s|$)/);
  });
});
