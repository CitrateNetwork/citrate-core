// HUP-S7.4 (US-7.1) — the Hermes identity card model. Pure: the states come from core
// (`agent_sbt_status`), and the card offers the mint only when core says it is available.
import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { identityCard, loadAgentSbt, requestAgentSbtMint, agentSbtSlice, AGENT_SBT_STATES, type AgentSbtStatus, type AgentSbtIo } from "./agentSbt";

const base = (over: Partial<AgentSbtStatus> = {}): AgentSbtStatus => ({
  contract: "0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b",
  member: "0x1111111111111111111111111111111111111111",
  did: "did:citrate:agent:0x1111111111111111111111111111111111111111",
  parentOrgId: "3",
  balance: "0",
  tokens: [],
  tokensNote: null,
  state: "org-not-active",
  available: false,
  message: "Hermes identity is available after the network upgrade. The member organization that Hermes identities belong to is not active on chain yet.",
  ...over,
});

describe("identityCard", () => {
  it("web preview says it needs the desktop app and offers nothing", () => {
    const c = identityCard(null, { mode: "sim", loaded: true, error: null });
    expect(c.canMint).toBe(false);
    expect(c.body).toContain("desktop app");
  });

  it("before the read returns it says it is checking", () => {
    const c = identityCard(null, { mode: "tauri", loaded: false, error: null });
    expect(c.canMint).toBe(false);
    expect(c.body).toContain("Checking");
  });

  it("a failed read is shown verbatim, never as ready", () => {
    const c = identityCard(null, { mode: "tauri", loaded: true, error: "wallet locked" });
    expect(c.canMint).toBe(false);
    expect(c.body).toContain("wallet locked");
  });

  it("the live 40204 state is disabled with the network-upgrade message", () => {
    const c = identityCard(base(), { mode: "tauri", loaded: true, error: null });
    expect(c.canMint).toBe(false);
    expect(c.tone).toBe("waiting");
    expect(c.body).toContain("available after the network upgrade");
    expect(c.button).toBe("Give Hermes an identity");
  });

  it("not in the book, no code and a contract without member issuance are all disabled", () => {
    for (const state of ["not-in-book", "no-code", "member-mint-unavailable"] as const) {
      const c = identityCard(base({ state, message: "Hermes identity is available after the network upgrade." }), {
        mode: "tauri",
        loaded: true,
        error: null,
      });
      expect(c.canMint).toBe(false);
    }
  });

  it("ready offers the mint and names the ceremony", () => {
    const c = identityCard(base({ state: "ready", available: true, message: "Give Hermes an on-chain identity... Signature Ceremony" }), {
      mode: "tauri",
      loaded: true,
      error: null,
    });
    expect(c.canMint).toBe(true);
    expect(c.tone).toBe("ready");
  });

  it("never offers the mint when core says unavailable, even in the ready state name", () => {
    const c = identityCard(base({ state: "ready", available: false }), { mode: "tauri", loaded: true, error: null });
    expect(c.canMint).toBe(false);
  });

  it("minted lists the tokens with their ids and DID hash", () => {
    const c = identityCard(
      base({
        state: "minted",
        balance: "1",
        message: "Hermes has an on-chain identity (AgentSBT) bound to your wallet.",
        tokens: [{ tokenId: "4", parentOrgId: "0", did: "0x8aca81cc" + "0".repeat(56), pubkeyFingerprint: "0x21fe" + "0".repeat(60), quarantined: false }],
      }),
      { mode: "tauri", loaded: true, error: null },
    );
    expect(c.canMint).toBe(false);
    expect(c.tone).toBe("done");
    expect(c.tokens).toEqual([{ label: "AgentSBT #4", detail: "parent org #0 · did 0x8aca81cc…0000", quarantined: false }]);
  });

  it("a held token that could not be listed says so instead of an empty list", () => {
    const c = identityCard(base({ state: "minted", balance: "1", tokens: null, tokensNote: "could not list tokens: range too large" }), {
      mode: "tauri",
      loaded: true,
      error: null,
    });
    expect(c.tokens).toEqual([]);
    expect(c.note).toContain("range too large");
  });

  it("no card text uses an em dash", () => {
    const cards = [
      identityCard(null, { mode: "sim", loaded: true, error: null }),
      identityCard(null, { mode: "tauri", loaded: false, error: null }),
      identityCard(base(), { mode: "tauri", loaded: true, error: null }),
    ];
    for (const c of cards) expect(JSON.stringify(c)).not.toContain("—");
  });
});

