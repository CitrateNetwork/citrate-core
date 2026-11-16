import { describe, it, expect } from "vitest";
import book from "../../src-tauri/addresses/40204.json";
import { citrate } from "../chain";
import {
  ACTIVE_PROFILE,
  ACTIVE_PROFILE_ID,
  PROFILES,
  expectedGenesis,
  gateGenesis,
  isReady,
  missingValues,
  profileProblems,
  type NetworkProfile,
} from "./profile";

describe("network profile (K6)", () => {
  it("the active profile is ready (a build cannot ship against an unset network)", () => {
    expect(ACTIVE_PROFILE?.id).toBe(ACTIVE_PROFILE_ID);
    expect(isReady()).toBe(true);
    expect(missingValues(ACTIVE_PROFILE!)).toEqual([]);
  });

  it("the next genesis is explicitly not set, with a reason for each value", () => {
    const next = PROFILES.find((p) => p.id === "genesis-2026-12");
    expect(next).toBeDefined();
    expect(isReady(next!)).toBe(false);
    expect(expectedGenesis(next!)).toBeNull();
    const missing = missingValues(next!);
    expect(missing.map((m) => m.field)).toContain("genesisHash");
    expect(missing.every((m) => m.reason !== "not recorded" && m.reason.length > 0)).toBe(true);
  });

  it("r1007 (0.5.3) is the active, ready profile after the genesis cut; r1006 is retired; a retired profile is refused", () => {
    const r1007 = PROFILES.find((p) => p.id === "r1007");
    expect(r1007).toBeDefined();
    expect(ACTIVE_PROFILE_ID).toBe("r1007");
    expect(isReady(r1007!)).toBe(true);
    expect(gateGenesis(r1007!)).toBe("0xcf68634502b4fca1e977ac8dd35aa006bb40c04208d5cd55f1c2b508bba96631");
    expect(missingValues(r1007!)).toEqual([]);
    expect(r1007!.memberPruneRetain).toBe(10_000);
    const r1006 = PROFILES.find((p) => p.id === "r1006");
    expect(r1006?.retired).toMatch(/r1007/);
    expect(isReady(r1006!)).toBe(false);
    // The Rust rule: retired is refused even with every value set; an empty reason is malformed.
    const retired: NetworkProfile = { ...ACTIVE_PROFILE!, retired: "replaced by r1007" };
    expect(isReady(retired)).toBe(false);
    expect(profileProblems(retired)).toEqual([expect.stringMatching(/retired.*replaced by r1007/)]);
    expect(gateGenesis(retired)).toBeNull();
    expect(profileProblems({ ...ACTIVE_PROFILE!, retired: " " })).toContain("retired is not a reason");
    expect(profileProblems({ ...ACTIVE_PROFILE!, memberPruneRetain: 9_999 })).toContain("memberPruneRetain is below the 10,000-block undo window");
    expect(isReady({ ...ACTIVE_PROFILE!, memberPruneRetain: 10_000 })).toBe(true);
  });

  it("a zero or malformed genesis counts as not set", () => {
    const base = { ...ACTIVE_PROFILE! };
    expect(expectedGenesis({ ...base, genesisHash: "0x" + "0".repeat(64) })).toBeNull();
    expect(expectedGenesis({ ...base, genesisHash: "0x1234" })).toBeNull();
    expect(isReady({ ...base, genesisHash: null })).toBe(false);
  });

  it("readiness follows the Rust ready() rule: set AND well formed", () => {
    const base = { ...ACTIVE_PROFILE! };
    expect(profileProblems(base)).toEqual([]);
    const cases: [Partial<typeof base>, string][] = [
      [{ addressBook: null }, "addressBook not set"],
      [{ addressBook: "" }, "addressBook is empty"],
      [{ chainId: 0 }, "chainId is not a chain id"],
      [{ nodeNetwork: "a b" }, "nodeNetwork is malformed"],
      [{ rpcUrl: "http://rpc.example.org" }, "rpcUrl is not an https URL"],
      [{ explorerUrl: "https://" }, "explorerUrl is not an https URL"],
      [{ faucetUrl: "https://a b" }, "faucetUrl is not an https URL"],
      [{ bootnodes: [] }, "bootnodes is empty"],
      [{ bootnodes: ["noise_" + "ab".repeat(32) + "@host:0"] }, "bootnodes has a malformed entry"],
      [{ bootnodes: ["noise_" + "ab".repeat(31) + "@host:30303"] }, "bootnodes has a malformed entry"],
      [{ bootnodes: ["noise_" + "ab".repeat(32) + "@ho_st:30303"] }, "bootnodes has a malformed entry"],
      [{ genesisHash: "0x" + "0".repeat(64) }, "genesisHash is not a block hash"],
      [{ addressBookSha256: null }, "addressBookSha256 not set"],
      [{ addressBookSha256: "ab".repeat(31) }, "addressBookSha256 is not a sha256 digest"],
      [{ addressBookSha256: "0".repeat(64) }, "addressBookSha256 is not a sha256 digest"],
      [{ minChainRev: null }, "minChainRev not set"],
      [{ minChainRev: "c384fc98" }, "minChainRev is not a commit"],
      [{ minChainRev: "0".repeat(40) }, "minChainRev is not a commit"],
    ];
    for (const [patch, why] of cases) {
      const p = { ...base, ...patch };
      expect(isReady(p), why).toBe(false);
      expect(profileProblems(p).join("; "), why).toContain(why);
      // The gate genesis is null whenever the profile is not ready, even with a valid genesis.
      expect(gateGenesis(p), why).toBeNull();
    }
    expect(gateGenesis(base)).toBe(expectedGenesis(base));
    expect(isReady(null)).toBe(false);
  });

  it("names a genesis profile the bundled node builds (RC52: the node builds the profile's block 0)", () => {
    const base = { ...ACTIVE_PROFILE! } as NetworkProfile & { genesisProfile: unknown };
    const as = (gp: unknown) => ({ ...base, genesisProfile: gp }) as unknown as NetworkProfile;
    expect(profileProblems(as("testnet_beta"))).toEqual([]);
    expect(profileProblems(as(null))).toEqual([expect.stringMatching(/^genesisProfile not set/)]);
    for (const bad of ["r9999", "", "r1006\n", "R1006", "testnet beta"]) {
      expect(profileProblems(as(bad)), JSON.stringify(bad)).toContain(
        "genesisProfile is not a genesis profile the bundled node builds",
      );
      expect(gateGenesis(as(bad)), JSON.stringify(bad)).toBeNull();
    }
  });

  it("the chain, the address book and the profile agree", () => {
    expect(citrate.id).toBe(ACTIVE_PROFILE!.chainId);
    expect(citrate.rpcUrls.default.http).toEqual([ACTIVE_PROFILE!.rpcUrl]);
    expect(String(book.genesisHash).toLowerCase()).toBe(expectedGenesis());
    expect(book.chainId).toBe(ACTIVE_PROFILE!.chainId);
  });
});
