import { describe, expect, it } from "vitest";
import { isAccessLink } from "./accessLink";
import { isInviteLink } from "../surfaces/inviteLink";

const CODE = "A".repeat(43);

describe("access link (download gate)", () => {
  it("recognises citrate://access links only", () => {
    expect(isAccessLink(`citrate://access?code=${CODE}`)).toBe(true);
    expect(isAccessLink(`  citrate://access?code=${CODE}\n`)).toBe(true);
    expect(isAccessLink(`citrate://invite?g=x&t=${CODE}`)).toBe(false);
    expect(isAccessLink(`https://www.citrate.ai/download/invite?t=${CODE}`)).toBe(false);
    expect(isAccessLink("")).toBe(false);
    expect(isAccessLink(`citrate://access?code=${"A".repeat(600)}`)).toBe(false);
  });

  it("is never taken for a group invite, and a group invite never for it", () => {
    expect(isInviteLink(`citrate://access?code=${CODE}`)).toBe(false);
    expect(isAccessLink(`citrate://invite?g=x&t=${CODE}`)).toBe(false);
  });
});
