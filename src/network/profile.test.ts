import { describe, it, expect } from "vitest";
import book from "../../src-tauri/addresses/40204.json";
import { citrate } from "../chain";
import {
  ACTIVE_PROFILE,
  ACTIVE_PROFILE_ID,
  PROFILES,
  expectedGenesis,
  isReady,
  missingValues,
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

  it("a zero or malformed genesis counts as not set", () => {
    const base = { ...ACTIVE_PROFILE! };
    expect(expectedGenesis({ ...base, genesisHash: "0x" + "0".repeat(64) })).toBeNull();
    expect(expectedGenesis({ ...base, genesisHash: "0x1234" })).toBeNull();
    expect(isReady({ ...base, genesisHash: null })).toBe(false);
  });

  it("the chain, the address book and the profile agree", () => {
    expect(citrate.id).toBe(ACTIVE_PROFILE!.chainId);
    expect(citrate.rpcUrls.default.http).toEqual([ACTIVE_PROFILE!.rpcUrl]);
    expect(String(book.genesisHash).toLowerCase()).toBe(expectedGenesis());
    expect(book.chainId).toBe(ACTIVE_PROFILE!.chainId);
  });
});