describe("member mint (reroll 2026-10-05)", () => {
  it("the states mirror core's MintState, with the member states and no issuer state", () => {
    expect([...AGENT_SBT_STATES]).toEqual([
      "ready",
      "minted",
      "not-in-book",
      "no-code",
      "member-mint-unavailable",
      "not-member",
      "org-not-active",
      "identity-key-missing",
      "chain-unreachable",
      "reverted",
    ]);
    expect(AGENT_SBT_STATES).not.toContain("not-issuer");
  });

  it("a wallet without the membership SBT waits with core's reason and cannot mint", () => {
    const msg = "Hermes identity is for Citrate members. Your wallet does not hold the membership SBT yet, so this step unlocks once your membership is active.";
    const c = identityCard(base({ state: "not-member", message: msg }), { mode: "tauri", loaded: true, error: null });
    expect(c.canMint).toBe(false);
    expect(c.tone).toBe("waiting");
    expect(c.body).toContain("membership SBT");
  });

  it("an unknown parent org (null) is accepted and never shown as a guessed id", () => {
    const c = identityCard(base({ state: "member-mint-unavailable", parentOrgId: null }), { mode: "tauri", loaded: true, error: null });
    expect(c.canMint).toBe(false);
    expect(JSON.stringify(c)).not.toContain("org #");
  });

  it("the identity flow never mentions a registrar or the owner-only mint", () => {
    for (const f of ["src/identity/agentSbt.ts", "src/onboarding/AgentIdentityStep.tsx"]) {
      const src = readFileSync(f, "utf8");
      expect(src.toLowerCase()).not.toContain("registrar");
      expect(src).not.toMatch(/mintAgent\(/);
    }
  });
});

describe("loadAgentSbt / requestAgentSbtMint", () => {
  it("stores the status from core", async () => {
    const io: AgentSbtIo = { mode: "tauri", invoke: vi.fn().mockResolvedValue(base()) };
    await loadAgentSbt(io);
    expect(io.invoke).toHaveBeenCalledWith("agent_sbt_status");
    expect(agentSbtSlice.get().status?.state).toBe("org-not-active");
    expect(agentSbtSlice.get().loaded).toBe(true);
  });

  it("does not call core in the web preview", async () => {
    const io: AgentSbtIo = { mode: "sim", invoke: vi.fn() };
    await loadAgentSbt(io);
    expect(io.invoke).not.toHaveBeenCalled();
    expect(agentSbtSlice.get().status).toBeNull();
  });

  it("keeps a read error for the card", async () => {
    const io: AgentSbtIo = { mode: "tauri", invoke: vi.fn().mockRejectedValue(new Error("rpc down")) };
    await loadAgentSbt(io);
    expect(agentSbtSlice.get().error).toBe("rpc down");
  });

  it("the mint returns core's ceremony view, and core's refusal reaches the member verbatim", async () => {
    const view = { id: "c1", origin: "local-user", kind: "transaction", chainId: 40204, decoded: { action: "a", cost: "", destination: "" }, requiresRawAck: false };
    const ok: AgentSbtIo = { mode: "tauri", invoke: vi.fn().mockResolvedValue(view) };
    await expect(requestAgentSbtMint(ok)).resolves.toEqual(view);
    expect(ok.invoke).toHaveBeenCalledWith("agent_sbt_mint");
    const no: AgentSbtIo = { mode: "tauri", invoke: vi.fn().mockRejectedValue("Hermes identity is available after the network upgrade.") };
    await expect(requestAgentSbtMint(no)).rejects.toThrow("available after the network upgrade");
  });

  it("the mint refuses in the web preview without calling core", async () => {
    const io: AgentSbtIo = { mode: "sim", invoke: vi.fn() };
    await expect(requestAgentSbtMint(io)).rejects.toThrow("desktop app");
    expect(io.invoke).not.toHaveBeenCalled();
  });
});
