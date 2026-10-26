// D-11 (P-9d) — the legacy gateway key display (`gwKey`) is gone from persisted UI state. An older
// install that stored it has it dropped on load, from state and from storage, and no persisted key
// may name a secret again (tripwire).
import { beforeEach, describe, expect, it } from "vitest";
import { PERSIST_KEYS, RETIRED_KEYS, STORAGE_KEY, freshState, loadState } from "./state";

beforeEach(() => localStorage.clear());

describe("legacy gwKey is not persisted (P-9d)", () => {
  it("PERSIST_KEYS no longer lists gwKey", () => {
    expect(PERSIST_KEYS as string[]).not.toContain("gwKey");
    expect(PERSIST_KEYS as string[]).not.toContain("gwKeyFull");
  });

  it("gwKey and gwKeyFull are retired keys, dropped on load like the other retired fields", () => {
    expect(RETIRED_KEYS).toContain("gwKey");
    expect(RETIRED_KEYS).toContain("gwKeyFull");
  });

  it("an install that stored gwKey loses it on load, in state and in storage", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", rpc: "public", gwKey: "cgk_9f31•••7f2a", gwKeyFull: "cgk_legacy_value" }));
    const s = loadState() as unknown as Record<string, unknown>;
    expect("gwKey" in s).toBe(false);
    expect("gwKeyFull" in s).toBe(false);
    expect(s.rpc).toBe("public");
    const raw = localStorage.getItem(STORAGE_KEY) ?? "";
    expect(raw).not.toContain("cgk_");
    expect(raw).not.toContain("gwKey");
    expect(JSON.parse(raw).rpc).toBe("public");
  });

  it("a stored state without gwKey is left as it is", () => {
    const stored = JSON.stringify({ persona: "p1", rpc: "local" });
    localStorage.setItem(STORAGE_KEY, stored);
    loadState();
    expect(localStorage.getItem(STORAGE_KEY)).toBe(stored);
  });

  it("no sim persona seeds a gateway key", () => {
    for (const p of ["p1", "p2", "p3", "p4"]) expect("gwKey" in (freshState(p) as unknown as Record<string, unknown>)).toBe(false);
  });

  it("tripwire: no persisted key names a key, secret, token, password, seed or mnemonic", () => {
    for (const k of PERSIST_KEYS) expect(String(k), String(k)).not.toMatch(/key|secret|token|password|passphrase|seed|mnemonic/i);
  });
});
