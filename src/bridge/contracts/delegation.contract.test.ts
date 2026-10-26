// D-11 bridge contract — the Cognition delegate and the cloud agent session. Rust side:
// `src-tauri/src/cognition.rs`, `hermes::hermes_session_open_cloud`.
import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { bridge } from "../index";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriDelegation } from "../tauri/delegation";
import { tauriAgentHarness } from "../tauri/agent";

const scope = { task: "t", repo: "acme/app", files: ["a.ts"], context: "" };

describe("bridge contract — delegation + cloud session (D-11)", () => {
  it("exposes the delegation domain and the cloud session op", () => {
    for (const k of ["prepare", "decline", "approve", "status"] as const) expect(typeof bridge.delegation[k]).toBe("function");
    expect(typeof bridge.agentHarness.sessionOpenCloud).toBe("function");
  });

  it("sim mode is honest (Rule 1): nothing is delegated or opened in a browser preview", async () => {
    if (bridge.mode !== "sim") return;
    await expect(bridge.delegation.prepare(scope)).rejects.toThrow(/desktop app/);
    await expect(bridge.delegation.approve("dg-1", "h")).rejects.toThrow(/desktop app/);
    await expect(bridge.agentHarness.sessionOpenCloud("p", "[]", "openai", "gpt-4o")).rejects.toThrow(/desktop app/);
  });

  it("invokes exactly the Rust commands with their arguments (the approval carries no scope)", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    await tauriDelegation.prepare(scope);
    await tauriDelegation.decline("dg-1");
    await tauriDelegation.approve("dg-1", "hash");
    await tauriDelegation.status("dg-1");
    await tauriAgentHarness.sessionOpenCloud("sys", "[]", "openai", "gpt-4o", { persona: "builder" });
    const calls = invokeMock.mock.calls.map((c) => [c[0], c[1]]);
    expect(calls).toEqual([
      ["delegation_prepare", { scope }],
      ["delegation_decline", { delegationId: "dg-1" }],
      ["delegation_approve", { delegationId: "dg-1", scopeHash: "hash" }],
      ["delegation_status", { delegationId: "dg-1" }],
      ["hermes_session_open_cloud", { systemPrompt: "sys", toolsJson: "[]", providerId: "openai", model: "gpt-4o", persona: "builder" }],
    ]);
  });

  it("every command is registered in Rust and allowed for the main window", () => {
    const root = join(__dirname, "..", "..", "..", "src-tauri");
    const lib = readFileSync(join(root, "src", "lib.rs"), "utf8");
    const acl = readFileSync(join(root, "permissions", "main-window.toml"), "utf8");
    for (const cmd of ["delegation_prepare", "delegation_decline", "delegation_approve", "delegation_status", "hermes_session_open_cloud"]) {
      expect(lib).toMatch(new RegExp(`::${cmd},`));
      expect(acl).toContain(`"${cmd}"`);
    }
  });
});
